//! Dependency manifest parsing and comparison.
//!
//! Instead of guessing from diff lines, both versions of a manifest are
//! parsed and compared, so moved or reformatted entries are not reported
//! as changes.

use std::collections::BTreeMap;
use std::fmt;

use serde::Serialize;
use thiserror::Error;

/// Dependency name → version requirement (or a source description such
/// as `path: ../foo` when there is no version).
pub type Deps = BTreeMap<String, String>;

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("invalid package.json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid Cargo.toml: {0}")]
    Toml(#[from] toml::de::Error),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "change", rename_all = "lowercase")]
pub enum DependencyChange {
    Added {
        name: String,
        version: String,
    },
    Removed {
        name: String,
        version: String,
    },
    Updated {
        name: String,
        from: String,
        to: String,
    },
}

impl fmt::Display for DependencyChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Added { name, version } => write!(f, "+ {name} {version}"),
            Self::Removed { name, version } => write!(f, "- {name} {version}"),
            Self::Updated { name, from, to } => write!(f, "~ {name} {from} → {to}"),
        }
    }
}

/// Whether `path` is a manifest this module can parse.
pub fn is_supported_manifest(path: &str) -> bool {
    matches!(
        path.rsplit('/').next(),
        Some("Gemfile" | "package.json" | "Cargo.toml")
    )
}

/// Parses a manifest. Returns `Ok(None)` for unsupported files.
pub fn parse_manifest(path: &str, content: &str) -> Result<Option<Deps>, ManifestError> {
    let deps = match path.rsplit('/').next() {
        Some("Gemfile") => parse_gemfile(content),
        Some("package.json") => parse_package_json(content)?,
        Some("Cargo.toml") => parse_cargo_toml(content)?,
        _ => return Ok(None),
    };
    Ok(Some(deps))
}

/// Compares two dependency sets, ordered by name.
pub fn compare(old: &Deps, new: &Deps) -> Vec<DependencyChange> {
    let mut changes = Vec::new();

    for (name, version) in old {
        match new.get(name) {
            None => changes.push(DependencyChange::Removed {
                name: name.clone(),
                version: version.clone(),
            }),
            Some(to) if to != version => changes.push(DependencyChange::Updated {
                name: name.clone(),
                from: version.clone(),
                to: to.clone(),
            }),
            Some(_) => {}
        }
    }
    for (name, version) in new {
        if !old.contains_key(name) {
            changes.push(DependencyChange::Added {
                name: name.clone(),
                version: version.clone(),
            });
        }
    }

    changes.sort_by(|a, b| a.name().cmp(b.name()));
    changes
}

impl DependencyChange {
    pub fn name(&self) -> &str {
        match self {
            Self::Added { name, .. } | Self::Removed { name, .. } | Self::Updated { name, .. } => {
                name
            }
        }
    }
}

/// Extracts `gem "name", "~> 1.0", ">= 1.0.1"` lines. Anything that is not
/// a version constraint (options such as `require: false`) is ignored.
fn parse_gemfile(content: &str) -> Deps {
    content
        .lines()
        .filter_map(|line| {
            let rest = line.trim_start().strip_prefix("gem")?;
            if !rest.starts_with([' ', '(']) {
                return None; // e.g. `gemspec`
            }
            let mut literals = quoted_strings(rest).into_iter();
            let name = literals.next()?;
            let constraints: Vec<String> = literals.take_while(|s| is_version(s)).collect();
            let version = if constraints.is_empty() {
                "*".to_string()
            } else {
                constraints.join(", ")
            };
            Some((name, version))
        })
        .collect()
}

/// The contents of the `'...'` / `"..."` literals in a line, in order.
/// Stops at a `#` comment outside a literal.
fn quoted_strings(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '#' => break,
            '"' | '\'' => {
                let literal: String = chars.by_ref().take_while(|&ch| ch != c).collect();
                out.push(literal);
            }
            _ => {}
        }
    }
    out
}

fn is_version(s: &str) -> bool {
    let s = s.trim_start_matches(['~', '>', '<', '=', '!', ' ']);
    s.starts_with(|c: char| c.is_ascii_digit())
}

fn parse_package_json(content: &str) -> Result<Deps, ManifestError> {
    const SECTIONS: &[&str] = &[
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ];

    let json: serde_json::Value = serde_json::from_str(content)?;
    let mut deps = Deps::new();
    for section in SECTIONS {
        let Some(entries) = json.get(section).and_then(|v| v.as_object()) else {
            continue;
        };
        for (name, version) in entries {
            let version = version.as_str().unwrap_or("*").to_string();
            deps.entry(name.clone()).or_insert(version);
        }
    }
    Ok(deps)
}

