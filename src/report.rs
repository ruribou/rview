//! Aggregation of changes into a report and rendering it for output.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::Serialize;

use crate::category::{Category, classify};
use crate::concern::{self, Concern};
use crate::diff::FileChange;

/// Files belonging to one category.
#[derive(Debug, Serialize)]
pub struct Group {
    pub category: Category,
    pub files: Vec<FileChange>,
}

/// The full analysis result for a diff range.
#[derive(Debug, Serialize)]
pub struct Report {
    pub base: String,
    pub head: String,
    pub groups: Vec<Group>,
    pub concerns: Vec<Concern>,
}

impl Report {
    pub fn build(base: &str, head: &str, changes: Vec<FileChange>) -> Self {
        let concerns = concern::detect(&changes);

        // BTreeMap keeps categories in their declared order.
        let mut by_category: BTreeMap<Category, Vec<FileChange>> = BTreeMap::new();
        for change in changes {
            by_category
                .entry(classify(&change.path))
                .or_default()
                .push(change);
        }

        let groups = by_category
            .into_iter()
            .map(|(category, mut files)| {
                files.sort_by(|a, b| a.path.cmp(&b.path));
                Group { category, files }
            })
            .collect();

        Self {
            base: base.to_string(),
            head: head.to_string(),
            groups,
            concerns,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }

    pub fn render_text(&self) -> String {
        let mut out = String::new();
        let range = format!("{}...{}", self.base, self.head);

        if self.is_empty() {
            let _ = writeln!(out, "No changes: {range}");
            return out;
        }

        let _ = writeln!(out, "Changes: {range}");
        for group in &self.groups {
            let _ = writeln!(out, "\n[{}]", group.category);
            for file in &group.files {
                let _ = match &file.old_path {
                    Some(old) => writeln!(out, "  {} {old} -> {}", file.kind.marker(), file.path),
                    None => writeln!(out, "  {} {}", file.kind.marker(), file.path),
                };
            }
        }

        if !self.concerns.is_empty() {
            let _ = writeln!(out, "\n⚠ Possible concerns");
            for concern in &self.concerns {
                let _ = writeln!(out, "  - {concern}");
            }
        }

        out
    }

    pub fn render_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::ChangeKind;

    fn change(kind: ChangeKind, path: &str) -> FileChange {
        FileChange {
            kind,
            path: path.to_string(),
            old_path: None,
        }
    }

    #[test]
    fn groups_in_category_order_with_sorted_files() {
        let report = Report::build(
            "main",
            "HEAD",
            vec![
                change(ChangeKind::Modified, "spec/b_spec.rb"),
                change(ChangeKind::Modified, "app/services/z.rb"),
                change(ChangeKind::Added, "db/migrate/1.rb"),
                change(ChangeKind::Modified, "app/services/a.rb"),
            ],
        );

        let categories: Vec<Category> = report.groups.iter().map(|g| g.category).collect();
        assert_eq!(
            categories,
            vec![Category::Db, Category::Logic, Category::Test]
        );

        let logic: Vec<&str> = report.groups[1]
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        assert_eq!(logic, vec!["app/services/a.rb", "app/services/z.rb"]);
    }

    #[test]
    fn renders_text() {
        let report = Report::build(
            "main",
            "HEAD",
            vec![
                change(ChangeKind::Added, "db/migrate/1_add.rb"),
                FileChange {
                    kind: ChangeKind::Renamed,
                    path: "app/services/new.rb".into(),
                    old_path: Some("app/services/old.rb".into()),
                },
            ],
        );

        let expected = "\
Changes: main...HEAD

[DB]
  A db/migrate/1_add.rb

[Logic]
  R app/services/old.rb -> app/services/new.rb

⚠ Possible concerns
  - Migration detected
  - Code changed without test changes
";
        assert_eq!(report.render_text(), expected);
    }

    #[test]
    fn renders_empty() {
        let report = Report::build("main", "HEAD", vec![]);
        assert_eq!(report.render_text(), "No changes: main...HEAD\n");
    }

    #[test]
    fn renders_json() {
        let report = Report::build(
            "main",
            "HEAD",
            vec![change(ChangeKind::Modified, "config/routes.rb")],
        );
        let json: serde_json::Value = serde_json::from_str(&report.render_json().unwrap()).unwrap();

        assert_eq!(json["groups"][0]["category"], "api");
        assert_eq!(json["groups"][0]["files"][0]["kind"], "modified");
        assert_eq!(json["concerns"][0]["kind"], "routes_changed");
    }
}
