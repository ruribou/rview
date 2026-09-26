//! End-to-end tests that run the `rview` binary against a temporary git
//! repository.

use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::Command as BinCommand;
use predicates::prelude::*;
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=rview-test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .status()
        .expect("git should run");
    assert!(status.success(), "git {args:?} failed");
}

fn write(repo: &Path, path: &str, contents: &str) {
    let full = repo.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, contents).unwrap();
}

/// Creates a repo with an initial commit on `main` and a `feature` branch
/// checked out.
fn setup_repo() -> TempDir {
    let dir = TempDir::new().unwrap();
    let repo = dir.path();

    git(repo, &["init", "-q", "-b", "main"]);
    write(
        repo,
        "app/services/user_service.rb",
        "class UserService; end\n",
    );
    write(
        repo,
        "app/services/old_service.rb",
        "class OldService; end\n",
    );
    write(repo, "config/application.rb", "# app\n");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "initial"]);
    git(repo, &["checkout", "-q", "-b", "feature"]);

    dir
}

fn rview(repo: &Path) -> BinCommand {
    let mut cmd = BinCommand::cargo_bin("rview").unwrap();
    cmd.arg("-C").arg(repo);
    cmd
}

#[test]
fn categorizes_feature_branch_changes() {
    let dir = setup_repo();
    let repo = dir.path();

    write(repo, "db/migrate/20260926_add_status_to_users.rb", "# m\n");
    write(repo, "db/schema.rb", "# schema\n");
    write(repo, "config/routes.rb", "# routes\n");
    write(repo, "app/controllers/api/users_controller.rb", "# c\n");
    write(
        repo,
        "app/services/user_service.rb",
        "class UserService; def x; end; end\n",
    );
    write(repo, "spec/services/user_service_spec.rb", "# spec\n");
    write(repo, "config/application.rb", "# app changed\n");
    git(repo, &["rm", "-q", "app/services/old_service.rb"]);
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "feature"]);

    let expected = "\
Changes: main...HEAD

[DB]
  A db/migrate/20260926_add_status_to_users.rb
  A db/schema.rb

[API]
  A app/controllers/api/users_controller.rb
  A config/routes.rb

[Logic]
  D app/services/old_service.rb
  M app/services/user_service.rb

[Test]
  A spec/services/user_service_spec.rb

[Config]
  M config/application.rb

⚠ Possible concerns
  - Migration detected
  - DB schema changed
  - API route changed
  - 1 file(s) deleted
";

    rview(repo).arg("main").assert().success().stdout(expected);
}

#[test]
fn defaults_base_to_main() {
    let dir = setup_repo();
    let repo = dir.path();

    write(repo, "README.md", "# hi\n");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "docs"]);

    rview(repo)
        .assert()
        .success()
        .stdout(predicate::str::contains("[Other]\n  A README.md"));
}

#[test]
fn reports_renames() {
    let dir = setup_repo();
    let repo = dir.path();

    git(
        repo,
        &[
            "mv",
            "app/services/old_service.rb",
            "app/services/legacy_service.rb",
        ],
    );
    git(repo, &["commit", "-q", "-m", "rename"]);

    rview(repo)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "R app/services/old_service.rb -> app/services/legacy_service.rb",
        ));
}

#[test]
fn reports_no_changes() {
    let dir = setup_repo();

    rview(dir.path())
        .assert()
        .success()
        .stdout("No changes: main...HEAD\n");
}

#[test]
fn outputs_json() {
    let dir = setup_repo();
    let repo = dir.path();

    write(repo, "Gemfile", "source 'https://rubygems.org'\n");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "deps"]);

    let output = rview(repo).args(["--format", "json"]).output().unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["base"], "main");
    assert_eq!(json["groups"][0]["category"], "config");
    assert_eq!(json["groups"][0]["files"][0]["path"], "Gemfile");
    assert_eq!(json["concerns"][0]["kind"], "dependencies_changed");
}

#[test]
fn fails_on_unknown_revision() {
    let dir = setup_repo();

    rview(dir.path())
        .arg("does-not-exist")
        .assert()
        .failure()
        .stderr(predicate::str::contains("git diff"));
}