fn parse_cargo_toml(content: &str) -> Result<Deps, ManifestError> {
    let root: toml::Table = toml::from_str(content)?;
    let mut tables = dependency_tables(&root);

    // [target.'cfg(unix)'.dependencies]
    if let Some(targets) = root.get("target").and_then(|v| v.as_table()) {
        for target in targets.values().filter_map(|v| v.as_table()) {
            tables.extend(dependency_tables(target));
        }
    }
    // [workspace.dependencies]
    if let Some(t) = root
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(|v| v.as_table())
    {
        tables.push(t);
    }

    let mut deps = Deps::new();
    for table in tables {
        for (name, spec) in table {
            deps.entry(name.clone()).or_insert_with(|| cargo_spec(spec));
        }
    }
    Ok(deps)
}

/// The `[dependencies]`, `[dev-dependencies]` and `[build-dependencies]`
/// tables directly under `table`.
fn dependency_tables(table: &toml::Table) -> Vec<&toml::Table> {
    ["dependencies", "dev-dependencies", "build-dependencies"]
        .iter()
        .filter_map(|section| table.get(*section)?.as_table())
        .collect()
}

/// `"1.0"` or `{ version = "1.0", features = [...] }` → `1.0`;
/// `{ path = "../x" }` → `path: ../x`; `{ workspace = true }` → `workspace`.
fn cargo_spec(spec: &toml::Value) -> String {
    if let Some(version) = spec.as_str() {
        return version.to_string();
    }
    let field = |key: &str| spec.get(key).and_then(|v| v.as_str());
    if let Some(version) = field("version") {
        version.to_string()
    } else if let Some(git) = field("git") {
        format!("git: {git}")
    } else if let Some(path) = field("path") {
        format!("path: {path}")
    } else if spec.get("workspace").is_some() {
        "workspace".to_string()
    } else {
        "*".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deps(pairs: &[(&str, &str)]) -> Deps {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn parses_gemfile() {
        let gemfile = r#"
source "https://rubygems.org"
gemspec
gem "rails", "~> 7.1", ">= 7.1.2"
gem 'pg'
  gem "sidekiq", "7.0.0", require: false # jobs
gem("puma", "6.4")
# gem "commented_out"
"#;
        assert_eq!(
            parse_gemfile(gemfile),
            deps(&[
                ("pg", "*"),
                ("puma", "6.4"),
                ("rails", "~> 7.1, >= 7.1.2"),
                ("sidekiq", "7.0.0"),
            ])
        );
    }

    #[test]
    fn parses_package_json() {
        let json = r#"{
  "name": "app",
  "version": "1.0.0",
  "scripts": { "build": "tsc" },
  "dependencies": { "react": "^18.2.0" },
  "devDependencies": { "typescript": "~5.4.0" }
}"#;
        assert_eq!(
            parse_package_json(json).unwrap(),
            deps(&[("react", "^18.2.0"), ("typescript", "~5.4.0")])
        );
    }

    #[test]
    fn parses_cargo_toml() {
        let toml = r#"
[package]
name = "demo"
edition = "2024"

[dependencies]
serde = { version = "1", features = ["derive"] }
clap = "4.5"
local = { path = "../local" }
shared = { workspace = true }

[dev-dependencies]
tempfile = "3"

[target.'cfg(unix)'.dependencies]
libc = "0.2"
"#;
        assert_eq!(
            parse_cargo_toml(toml).unwrap(),
            deps(&[
                ("clap", "4.5"),
                ("libc", "0.2"),
                ("local", "path: ../local"),
                ("serde", "1"),
                ("shared", "workspace"),
                ("tempfile", "3"),
            ])
        );
    }

    #[test]
    fn invalid_manifest_is_an_error() {
        assert!(parse_manifest("package.json", "{ nope").is_err());
        assert!(parse_manifest("Cargo.toml", "[[[").is_err());
        assert!(parse_manifest("README.md", "").unwrap().is_none());
    }

    #[test]
    fn compares_dependency_sets() {
        let old = deps(&[("clap", "4.5"), ("rand", "0.8"), ("serde", "1")]);
        let new = deps(&[("anyhow", "1"), ("clap", "4.6"), ("serde", "1")]);
        let changes = compare(&old, &new);

        let rendered: Vec<String> = changes.iter().map(ToString::to_string).collect();
        assert_eq!(
            rendered,
            vec!["+ anyhow 1", "~ clap 4.5 → 4.6", "- rand 0.8"]
        );
    }
}
