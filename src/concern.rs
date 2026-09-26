//! Detection of review-worthy signals ("Possible concerns").
//!
//! Signals come from file paths (Phase 1) and from diff contents, schema
//! and manifest comparisons collected in [`Analysis`] (Phase 2).

use std::fmt;

use serde::Serialize;

use crate::analysis::Analysis;
use crate::category::{Category, classify, is_dependency_manifest};
use crate::deps::DependencyChange;
use crate::diff::ChangeKind;
use crate::migration;
use crate::schema::SchemaChange;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Concern {
    /// New or modified migration files.
    Migration { count: usize },
    /// A migration that removes, renames or rewrites existing data.
    DestructiveMigration {
        file: String,
        statements: Vec<String>,
    },
    /// The DB schema dump changed.
    SchemaChanged { changes: Vec<SchemaChange> },
    /// Route definitions changed.
    RoutesChanged {
        added: Vec<String>,
        removed: Vec<String>,
    },
    /// Dependency manifests or lockfiles changed.
    DependenciesChanged {
        files: Vec<String>,
        changes: Vec<DependencyChange>,
    },
    /// Production-only configuration or credentials changed.
    ProductionConfigChanged { files: Vec<String> },
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
            Self::DestructiveMigration { file, .. } => {
                write!(f, "Destructive migration: {file}")
            }
            Self::SchemaChanged { changes } if changes.iter().any(SchemaChange::is_destructive) => {
                write!(f, "DB schema changed (tables or columns removed)")
            }
            Self::SchemaChanged { .. } => write!(f, "DB schema changed"),
            Self::RoutesChanged { removed, .. } if !removed.is_empty() => {
                write!(f, "API route changed (routes removed)")
            }
            Self::RoutesChanged { .. } => write!(f, "API route changed"),
            Self::DependenciesChanged { files, .. } => {
                write!(f, "Dependencies changed ({})", files.join(", "))
            }
            Self::ProductionConfigChanged { .. } => write!(f, "Production config changed"),
            Self::FilesDeleted { count } => write!(f, "{count} file(s) deleted"),
            Self::NoTestChanges => write!(f, "Code changed without test changes"),
        }
    }
}

