//! Parsing and comparison of Rails `db/schema.rb`.
//!
//! Both versions of the schema dump are parsed into tables and columns so
//! that a diff can be described as "column `users.status` removed" rather
//! than as raw line changes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::Serialize;

/// Table name → column names.
pub type Schema = BTreeMap<String, BTreeSet<String>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum SchemaChange {
    TableAdded { table: String },
    TableRemoved { table: String },
    ColumnAdded { table: String, column: String },
    ColumnRemoved { table: String, column: String },
}

impl SchemaChange {
    /// Removals can break running code and lose data.
    pub fn is_destructive(&self) -> bool {
        matches!(self, Self::TableRemoved { .. } | Self::ColumnRemoved { .. })
    }
}

impl fmt::Display for SchemaChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TableAdded { table } => write!(f, "+ table {table}"),
            Self::TableRemoved { table } => write!(f, "- table {table}"),
            Self::ColumnAdded { table, column } => write!(f, "+ column {table}.{column}"),
            Self::ColumnRemoved { table, column } => write!(f, "- column {table}.{column}"),
        }
    }
}

/// Parses `create_table "users" ... do |t|` blocks and their
/// `t.<type> "column"` lines. The implicit `id` primary key is omitted.
pub fn parse_schema_rb(content: &str) -> Schema {
    let mut schema = Schema::new();
    let mut current: Option<String> = None;

    for line in content.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("create_table ") {
            if let Some(table) = first_string_literal(rest) {
                schema.entry(table.clone()).or_default();
                current = Some(table);
            }
        } else if line == "end" {
            current = None;
        } else if let (Some(table), Some(rest)) = (&current, line.strip_prefix("t.")) {
            // Skip `t.index [...]`, `t.timestamps` (older dumps) etc.
            let kind: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if kind == "index" {
                continue;
            }
            if let Some(column) = first_string_literal(rest) {
                schema.entry(table.clone()).or_default().insert(column);
            }
        }
    }

    schema
}

fn first_string_literal(s: &str) -> Option<String> {
    let start = s.find('"')? + 1;
    let len = s[start..].find('"')?;
    Some(s[start..start + len].to_string())
}

/// Lists table and column additions and removals. Columns of added or
/// removed tables are not listed individually.
pub fn compare(old: &Schema, new: &Schema) -> Vec<SchemaChange> {
    let mut changes = Vec::new();

    for (table, old_columns) in old {
        let Some(new_columns) = new.get(table) else {
            changes.push(SchemaChange::TableRemoved {
                table: table.clone(),
            });
            continue;
        };
        for column in old_columns.difference(new_columns) {
            changes.push(SchemaChange::ColumnRemoved {
                table: table.clone(),
                column: column.clone(),
            });
        }
        for column in new_columns.difference(old_columns) {
            changes.push(SchemaChange::ColumnAdded {
                table: table.clone(),
                column: column.clone(),
            });
        }
    }
    for table in new.keys().filter(|t| !old.contains_key(*t)) {
        changes.push(SchemaChange::TableAdded {
            table: table.clone(),
        });
    }

    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = r#"
ActiveRecord::Schema[7.1].define(version: 2026_09_01_000000) do
  create_table "users", force: :cascade do |t|
    t.string "name", null: false
    t.string "legacy_flag"
    t.datetime "created_at", null: false
    t.index ["name"], name: "index_users_on_name"
  end

  create_table "sessions", force: :cascade do |t|
    t.bigint "user_id"
  end
end
"#;

    const NEW: &str = r#"
ActiveRecord::Schema[7.1].define(version: 2026_09_26_000000) do
  create_table "posts", force: :cascade do |t|
    t.text "body"
  end

  create_table "users", force: :cascade do |t|
    t.string "name", null: false
    t.integer "status", default: 0
    t.datetime "created_at", null: false
    t.index ["name"], name: "index_users_on_name"
  end
end
"#;

    #[test]
    fn parses_tables_and_columns() {
        let schema = parse_schema_rb(OLD);
        let users: Vec<&str> = schema["users"].iter().map(String::as_str).collect();
        assert_eq!(users, vec!["created_at", "legacy_flag", "name"]);
        assert!(schema["sessions"].contains("user_id"));
    }

    #[test]
    fn compares_schemas() {
        let changes = compare(&parse_schema_rb(OLD), &parse_schema_rb(NEW));
        let rendered: Vec<String> = changes.iter().map(ToString::to_string).collect();
        assert_eq!(
            rendered,
            vec![
                "- table sessions",
                "- column users.legacy_flag",
                "+ column users.status",
                "+ table posts",
            ]
        );
        assert_eq!(changes.iter().filter(|c| c.is_destructive()).count(), 2);
    }
}
