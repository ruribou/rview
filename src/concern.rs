//! Detection of review-worthy signals ("Possible concerns") from a set of
//! changed files.

use std::fmt;

use serde::Serialize;

use crate::category::{Category, classify, is_dependency_manifest};
use crate::diff::{ChangeKind, FileChange};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Concern {
    /// New or modified migration files.
    Migration { count: usize },
    /// The DB schema dump changed.
    SchemaChanged,
    /// Route definitions changed.
    RoutesChanged,
    /// Dependency manifests or lockfiles changed.
    DependenciesChanged { files: Vec<String> },
    /// Files were deleted.
    FilesDeleted { count: usize },
    /// Logic or API code changed but no test file was touched.
    NoTestChanges,
}

impl fmt::Display for Concern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Migration { count: 1 } => write!(f, "Migration detected"),
            Self::Migration { count } => write!(f, "Migration detected ({count} files)"),
            Self::SchemaChanged => write!(f, "DB schema changed"),
            Self::RoutesChanged => write!(f, "API route changed"),
            Self::DependenciesChanged { files } => {
                write!(f, "Dependencies changed ({})", files.join(", "))
            }
            Self::FilesDeleted { count } => write!(f, "{count} file(s) deleted"),
            Self::NoTestChanges => write!(f, "Code changed without test changes"),
        }
    }
}

fn is_migration(path: &str) -> bool {
    path.starts_with("db/migrate/") || path.split('/').any(|c| c == "migrations")
}

fn is_schema(path: &str) -> bool {
    matches!(
        path,
        "db/schema.rb" | "db/structure.sql" | "prisma/schema.prisma"
    )
}

fn is_routes(path: &str) -> bool {
    path == "config/routes.rb" || path.starts_with("config/routes/")
}

/// Inspects the changes and returns the concerns in a stable order.
pub fn detect(changes: &[FileChange]) -> Vec<Concern> {
    let mut concerns = Vec::new();

    let migrations = changes
        .iter()
        .filter(|c| c.kind != ChangeKind::Deleted && is_migration(&c.path))
        .count();
    if migrations > 0 {
        concerns.push(Concern::Migration { count: migrations });
    }

    if changes.iter().any(|c| is_schema(&c.path)) {
        concerns.push(Concern::SchemaChanged);
    }

    if changes.iter().any(|c| is_routes(&c.path)) {
        concerns.push(Concern::RoutesChanged);
    }

    let dependency_files: Vec<String> = changes
        .iter()
        .filter(|c| is_dependency_manifest(&c.path))
        .map(|c| c.path.clone())
        .collect();
    if !dependency_files.is_empty() {
        concerns.push(Concern::DependenciesChanged {
            files: dependency_files,
        });
    }

    let deleted = changes
        .iter()
        .filter(|c| c.kind == ChangeKind::Deleted)
        .count();
    if deleted > 0 {
        concerns.push(Concern::FilesDeleted { count: deleted });
    }

    let categories: Vec<Category> = changes.iter().map(|c| classify(&c.path)).collect();
    let touches_code = categories
        .iter()
        .any(|c| matches!(c, Category::Logic | Category::Api));
    let touches_tests = categories.contains(&Category::Test);
    if touches_code && !touches_tests {
        concerns.push(Concern::NoTestChanges);
    }

    concerns
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
    fn detects_rails_feature_concerns() {
        let changes = vec![
            change(ChangeKind::Added, "db/migrate/20260926_add_status.rb"),
            change(ChangeKind::Modified, "db/schema.rb"),
            change(ChangeKind::Modified, "config/routes.rb"),
            change(ChangeKind::Modified, "app/services/user_service.rb"),
            change(ChangeKind::Modified, "spec/services/user_service_spec.rb"),
        ];
        assert_eq!(
            detect(&changes),
            vec![
                Concern::Migration { count: 1 },
                Concern::SchemaChanged,
                Concern::RoutesChanged,
            ]
        );
    }

    #[test]
    fn flags_code_without_tests() {
        let changes = vec![change(ChangeKind::Modified, "app/models/user.rb")];
        assert_eq!(detect(&changes), vec![Concern::NoTestChanges]);
    }

    #[test]
    fn flags_dependencies_and_deletions() {
        let changes = vec![
            change(ChangeKind::Modified, "Gemfile"),
            change(ChangeKind::Modified, "Gemfile.lock"),
            change(ChangeKind::Deleted, "README.old"),
        ];
        assert_eq!(
            detect(&changes),
            vec![
                Concern::DependenciesChanged {
                    files: vec!["Gemfile".into(), "Gemfile.lock".into()]
                },
                Concern::FilesDeleted { count: 1 },
            ]
        );
    }

    #[test]
    fn deleted_migration_is_not_counted_as_new_migration() {
        let changes = vec![change(ChangeKind::Deleted, "db/migrate/1_old.rb")];
        assert_eq!(detect(&changes), vec![Concern::FilesDeleted { count: 1 }]);
    }

    #[test]
    fn docs_only_change_has_no_concerns() {
        let changes = vec![change(ChangeKind::Modified, "README.md")];
        assert!(detect(&changes).is_empty());
    }
}
