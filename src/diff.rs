//! Parsing of `git diff --name-status -z` output into structured data.

use serde::Serialize;
use thiserror::Error;

/// The kind of change git reports for a single file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
}

impl ChangeKind {
    /// Parses a git status token such as `M`, `A`, or `R087`.
    ///
    /// Renames and copies carry a similarity score (`R100`), so only the
    /// first character is significant.
    fn from_status(status: &str) -> Option<Self> {
        match status.chars().next()? {
            'A' => Some(Self::Added),
            'M' => Some(Self::Modified),
            'D' => Some(Self::Deleted),
            'R' => Some(Self::Renamed),
            'C' => Some(Self::Copied),
            'T' => Some(Self::TypeChanged),
            _ => None,
        }
    }

    /// Single-letter marker used in terminal output.
    pub fn marker(self) -> char {
        match self {
            Self::Added => 'A',
            Self::Modified => 'M',
            Self::Deleted => 'D',
            Self::Renamed => 'R',
            Self::Copied => 'C',
            Self::TypeChanged => 'T',
        }
    }

    /// Whether git reports two paths (source and destination) for this kind.
    fn has_two_paths(self) -> bool {
        matches!(self, Self::Renamed | Self::Copied)
    }
}

/// A single changed file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileChange {
    pub kind: ChangeKind,
    /// The path after the change (the new path for renames).
    pub path: String,
    /// The path before a rename or copy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("unknown change status `{0}`")]
    UnknownStatus(String),
    #[error("missing path after status `{0}`")]
    MissingPath(String),
}

/// Parses the NUL-separated output of `git diff --name-status -z`.
///
/// The format is a flat sequence of fields:
/// `STATUS\0PATH\0` for most changes and `STATUS\0OLD\0NEW\0` for
/// renames and copies. Using `-z` means paths are never quoted or escaped.
pub fn parse_name_status(input: &str) -> Result<Vec<FileChange>, ParseError> {
    let mut fields = input.split('\0').filter(|f| !f.is_empty());
    let mut changes = Vec::new();

    while let Some(status) = fields.next() {
        let kind = ChangeKind::from_status(status)
            .ok_or_else(|| ParseError::UnknownStatus(status.to_string()))?;
        let mut next_path = || {
            fields
                .next()
                .map(str::to_string)
                .ok_or_else(|| ParseError::MissingPath(status.to_string()))
        };

        let change = if kind.has_two_paths() {
            let old_path = next_path()?;
            let path = next_path()?;
            FileChange {
                kind,
                path,
                old_path: Some(old_path),
            }
        } else {
            FileChange {
                kind,
                path: next_path()?,
                old_path: None,
            }
        };
        changes.push(change);
    }

    Ok(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(kind: ChangeKind, path: &str) -> FileChange {
        FileChange {
            kind,
            path: path.to_string(),
            old_path: None,
        }
    }

    #[test]
    fn parses_basic_statuses() {
        let input = "M\0app/services/user_service.rb\0A\0db/migrate/1_add.rb\0D\0app/old.rb\0";
        let changes = parse_name_status(input).unwrap();
        assert_eq!(
            changes,
            vec![
                change(ChangeKind::Modified, "app/services/user_service.rb"),
                change(ChangeKind::Added, "db/migrate/1_add.rb"),
                change(ChangeKind::Deleted, "app/old.rb"),
            ]
        );
    }

    #[test]
    fn parses_rename_with_similarity_score() {
        let changes = parse_name_status("R087\0lib/old.rb\0lib/new.rb\0").unwrap();
        assert_eq!(
            changes,
            vec![FileChange {
                kind: ChangeKind::Renamed,
                path: "lib/new.rb".to_string(),
                old_path: Some("lib/old.rb".to_string()),
            }]
        );
    }

    #[test]
    fn keeps_paths_with_spaces_and_tabs_intact() {
        let changes = parse_name_status("M\0docs/my file\twith tab.md\0").unwrap();
        assert_eq!(changes[0].path, "docs/my file\twith tab.md");
    }

    #[test]
    fn empty_input_yields_no_changes() {
        assert_eq!(parse_name_status("").unwrap(), vec![]);
    }

    #[test]
    fn rejects_unknown_status() {
        assert_eq!(
            parse_name_status("Z\0foo\0"),
            Err(ParseError::UnknownStatus("Z".to_string()))
        );
    }

    #[test]
    fn rejects_truncated_rename() {
        assert_eq!(
            parse_name_status("R100\0only_old\0"),
            Err(ParseError::MissingPath("R100".to_string()))
        );
    }
}
