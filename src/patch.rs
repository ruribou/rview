//! Parsing of `git diff -U0` output (unified diff without context lines).

use serde::Serialize;
use thiserror::Error;

/// One `@@ -a,b +c,d @@` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub old_start: u32,
    pub new_start: u32,
    pub removed: Vec<String>,
    pub added: Vec<String>,
}

/// The content changes of a single file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FileDiff {
    /// Path after the change, or the old path for deleted files.
    pub path: String,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

/// Line counts for a file, as shown next to each path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Stats {
    pub additions: usize,
    pub deletions: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub binary: bool,
}

impl FileDiff {
    pub fn stats(&self) -> Stats {
        Stats {
            additions: self.hunks.iter().map(|h| h.added.len()).sum(),
            deletions: self.hunks.iter().map(|h| h.removed.len()).sum(),
            binary: self.binary,
        }
    }

    pub fn added_lines(&self) -> impl Iterator<Item = &str> {
        self.hunks
            .iter()
            .flat_map(|h| h.added.iter().map(String::as_str))
    }

    pub fn removed_lines(&self) -> impl Iterator<Item = &str> {
        self.hunks
            .iter()
            .flat_map(|h| h.removed.iter().map(String::as_str))
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PatchError {
    #[error("malformed hunk header `{0}`")]
    BadHunkHeader(String),
    #[error("hunk ended early in `{0}`")]
    TruncatedHunk(String),
}

/// Parses the output of
/// `git diff -U0 --src-prefix=a/ --dst-prefix=b/ ...`.
///
/// Hunk bodies are consumed by the line counts in their header rather than
/// by looking at line prefixes, so a removed line that itself starts with
/// `--` can never be mistaken for a file header.
pub fn parse_patch(input: &str) -> Result<Vec<FileDiff>, PatchError> {
    let mut files = Vec::new();
    let mut current: Option<FileDiff> = None;
    let mut lines = input.lines();

    while let Some(line) = lines.next() {
        if let Some(rest) = line.strip_prefix("diff --git ") {
            files.extend(current.take());
            current = Some(FileDiff {
                path: path_from_git_header(rest).unwrap_or_default(),
                ..FileDiff::default()
            });
            continue;
        }
        let Some(file) = current.as_mut() else {
            continue;
        };

        if let Some(rest) = line.strip_prefix("+++ ") {
            if let Some(path) = strip_side_prefix(rest, "b/") {
                file.path = path;
            }
        } else if let Some(rest) = line.strip_prefix("--- ") {
            // For deletions `+++` is /dev/null, so this is the only path.
            if let Some(path) = strip_side_prefix(rest, "a/") {
                file.path = path;
            }
        } else if let Some(rest) = line.strip_prefix("rename to ") {
            file.path = unquote(rest);
        } else if line.starts_with("Binary files ") {
            file.binary = true;
        } else if line.starts_with("@@ ") {
            let (old_start, old_len, new_start, new_len) = parse_hunk_header(line)?;
            let mut hunk = Hunk {
                old_start,
                new_start,
                removed: Vec::with_capacity(old_len as usize),
                added: Vec::with_capacity(new_len as usize),
            };
            let (mut old_left, mut new_left) = (old_len, new_len);
            while old_left > 0 || new_left > 0 {
                let body = lines
                    .next()
                    .ok_or_else(|| PatchError::TruncatedHunk(file.path.clone()))?;
                if let Some(text) = body.strip_prefix('-') {
                    hunk.removed.push(text.to_string());
                    old_left = old_left.saturating_sub(1);
                } else if let Some(text) = body.strip_prefix('+') {
                    hunk.added.push(text.to_string());
                    new_left = new_left.saturating_sub(1);
                } else if body.starts_with('\\') {
                    // "\ No newline at end of file"
                } else {
                    // A context line; not produced with -U0 but harmless.
                    old_left = old_left.saturating_sub(1);
                    new_left = new_left.saturating_sub(1);
                }
            }
            file.hunks.push(hunk);
        }
    }
    files.extend(current);

    Ok(files)
}

/// `@@ -12,3 +12,0 @@ fn foo` → (12, 3, 12, 0). A missing count means 1.
fn parse_hunk_header(line: &str) -> Result<(u32, u32, u32, u32), PatchError> {
    let bad = || PatchError::BadHunkHeader(line.to_string());
    let mut parts = line.split(' ').skip(1);
    let old = parts
        .next()
        .and_then(|p| p.strip_prefix('-'))
        .ok_or_else(bad)?;
    let new = parts
        .next()
        .and_then(|p| p.strip_prefix('+'))
        .ok_or_else(bad)?;

    let range = |s: &str| -> Option<(u32, u32)> {
        match s.split_once(',') {
            Some((start, len)) => Some((start.parse().ok()?, len.parse().ok()?)),
            None => Some((s.parse().ok()?, 1)),
        }
    };
    let (old_start, old_len) = range(old).ok_or_else(bad)?;
    let (new_start, new_len) = range(new).ok_or_else(bad)?;
    Ok((old_start, old_len, new_start, new_len))
}

/// Best-effort path from `diff --git a/P b/P` for sections without
/// `---`/`+++` lines (binary files, mode-only changes). Later header lines
/// override it. Only unambiguous when both sides are equal, which holds
/// unless the file was renamed, and renames carry `rename to` anyway.
fn path_from_git_header(rest: &str) -> Option<String> {
    if rest.starts_with('"') {
        // `"a/x y" "b/x y"`: split at the closing quote of the first path.
        let (old, new) = rest.split_once("\" \"")?;
        let old = unquote(&format!("{old}\""));
        let new = unquote(&format!("\"{new}"));
        let path = new.strip_prefix("b/")?;
        return (old.strip_prefix("a/") == Some(path)).then(|| path.to_string());
    }
    // `a/P b/P` has length 2 * len(P) + 5.
    let len = rest.len().checked_sub(5)? / 2;
    let old = rest.get(2..2 + len)?;
    let new = rest.get(2 + len + 3..)?;
    (rest.starts_with("a/") && rest[2 + len..].starts_with(" b/") && old == new)
        .then(|| new.to_string())
}

/// Returns the path from a `---`/`+++` header, or `None` for `/dev/null`.
fn strip_side_prefix(raw: &str, prefix: &str) -> Option<String> {
    let path = unquote(raw);
    path.strip_prefix(prefix).map(str::to_string)
}

/// Undoes git's C-style quoting of unusual paths (`"a/tab\there"`,
/// `"a/\346\227\245.txt"`). Unquoted input is returned as is.
fn unquote(raw: &str) -> String {
    let Some(inner) = raw.strip_prefix('"').and_then(|s| s.strip_suffix('"')) else {
        return raw.to_string();
    };

    let mut bytes = Vec::with_capacity(inner.len());
    let mut chars = inner.bytes().peekable();
    while let Some(b) = chars.next() {
        if b != b'\\' {
            bytes.push(b);
            continue;
        }
        match chars.next() {
            Some(b't') => bytes.push(b'\t'),
            Some(b'n') => bytes.push(b'\n'),
            Some(b'r') => bytes.push(b'\r'),
            Some(b'a') => bytes.push(0x07),
            Some(b'b') => bytes.push(0x08),
            Some(b'f') => bytes.push(0x0c),
            Some(b'v') => bytes.push(0x0b),
            Some(d @ b'0'..=b'7') => {
                // Up to three octal digits encode one raw byte.
                let mut value = u32::from(d - b'0');
                for _ in 0..2 {
                    match chars.peek() {
                        Some(&n @ b'0'..=b'7') => {
                            value = value * 8 + u32::from(n - b'0');
                            chars.next();
                        }
                        _ => break,
                    }
                }
                bytes.push(value as u8);
            }
            Some(other) => bytes.push(other), // \" and \\
            None => bytes.push(b'\\'),
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "\
diff --git a/app/models/user.rb b/app/models/user.rb
index 1111111..2222222 100644
--- a/app/models/user.rb
+++ b/app/models/user.rb
@@ -3 +3,2 @@ class User
-  validates :name
+  validates :name, presence: true
+  validates :email
@@ -10,0 +12 @@ class User
+  has_many :posts
diff --git a/db/migrate/1_add.rb b/db/migrate/1_add.rb
new file mode 100644
index 0000000..3333333
--- /dev/null
+++ b/db/migrate/1_add.rb
@@ -0,0 +1,2 @@
+class Add < ActiveRecord::Migration[7.1]
+end
\\ No newline at end of file
diff --git a/old.txt b/old.txt
deleted file mode 100644
index 4444444..0000000
--- a/old.txt
+++ /dev/null
@@ -1 +0,0 @@
---not a header
diff --git a/logo.png b/logo.png
index 5555555..6666666 100644
Binary files a/logo.png and b/logo.png differ
";

    #[test]
    fn parses_files_and_hunks() {
        let files = parse_patch(PATCH).unwrap();
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "app/models/user.rb",
                "db/migrate/1_add.rb",
                "old.txt",
                "logo.png"
            ]
        );

        let user = &files[0];
        assert_eq!(user.hunks.len(), 2);
        assert_eq!(user.hunks[0].removed, vec!["  validates :name"]);
        assert_eq!(
            user.hunks[0].added,
            vec!["  validates :name, presence: true", "  validates :email"]
        );
        assert_eq!(user.hunks[1].new_start, 12);
    }

    #[test]
    fn computes_stats() {
        let files = parse_patch(PATCH).unwrap();
        let stats: Vec<(usize, usize, bool)> = files
            .iter()
            .map(|f| {
                let s = f.stats();
                (s.additions, s.deletions, s.binary)
            })
            .collect();
        assert_eq!(
            stats,
            vec![(3, 1, false), (2, 0, false), (0, 1, false), (0, 0, true)]
        );
    }

    #[test]
    fn removed_line_that_looks_like_header_stays_content() {
        let files = parse_patch(PATCH).unwrap();
        assert_eq!(files[2].hunks[0].removed, vec!["--not a header"]);
    }

    #[test]
    fn follows_renames() {
        let patch = "\
diff --git a/lib/old.rb b/lib/new.rb
similarity index 100%
rename from lib/old.rb
rename to lib/new.rb
";
        assert_eq!(parse_patch(patch).unwrap()[0].path, "lib/new.rb");
    }

    #[test]
    fn takes_path_from_git_header() {
        assert_eq!(
            path_from_git_header("a/img/a b.png b/img/a b.png").as_deref(),
            Some("img/a b.png")
        );
        assert_eq!(
            path_from_git_header(r#""a/tab\there" "b/tab\there""#).as_deref(),
            Some("tab\there")
        );
        assert_eq!(path_from_git_header("a/old b/new"), None);
    }

    #[test]
    fn unquotes_paths() {
        assert_eq!(unquote("plain/path"), "plain/path");
        assert_eq!(unquote(r#""b/tab\there""#), "b/tab\there");
        assert_eq!(unquote(r#""b/q\"uote\\""#), "b/q\"uote\\");
        assert_eq!(unquote(r#""b/\346\227\245.txt""#), "b/日.txt");
    }

    #[test]
    fn rejects_truncated_hunk() {
        let patch = "diff --git a/x b/x\n+++ b/x\n@@ -1,2 +1 @@\n-a\n";
        assert_eq!(
            parse_patch(patch),
            Err(PatchError::TruncatedHunk("x".to_string()))
        );
    }

    #[test]
    fn rejects_bad_hunk_header() {
        let patch = "diff --git a/x b/x\n+++ b/x\n@@ nonsense @@\n";
        assert!(matches!(
            parse_patch(patch),
            Err(PatchError::BadHunkHeader(_))
        ));
    }
}
