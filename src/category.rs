//! Path-based classification of changed files into review categories.

use std::fmt;

use serde::Serialize;

/// A review-oriented bucket for a changed file.
///
/// The declaration order is also the display order (via `Ord`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Db,
    Api,
    Logic,
    Test,
    Config,
    Other,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::Db => "DB",
            Self::Api => "API",
            Self::Logic => "Logic",
            Self::Test => "Test",
            Self::Config => "Config",
            Self::Other => "Other",
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// Classifies a repository-relative path (always `/`-separated, as git
/// reports it).
///
/// Rules are checked from most to least specific: a spec under
/// `spec/models/` is a Test, and `config/routes.rb` is API rather than
/// Config.
pub fn classify(path: &str) -> Category {
    if is_test(path) {
        Category::Test
    } else if is_db(path) {
        Category::Db
    } else if is_api(path) {
        Category::Api
    } else if is_logic(path) {
        Category::Logic
    } else if is_config(path) {
        Category::Config
    } else {
        Category::Other
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// True when `dir` appears as a whole directory component of `path`.
fn has_dir(path: &str, dir: &str) -> bool {
    path.split('/')
        .rev()
        .skip(1) // the file name itself
        .any(|component| component == dir)
}

fn starts_with_any(path: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| path.starts_with(p))
}

fn is_test(path: &str) -> bool {
    const TEST_DIRS: &[&str] = &["spec", "test", "tests", "__tests__"];
    const TEST_SUFFIXES: &[&str] = &[
        "_spec.rb",
        "_test.rb",
        "_test.go",
        "_test.py",
        "_test.rs",
        ".test.js",
        ".test.jsx",
        ".test.ts",
        ".test.tsx",
        ".spec.js",
        ".spec.jsx",
        ".spec.ts",
        ".spec.tsx",
    ];

    let name = file_name(path);
    TEST_DIRS.iter().any(|dir| has_dir(path, dir))
        || TEST_SUFFIXES.iter().any(|s| name.ends_with(s))
        || (name.starts_with("test_") && name.ends_with(".py"))
}

fn is_db(path: &str) -> bool {
    const DB_FILES: &[&str] = &[
        "db/schema.rb",
        "db/structure.sql",
        "db/seeds.rb",
        "prisma/schema.prisma",
    ];

    starts_with_any(path, &["db/migrate/"])
        || DB_FILES.contains(&path)
        || has_dir(path, "migrations")
        || path.ends_with(".sql")
}

fn is_api(path: &str) -> bool {
    const API_DIRS: &[&str] = &[
        "app/controllers/",
        "app/graphql/",
        "app/serializers/",
        "config/routes/",
    ];
    const API_FILES: &[&str] = &[
        "openapi.yaml",
        "openapi.yml",
        "openapi.json",
        "swagger.yaml",
        "swagger.yml",
        "swagger.json",
    ];

    path == "config/routes.rb"
        || starts_with_any(path, API_DIRS)
        || API_FILES.contains(&file_name(path))
}

fn is_logic(path: &str) -> bool {
    const LOGIC_DIRS: &[&str] = &[
        "app/models/",
        "app/services/",
        "app/jobs/",
        "app/workers/",
        "app/policies/",
        "app/forms/",
        "app/interactors/",
        "app/usecases/",
        "lib/",
        "src/",
    ];

    starts_with_any(path, LOGIC_DIRS)
}

/// Dependency manifests and lockfiles. Also used by concern detection.
pub fn is_dependency_manifest(path: &str) -> bool {
    const MANIFESTS: &[&str] = &[
        "Gemfile",
        "Gemfile.lock",
        "package.json",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "bun.lockb",
        "Cargo.toml",
        "Cargo.lock",
        "go.mod",
        "go.sum",
        "requirements.txt",
        "pyproject.toml",
        "poetry.lock",
        "uv.lock",
        "composer.json",
        "composer.lock",
    ];

    MANIFESTS.contains(&file_name(path))
}

fn is_config(path: &str) -> bool {
    const CONFIG_EXTS: &[&str] = &[".yml", ".yaml", ".toml", ".ini", ".json"];

    let name = file_name(path);
    let at_root = !path.contains('/');

    starts_with_any(path, &["config/", ".github/", ".circleci/"])
        || is_dependency_manifest(path)
        || name == "Dockerfile"
        || name.starts_with("docker-compose")
        || name.starts_with(".env")
        || (at_root && name.starts_with('.'))
        || (at_root && CONFIG_EXTS.iter().any(|ext| name.ends_with(ext)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_category(expected: Category, paths: &[&str]) {
        for path in paths {
            assert_eq!(classify(path), expected, "path: {path}");
        }
    }

    #[test]
    fn classifies_db() {
        assert_category(
            Category::Db,
            &[
                "db/migrate/20260926_add_status_to_users.rb",
                "db/schema.rb",
                "db/structure.sql",
                "prisma/schema.prisma",
                "backend/migrations/0001_init.sql",
                "sql/report.sql",
            ],
        );
    }

    #[test]
    fn classifies_api() {
        assert_category(
            Category::Api,
            &[
                "app/controllers/api/users_controller.rb",
                "config/routes.rb",
                "config/routes/admin.rb",
                "docs/openapi.yaml",
            ],
        );
    }

    #[test]
    fn classifies_logic() {
        assert_category(
            Category::Logic,
            &[
                "app/services/user_service.rb",
                "app/models/user.rb",
                "lib/tasks/cleanup.rake",
                "src/main.rs",
            ],
        );
    }

    #[test]
    fn classifies_test() {
        assert_category(
            Category::Test,
            &[
                "spec/services/user_service_spec.rb",
                "test/models/user_test.rb",
                "tests/cli.rs",
                "src/components/__tests__/Button.tsx",
                "src/utils/date.test.ts",
                "pkg/server/handler_test.go",
                "test_utils.py",
            ],
        );
    }

    #[test]
    fn classifies_config() {
        assert_category(
            Category::Config,
            &[
                "config/application.rb",
                ".github/workflows/ci.yml",
                "Gemfile",
                "Gemfile.lock",
                "Cargo.toml",
                "frontend/package.json",
                "Dockerfile",
                "docker-compose.yml",
                ".rubocop.yml",
                ".env.example",
            ],
        );
    }

    #[test]
    fn falls_back_to_other() {
        assert_category(
            Category::Other,
            &["README.md", "docs/design.md", "app/assets/logo.png"],
        );
    }

    #[test]
    fn test_dir_must_be_a_whole_component() {
        // "latest/" and "contest.rb" contain "test" but are not test paths.
        assert_eq!(classify("app/models/contest.rb"), Category::Logic);
        assert_eq!(classify("latest/notes.md"), Category::Other);
    }
}
