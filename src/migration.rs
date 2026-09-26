//! Detection of destructive statements in migration files.

/// Rails migration methods that drop or rewrite existing data.
const RAILS_DESTRUCTIVE: &[&str] = &[
    "remove_column",
    "remove_columns",
    "remove_reference",
    "remove_belongs_to",
    "remove_timestamps",
    "remove_index",
    "remove_foreign_key",
    "drop_table",
    "drop_join_table",
    "rename_column",
    "rename_table",
    "change_column",
    "change_column_null",
];

/// The same operations inside a `change_table :users do |t|` block.
const RAILS_TABLE_DESTRUCTIVE: &[&str] = &[
    "remove",
    "remove_references",
    "remove_belongs_to",
    "remove_timestamps",
    "remove_index",
    "rename",
    "change",
];

const SQL_DESTRUCTIVE: &[&str] = &[
    "DROP TABLE",
    "DROP COLUMN",
    "DROP INDEX",
    "TRUNCATE",
    "RENAME COLUMN",
    "RENAME TO",
    "ALTER COLUMN",
    "DELETE FROM",
];

/// Django migration operations.
const DJANGO_DESTRUCTIVE: &[&str] = &[
    "RemoveField(",
    "DeleteModel(",
    "RenameField(",
    "RenameModel(",
    "AlterField(",
];

/// Returns the trimmed lines among `added_lines` that perform a
/// destructive schema change.
pub fn destructive_statements<'a>(added_lines: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    added_lines
        .into_iter()
        .map(str::trim)
        .filter(|line| !is_comment(line) && is_destructive(line))
        .map(str::to_string)
        .collect()
}

fn is_comment(line: &str) -> bool {
    line.starts_with('#') || line.starts_with("--") || line.starts_with("//")
}

fn is_destructive(line: &str) -> bool {
    is_rails_destructive(line)
        || {
            let upper = line.to_ascii_uppercase();
            SQL_DESTRUCTIVE.iter().any(|kw| upper.contains(kw))
        }
        || DJANGO_DESTRUCTIVE.iter().any(|op| line.contains(op))
}

/// Matches the leading method call exactly, so `change_column_default`
/// does not count as `change_column`.
fn is_rails_destructive(line: &str) -> bool {
    let (receiver, method) = leading_call(line);
    match receiver {
        None => RAILS_DESTRUCTIVE.contains(&method),
        Some(_) => RAILS_TABLE_DESTRUCTIVE.contains(&method),
    }
}

/// Splits `t.remove :x` into (Some("t"), "remove") and
/// `drop_table :x` into (None, "drop_table").
fn leading_call(line: &str) -> (Option<&str>, &str) {
    let ident_end = |s: &str| {
        s.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(s.len())
    };

    let first_end = ident_end(line);
    let (first, rest) = line.split_at(first_end);
    match rest.strip_prefix('.') {
        Some(after_dot) => {
            let (method, _) = after_dot.split_at(ident_end(after_dot));
            (Some(first), method)
        }
        None => (None, first),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rails_operations() {
        let lines = [
            "    remove_column :users, :legacy_flag, :string",
            "    add_column :users, :status, :integer",
            "    change_column_default :users, :status, from: nil, to: 0",
            "    change_column :users, :name, :text",
            "    # drop_table :old_things",
            "      t.remove :nickname",
            "      t.string :title",
        ];
        assert_eq!(
            destructive_statements(lines),
            vec![
                "remove_column :users, :legacy_flag, :string",
                "change_column :users, :name, :text",
                "t.remove :nickname",
            ]
        );
    }

    #[test]
    fn detects_sql_operations() {
        let lines = [
            "ALTER TABLE users DROP COLUMN legacy_flag;",
            "alter table users add column status int;",
            "drop table sessions;",
            "-- DROP TABLE commented;",
        ];
        assert_eq!(
            destructive_statements(lines),
            vec![
                "ALTER TABLE users DROP COLUMN legacy_flag;",
                "drop table sessions;"
            ]
        );
    }

    #[test]
    fn detects_django_operations() {
        let lines = [
            "        migrations.RemoveField(",
            "        migrations.AddField(",
        ];
        assert_eq!(
            destructive_statements(lines),
            vec!["migrations.RemoveField("]
        );
    }
}