impl Concern {
    /// Supporting lines shown under the headline.
    pub fn details(&self) -> Vec<String> {
        match self {
            Self::DestructiveMigration { statements, .. } => statements.clone(),
            Self::SchemaChanged { changes } => changes.iter().map(ToString::to_string).collect(),
            Self::RoutesChanged { added, removed } => removed
                .iter()
                .map(|l| format!("- {l}"))
                .chain(added.iter().map(|l| format!("+ {l}")))
                .collect(),
            Self::DependenciesChanged { changes, .. } => {
                changes.iter().map(ToString::to_string).collect()
            }
            Self::ProductionConfigChanged { files } => files.clone(),
            Self::Migration { .. } | Self::FilesDeleted { .. } | Self::NoTestChanges => vec![],
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

fn is_production_config(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    path.starts_with("config/environments/production")
        || path.starts_with("config/credentials")
        || name.starts_with(".env.production")
}

/// Rails routing DSL keywords that define or scope routes.
const ROUTE_KEYWORDS: &[&str] = &[
    "get",
    "post",
    "put",
    "patch",
    "delete",
    "match",
    "root",
    "resources",
    "resource",
    "namespace",
    "scope",
    "mount",
    "concern",
    "concerns",
    "member",
    "collection",
    "direct",
    "devise_for",
];

fn route_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<String> {
    lines
        .map(str::trim)
        .filter(|line| {
            let word = line
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .next()
                .unwrap_or("");
            ROUTE_KEYWORDS.contains(&word)
        })
        .map(str::to_string)
        .collect()
}

/// Inspects the analysis and returns the concerns in a stable order.
pub fn detect(analysis: &Analysis) -> Vec<Concern> {
    let changes = &analysis.changes;
    let mut concerns = Vec::new();

    let migrations: Vec<&str> = changes
        .iter()
        .filter(|c| c.kind != ChangeKind::Deleted && is_migration(&c.path))
        .map(|c| c.path.as_str())
        .collect();
    if !migrations.is_empty() {
        concerns.push(Concern::Migration {
            count: migrations.len(),
        });
    }
    for path in &migrations {
        let Some(patch) = analysis.patch(path) else {
            continue;
        };
        let statements = migration::destructive_statements(patch.added_lines());
        if !statements.is_empty() {
            concerns.push(Concern::DestructiveMigration {
                file: path.to_string(),
                statements,
            });
        }
    }

    if changes.iter().any(|c| is_schema(&c.path)) {
        concerns.push(Concern::SchemaChanged {
            changes: analysis.schema.clone(),
        });
    }

    let route_patches: Vec<_> = changes
        .iter()
        .filter(|c| is_routes(&c.path))
        .map(|c| analysis.patch(&c.path))
        .collect();
    if !route_patches.is_empty() {
        let patches = route_patches.iter().flatten();
        concerns.push(Concern::RoutesChanged {
            added: patches
                .clone()
                .flat_map(|p| route_lines(p.added_lines()))
                .collect(),
            removed: patches
                .flat_map(|p| route_lines(p.removed_lines()))
                .collect(),
        });
    }

    let dependency_files: Vec<String> = changes
        .iter()
        .filter(|c| is_dependency_manifest(&c.path))
        .map(|c| c.path.clone())
        .collect();
    if !dependency_files.is_empty() {
        concerns.push(Concern::DependenciesChanged {
            files: dependency_files,
            changes: analysis.dependencies.values().flatten().cloned().collect(),
        });
    }

    let production: Vec<String> = changes
        .iter()
        .filter(|c| is_production_config(&c.path))
        .map(|c| c.path.clone())
        .collect();
    if !production.is_empty() {
        concerns.push(Concern::ProductionConfigChanged { files: production });
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
    use crate::diff::FileChange;
    use crate::patch::{FileDiff, Hunk};

    fn change(kind: ChangeKind, path: &str) -> FileChange {
        FileChange {
            kind,
            path: path.to_string(),
            old_path: None,
        }
    }

    fn patch(path: &str, removed: &[&str], added: &[&str]) -> FileDiff {
        FileDiff {
            path: path.to_string(),
            binary: false,
            hunks: vec![Hunk {
                old_start: 1,
                new_start: 1,
                removed: removed.iter().map(|s| s.to_string()).collect(),
                added: added.iter().map(|s| s.to_string()).collect(),
            }],
        }
    }

    fn detect_paths(changes: Vec<FileChange>) -> Vec<Concern> {
        detect(&Analysis::from_changes(changes, vec![]))
    }

    #[test]
    fn detects_rails_feature_concerns() {
        let concerns = detect_paths(vec![
            change(ChangeKind::Added, "db/migrate/20260926_add_status.rb"),
            change(ChangeKind::Modified, "db/schema.rb"),
            change(ChangeKind::Modified, "config/routes.rb"),
            change(ChangeKind::Modified, "app/services/user_service.rb"),
            change(ChangeKind::Modified, "spec/services/user_service_spec.rb"),
        ]);
        assert_eq!(
            concerns,
            vec![
                Concern::Migration { count: 1 },
                Concern::SchemaChanged { changes: vec![] },
                Concern::RoutesChanged {
                    added: vec![],
                    removed: vec![]
                },
            ]
        );
    }

    #[test]
    fn flags_code_without_tests() {
        let concerns = detect_paths(vec![change(ChangeKind::Modified, "app/models/user.rb")]);
        assert_eq!(concerns, vec![Concern::NoTestChanges]);
    }

    #[test]
    fn flags_dependencies_and_deletions() {
        let concerns = detect_paths(vec![
            change(ChangeKind::Modified, "Gemfile"),
            change(ChangeKind::Modified, "Gemfile.lock"),
            change(ChangeKind::Deleted, "README.old"),
        ]);
        assert_eq!(
            concerns,
            vec![
                Concern::DependenciesChanged {
                    files: vec!["Gemfile".into(), "Gemfile.lock".into()],
                    changes: vec![],
                },
                Concern::FilesDeleted { count: 1 },
            ]
        );
    }

    #[test]
    fn deleted_migration_is_not_counted_as_new_migration() {
        let concerns = detect_paths(vec![change(ChangeKind::Deleted, "db/migrate/1_old.rb")]);
        assert_eq!(concerns, vec![Concern::FilesDeleted { count: 1 }]);
    }

    #[test]
    fn docs_only_change_has_no_concerns() {
        assert!(detect_paths(vec![change(ChangeKind::Modified, "README.md")]).is_empty());
    }

    #[test]
    fn detects_destructive_migration_from_contents() {
        let path = "db/migrate/2_remove_flag.rb";
        let analysis = Analysis::from_changes(
            vec![change(ChangeKind::Added, path)],
            vec![patch(
                path,
                &[],
                &["  def change", "    remove_column :users, :flag", "  end"],
            )],
        );
        assert_eq!(
            detect(&analysis),
            vec![
                Concern::Migration { count: 1 },
                Concern::DestructiveMigration {
                    file: path.to_string(),
                    statements: vec!["remove_column :users, :flag".to_string()],
                },
            ]
        );
    }

    #[test]
    fn lists_changed_routes() {
        let analysis = Analysis::from_changes(
            vec![change(ChangeKind::Modified, "config/routes.rb")],
            vec![patch(
                "config/routes.rb",
                &["    get :legacy"],
                &[
                    "    resources :users, only: [:index]",
                    "    end",
                    "  # note",
                ],
            )],
        );
        let concerns = detect(&analysis);
        assert_eq!(
            concerns[0],
            Concern::RoutesChanged {
                added: vec!["resources :users, only: [:index]".into()],
                removed: vec!["get :legacy".into()],
            }
        );
        assert_eq!(
            concerns[0].to_string(),
            "API route changed (routes removed)"
        );
        assert_eq!(
            concerns[0].details(),
            vec!["- get :legacy", "+ resources :users, only: [:index]"]
        );
    }

    #[test]
    fn flags_production_config() {
        let concerns = detect_paths(vec![
            change(ChangeKind::Modified, "config/environments/production.rb"),
            change(ChangeKind::Modified, "config/environments/development.rb"),
        ]);
        assert_eq!(
            concerns,
            vec![Concern::ProductionConfigChanged {
                files: vec!["config/environments/production.rb".into()]
            }]
        );
    }
}
