//! Collects everything the report needs from git.
//!
//! This is the only module (besides `git`) that performs I/O. The resulting
//! [`Analysis`] is plain data, so concern detection and rendering can be
//! tested without a repository.

use std::collections::BTreeMap;
use std::path::Path;

use thiserror::Error;

use crate::deps::{self, DependencyChange};
use crate::diff::{ChangeKind, FileChange, ParseError, parse_name_status};
use crate::git::{self, GitError};
use crate::patch::{FileDiff, PatchError, parse_patch};
use crate::schema::{self, SchemaChange};

const SCHEMA_RB: &str = "db/schema.rb";

#[derive(Debug, Default)]
pub struct Analysis {
    pub changes: Vec<FileChange>,
    /// Content changes keyed by [`FileDiff::path`].
    pub patches: BTreeMap<String, FileDiff>,
    /// Dependency changes keyed by manifest path.
    pub dependencies: BTreeMap<String, Vec<DependencyChange>>,
    /// Table / column changes in `db/schema.rb`, if it changed.
    pub schema: Vec<SchemaChange>,
    /// Non-fatal problems (e.g. a manifest that failed to parse).
    pub warnings: Vec<String>,
}

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("failed to parse git diff --name-status output: {0}")]
    NameStatus(#[from] ParseError),
    #[error("failed to parse git diff output: {0}")]
    Patch(#[from] PatchError),
}

impl Analysis {
    /// Builds an analysis from already-fetched data. Useful for tests.
    pub fn from_changes(changes: Vec<FileChange>, patches: Vec<FileDiff>) -> Self {
        Self {
            changes,
            patches: patches.into_iter().map(|p| (p.path.clone(), p)).collect(),
            ..Self::default()
        }
    }

    pub fn patch(&self, path: &str) -> Option<&FileDiff> {
        self.patches.get(path)
    }
}

/// Runs git and gathers the analysis for `base...head`.
pub fn collect(repo: &Path, base: &str, head: &str) -> Result<Analysis, AnalyzeError> {
    let changes = parse_name_status(&git::diff_name_status(repo, base, head)?)?;
    let patches = parse_patch(&git::diff_patch(repo, base, head)?)?;
    let mut analysis = Analysis::from_changes(changes, patches);

    let needs_contents = analysis
        .changes
        .iter()
        .any(|c| deps::is_supported_manifest(&c.path) || c.path == SCHEMA_RB);
    if !needs_contents {
        return Ok(analysis);
    }

    let merge_base = git::merge_base(repo, base, head)?;
    let files = Revisions {
        repo,
        old: &merge_base,
        new: head,
    };

    for change in &analysis.changes {
        if deps::is_supported_manifest(&change.path) {
            match compare_manifest(&files, change) {
                Ok(dep_changes) => {
                    analysis
                        .dependencies
                        .insert(change.path.clone(), dep_changes);
                }
                Err(message) => analysis.warnings.push(message),
            }
        } else if change.path == SCHEMA_RB {
            let (old, new) = files.read_both(change)?;
            analysis.schema = schema::compare(
                &schema::parse_schema_rb(&old),
                &schema::parse_schema_rb(&new),
            );
        }
    }

    Ok(analysis)
}

/// The two sides of the comparison.
struct Revisions<'a> {
    repo: &'a Path,
    old: &'a str,
    new: &'a str,
}

impl Revisions<'_> {
    /// Reads the file before and after the change. A side where the file
    /// does not exist reads as empty.
    fn read_both(&self, change: &FileChange) -> Result<(String, String), GitError> {
        let old = match change.kind {
            ChangeKind::Added | ChangeKind::Copied => String::new(),
            _ => {
                let old_path = change.old_path.as_deref().unwrap_or(&change.path);
                git::show_file(self.repo, self.old, old_path)?
            }
        };
        let new = match change.kind {
            ChangeKind::Deleted => String::new(),
            _ => git::show_file(self.repo, self.new, &change.path)?,
        };
        Ok((old, new))
    }
}

fn compare_manifest(
    files: &Revisions<'_>,
    change: &FileChange,
) -> Result<Vec<DependencyChange>, String> {
    let (old, new) = files.read_both(change).map_err(|e| e.to_string())?;
    let parse = |content: &str| -> Result<deps::Deps, String> {
        if content.is_empty() {
            return Ok(deps::Deps::new());
        }
        deps::parse_manifest(&change.path, content)
            .map(Option::unwrap_or_default)
            .map_err(|e| format!("{}: {e}", change.path))
    };
    Ok(deps::compare(&parse(&old)?, &parse(&new)?))
}
