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

const BASE_SCHEMA: &str = r#"ActiveRecord::Schema[7.1].define(version: 2026_09_01_000000) do
  create_table "users", force: :cascade do |t|
    t.string "name", null: false
    t.string "legacy_flag"
  end
end
"#;

const BASE_ROUTES: &str = "\
Rails.application.routes.draw do
  namespace :api do
    get :legacy_status
  end
end
";

/// Creates a Rails-like repo with an initial commit on `main` and a
/// `feature` branch checked out.
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
    write(repo, "config/routes.rb", BASE_ROUTES);
    write(repo, "db/schema.rb", BASE_SCHEMA);
    write(
        repo,
        "Gemfile",
        "gem \"rails\", \"7.1.0\"\ngem \"rack-cors\"\n",
    );
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
fn analyzes_feature_branch() {
    let dir = setup_repo();
    let repo = dir.path();

    write(
        repo,
        "db/migrate/20260926_add_status_to_users.rb",
        "\
class AddStatusToUsers < ActiveRecord::Migration[7.1]
  def change
    add_column :users, :status, :integer, default: 0
    remove_column :users, :legacy_flag, :string
  end
end
",
    );
    write(
        repo,
        "db/schema.rb",
        &BASE_SCHEMA
            .replace("2026_09_01", "2026_09_26")
            .replace("\"legacy_flag\"", "\"status\", default: 0")
            .replace("t.string \"status\"", "t.integer \"status\""),
    );
    write(
        repo,
        "config/routes.rb",
        &BASE_ROUTES.replace(
            "    get :legacy_status\n",
            "    resources :users, only: [:index, :update]\n",
        ),
    );
    write(
        repo,
        "app/controllers/api/users_controller.rb",
        "class Api::UsersController; end\n",
    );
    write(
        repo,
        "app/services/user_service.rb",
        "class UserService\n  def activate; end\nend\n",
    );
    write(repo, "spec/services/user_service_spec.rb", "# spec\n");
    write(repo, "config/application.rb", "# app changed\n");
    write(
        repo,
        "Gemfile",
        "gem \"rails\", \"7.2.0\"\ngem \"sidekiq\", \"~> 7.0\"\n",
    );
    git(repo, &["rm", "-q", "app/services/old_service.rb"]);
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "feature"]);

    let expected = "\
Changes: main...HEAD

[DB]
  A db/migrate/20260926_add_status_to_users.rb (+6 -0)
  M db/schema.rb (+2 -2)

[API]
  A app/controllers/api/users_controller.rb (+1 -0)
  M config/routes.rb (+1 -1)

[Logic]
  D app/services/old_service.rb (+0 -1)
  M app/services/user_service.rb (+3 -1)

[Test]
  A spec/services/user_service_spec.rb (+1 -0)

[Config]
  M Gemfile (+2 -2)
  M config/application.rb (+1 -1)

⚠ Possible concerns
  - Migration detected
  - Destructive migration: db/migrate/20260926_add_status_to_users.rb
      remove_column :users, :legacy_flag, :string
  - DB schema changed (tables or columns removed)
      - column users.legacy_flag
      + column users.status
  - API route changed (routes removed)
      - get :legacy_status
      + resources :users, only: [:index, :update]
  - Dependencies changed (Gemfile)
      - rack-cors *
      ~ rails 7.1.0 → 7.2.0
      + sidekiq ~> 7.0
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

    write(repo, "Gemfile", "gem \"rails\", \"7.1.0\"\n");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "deps"]);

    let output = rview(repo).args(["--format", "json"]).output().unwrap();
    assert!(output.status.success());

    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["base"], "main");
    assert_eq!(json["groups"][0]["category"], "config");
    assert_eq!(json["groups"][0]["files"][0]["path"], "Gemfile");
    assert_eq!(json["groups"][0]["files"][0]["deletions"], 1);
    assert_eq!(json["concerns"][0]["kind"], "dependencies_changed");
    assert_eq!(json["concerns"][0]["changes"][0]["change"], "removed");
    assert_eq!(json["concerns"][0]["changes"][0]["name"], "rack-cors");
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

#[test]
fn warns_on_invalid_manifest_but_still_reports() {
    let dir = setup_repo();
    let repo = dir.path();

    write(repo, "package.json", "{ not json");
    git(repo, &["add", "."]);
    git(repo, &["commit", "-q", "-m", "broken"]);

    rview(repo)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Dependencies changed (package.json)",
        ))
        .stderr(predicate::str::contains(
            "warning: package.json: invalid package.json",
        ));
}
