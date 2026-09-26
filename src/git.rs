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

/// Options shared by every diff invocation so that user configuration
/// (external diff tools, color, relative paths, custom prefixes) cannot
/// change the output format.
const DIFF_FLAGS: &[&str] = &[
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
    "--no-relative",
    "-M",
];

/// Runs `git diff --name-status -z -M <base>...<head>` in `repo` and
/// returns its raw stdout.
///
/// The three-dot range compares `head` against the merge base with `base`,
/// which is what a pull request shows.
pub fn diff_name_status(repo: &Path, base: &str, head: &str) -> Result<String, GitError> {
    let range = format!("{base}...{head}");
    let mut args = vec!["diff", "--name-status", "-z"];
    args.extend(DIFF_FLAGS);
    args.extend([range.as_str(), "--"]);
    run(repo, &args)
}

/// Runs `git diff -U0` for the same range and returns the patch text.
pub fn diff_patch(repo: &Path, base: &str, head: &str) -> Result<String, GitError> {
    let range = format!("{base}...{head}");
    let mut args = vec![
        "-c",
        "core.quotePath=false",
        "diff",
        "-U0",
        "--src-prefix=a/",
        "--dst-prefix=b/",
    ];
    args.extend(DIFF_FLAGS);
    args.extend([range.as_str(), "--"]);
    let patch = run_bytes(repo, &args)?;
    // Changed files may contain non-UTF-8 text; don't fail the whole run.
    Ok(String::from_utf8_lossy(&patch).into_owned())
}

/// Returns the merge base commit of `base` and `head`.
pub fn merge_base(repo: &Path, base: &str, head: &str) -> Result<String, GitError> {
    Ok(run(repo, &["merge-base", base, head])?.trim().to_string())
}

/// Returns the contents of `path` at revision `rev`.
pub fn show_file(repo: &Path, rev: &str, path: &str) -> Result<String, GitError> {
    let object = format!("{rev}:{path}");
    let bytes = run_bytes(repo, &["cat-file", "blob", &object])?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn run(repo: &Path, args: &[&str]) -> Result<String, GitError> {
    String::from_utf8(run_bytes(repo, args)?).map_err(GitError::InvalidUtf8)
}

fn run_bytes(repo: &Path, args: &[&str]) -> Result<Vec<u8>, GitError> {
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

    Ok(output.stdout)
}
