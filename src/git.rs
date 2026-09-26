//! Thin wrapper around the `git` command line.

use std::io;
use std::path::Path;
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum GitError {
    #[error("failed to run git (is it installed and on PATH?): {0}")]
    Spawn(#[source] io::Error),
    #[error("`git {args}` failed: {stderr}")]
    Failed { args: String, stderr: String },
    #[error("git output was not valid UTF-8")]
    InvalidUtf8(#[source] std::string::FromUtf8Error),
}

/// Runs `git diff --name-status -z -M <base>...<head>` in `repo` and
/// returns its raw stdout.
///
/// The three-dot range compares `head` against the merge base with `base`,
/// which is what a pull request shows.
pub fn diff_name_status(repo: &Path, base: &str, head: &str) -> Result<String, GitError> {
    let range = format!("{base}...{head}");
    run(repo, &["diff", "--name-status", "-z", "-M", &range, "--"])
}

fn run(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(GitError::Spawn)?;

    if !output.status.success() {
        return Err(GitError::Failed {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
        });
    }

    String::from_utf8(output.stdout).map_err(GitError::InvalidUtf8)
}
