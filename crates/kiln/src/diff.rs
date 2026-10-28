//! One file's changes for the diff viewer: `git diff` against HEAD (staged and unstaged
//! together; an untracked file as new), parsed into hunks and lines, and laid out as rows for
//! the unified and the split views. Copies take the lines' text only, never the gutter.

use std::process::{Command, Stdio};

use crate::git::FileChange;

/// Rows shown before "Show all" (a very large diff would take long to lay out).
pub const ROW_CAP: usize = 5000;
/// Tabs are shown to the next multiple of this.
const TAB: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: LineKind,
    /// Line numbers in the old and the new file.
    pub old: Option<u32>,
    pub new: Option<u32>,
    /// The text as in the file (no "+", "-" or " " in front).
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// "@@ -12,7 +12,9 @@ fn main() {"
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    /// The status letter, as the rail shows it.
    pub kind: char,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
    pub added: usize,
    pub removed: usize,
    /// The patch as git wrote it (Copy diff; `git apply` takes it).
    pub patch: String,
}

impl FileDiff {
    /// The first changed line in the new file (Open in editor goes there).
    pub fn first_changed_line(&self) -> u32 {
        self.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .find(|l| l.kind != LineKind::Context)
            .and_then(|l| l.new.or(l.old))
            .unwrap_or(1)
    }
}

/// Parses git's unified diff of one file.
pub fn parse(path: &str, kind: char, patch: &str) -> FileDiff {
    let mut diff = FileDiff {
        path: path.to_string(),
        kind,
        patch: patch.to_string(),
        ..Default::default()
    };
    let (mut old, mut new) = (0u32, 0u32);
    for line in patch.lines() {
        if line.starts_with("Binary files ") || line == "GIT binary patch" {
            diff.binary = true;
            continue;
        }
        if let Some(rest) = line.strip_prefix("@@ ") {
            // "-a,b +c,d @@ context"
            let mut parts = rest.split(' ');
            let start = |s: Option<&str>, sign: char| {
                s.and_then(|s| s.strip_prefix(sign))
                    .and_then(|s| s.split(',').next())
                    .and_then(|n| n.parse::<u32>().ok())
                    .unwrap_or(1)
            };
            old = start(parts.next(), '-');
            new = start(parts.next(), '+');
            diff.hunks.push(Hunk {
                header: line.to_string(),
                lines: Vec::new(),
            });
            continue;
        }
        let Some(hunk) = diff.hunks.last_mut() else {
            // The file's header lines (diff --git, index, ---, +++).
            continue;
        };
        let (kind, text) = match line.chars().next() {
            Some('+') => (LineKind::Added, &line[1..]),
            Some('-') => (LineKind::Removed, &line[1..]),
            Some(' ') => (LineKind::Context, &line[1..]),
            // "\ No newline at end of file", or an empty context line some tools write.
            Some('\\') => continue,
            None => (LineKind::Context, ""),
            Some(_) => continue,
        };
        let (o, n) = match kind {
            LineKind::Context => (Some(old), Some(new)),
            LineKind::Added => (None, Some(new)),
            LineKind::Removed => (Some(old), None),
        };
        if o.is_some() {
            old += 1;
        }
        if n.is_some() {
            new += 1;
        }
        match kind {
            LineKind::Added => diff.added += 1,
            LineKind::Removed => diff.removed += 1,
            LineKind::Context => {}
        }
        hunk.lines.push(DiffLine {
            kind,
            old: o,
            new: n,
            text: text.to_string(),
        });
    }
    diff
}

/// `git diff` for one file in the repository at `root`: against HEAD, staged and unstaged
/// together; an untracked file (or any file in a repository without a commit yet) as new.
/// Blocking; call it off the UI thread.
pub fn load(root: &str, file: &FileChange) -> FileDiff {
    let tracked = file.kind != '?';
    let patch = tracked
        .then(|| git_diff(root, &["HEAD", "--", &file.path]))
        .flatten()
        .or_else(|| git_diff(root, &["--no-index", "--", "/dev/null", &file.path]))
        .unwrap_or_default();
    parse(&file.path, file.kind, &patch)
}

fn git_diff(root: &str, what: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(["-c", "core.fsmonitor=false", "-C", root])
        .args(["--literal-pathspecs", "--no-pager", "diff"])
        .args(["--no-color", "--no-ext-diff", "--no-textconv"])
        .args(what)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    // 1 is "there were differences" for --no-index.
    matches!(output.status.code(), Some(0 | 1))
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// A row of the unified view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Row {
    /// A hunk's header, by its index.
    Hunk(usize),
    Line(DiffLine),
}

pub fn unified_rows(diff: &FileDiff) -> Vec<Row> {
    let mut rows = Vec::new();
    for (i, hunk) in diff.hunks.iter().enumerate() {
        rows.push(Row::Hunk(i));
        rows.extend(hunk.lines.iter().cloned().map(Row::Line));
    }
    rows
}

/// A row of the split view: old on the left, new on the right. Removed lines followed by
/// added ones pair up; context lines show on both sides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SplitRow {
    Hunk(usize),
    Pair(Option<DiffLine>, Option<DiffLine>),
}

pub fn split_rows(diff: &FileDiff) -> Vec<SplitRow> {
    let mut rows = Vec::new();
    for (i, hunk) in diff.hunks.iter().enumerate() {
        rows.push(SplitRow::Hunk(i));
        let mut lines = hunk.lines.iter().peekable();
        while let Some(line) = lines.next() {
            match line.kind {
                LineKind::Context => {
                    rows.push(SplitRow::Pair(Some(line.clone()), Some(line.clone())));
                }
                LineKind::Added => rows.push(SplitRow::Pair(None, Some(line.clone()))),
                LineKind::Removed => {
                    // This run of removed lines, then the added run after it.
                    let mut removed = vec![line.clone()];
                    while let Some(next) = lines.next_if(|l| l.kind == LineKind::Removed) {
                        removed.push(next.clone());
                    }
                    let mut added = Vec::new();
                    while let Some(next) = lines.next_if(|l| l.kind == LineKind::Added) {
                        added.push(next.clone());
                    }
                    let n = removed.len().max(added.len());
                    let mut removed = removed.into_iter();
                    let mut added = added.into_iter();
                    for _ in 0..n {
                        rows.push(SplitRow::Pair(removed.next(), added.next()));
                    }
                }
            }
        }
    }
    rows
}

/// How many of `total` rows to lay out: all, or the first `ROW_CAP` until "Show all".
pub fn shown_rows(total: usize, show_all: bool) -> usize {
    if show_all { total } else { total.min(ROW_CAP) }
}

/// A line as shown: tabs to the next multiple of four.
pub fn display(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut column = 0;
    for c in text.chars() {
        if c == '\t' {
            let to = (column / TAB + 1) * TAB;
            out.extend(std::iter::repeat_n(' ', to - column));
            column = to;
        } else {
            out.push(c);
            column += 1;
        }
    }
    out
}

/// The part of `text` between shown columns `from` and `to` (a tab counts as the columns it
/// takes), as in the file.
fn original_between(text: &str, from: usize, to: usize) -> String {
    let mut out = String::new();
    let mut column = 0;
    for c in text.chars() {
        let width = if c == '\t' {
            (column / TAB + 1) * TAB - column
        } else {
            1
        };
        // A character is in if it starts inside the range.
        if column >= from && column < to {
            out.push(c);
        }
        column += width;
    }
    out
}

/// A place in the unified rows: the row and the shown column.
pub type Place = (usize, usize);

/// The selected text between two places (in either order), as plain text: each line's text
/// only, no line numbers, no "+" or "-", hunk headers left out, so it pastes as code.
pub fn copy_text(rows: &[Row], a: Place, b: Place) -> String {
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    let mut lines = Vec::new();
    for (i, row) in rows.iter().enumerate().take(end.0 + 1).skip(start.0) {
        let Row::Line(line) = row else {
            continue;
        };
        let from = if i == start.0 { start.1 } else { 0 };
        let to = if i == end.0 { end.1 } else { usize::MAX };
        lines.push(original_between(&line.text, from, to));
    }
    lines.join("\n")
}

/// The whole new side (⌘A): every context and added line.
pub fn new_side(rows: &[Row]) -> String {
    rows.iter()
        .filter_map(|row| match row {
            Row::Line(line) if line.kind != LineKind::Removed => Some(line.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The word around a shown column of `text` (double-click): (from, to) in shown columns.
pub fn word_at(text: &str, column: usize) -> (usize, usize) {
    let shown: Vec<char> = display(text).chars().collect();
    let word = |c: &char| c.is_alphanumeric() || *c == '_';
    if column >= shown.len() || !word(&shown[column]) {
        return (column, (column + 1).min(shown.len()));
    }
    let mut from = column;
    while from > 0 && word(&shown[from - 1]) {
        from -= 1;
    }
    let mut to = column;
    while to < shown.len() && word(&shown[to]) {
        to += 1;
    }
    (from, to)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/routes.js b/src/routes.js
index 1111111..2222222 100644
--- a/src/routes.js
+++ b/src/routes.js
@@ -1,4 +1,5 @@
 const a = 1;
-const b = 2;
+const b = 3;
+\tconst c = 4;
 module.exports = a;
\\ No newline at end of file
@@ -10,2 +11,2 @@ fn tail() {
-old
+new
";

    #[test]
    fn hunks_lines_and_numbers() {
        let diff = parse("src/routes.js", 'M', PATCH);
        assert!(!diff.binary);
        assert_eq!(diff.hunks.len(), 2);
        assert_eq!((diff.added, diff.removed), (3, 2));
        let first = &diff.hunks[0];
        assert_eq!(first.header, "@@ -1,4 +1,5 @@");
        let numbers: Vec<_> = first.lines.iter().map(|l| (l.kind, l.old, l.new)).collect();
        assert_eq!(
            numbers,
            vec![
                (LineKind::Context, Some(1), Some(1)),
                (LineKind::Removed, Some(2), None),
                (LineKind::Added, None, Some(2)),
                (LineKind::Added, None, Some(3)),
                (LineKind::Context, Some(3), Some(4)),
            ]
        );
        assert_eq!(diff.hunks[1].lines[0].old, Some(10));
        assert_eq!(diff.hunks[1].lines[1].new, Some(11));
        assert_eq!(diff.first_changed_line(), 2);
        assert_eq!(diff.patch, PATCH);
    }

    #[test]
    fn new_deleted_and_binary_files() {
        let new = parse(
            "a.txt",
            '?',
            "diff --git a/a.txt b/a.txt\nnew file mode 100644\n--- /dev/null\n+++ b/a.txt\n@@ -0,0 +1,2 @@\n+one\n+two\n",
        );
        assert_eq!((new.added, new.removed), (2, 0));
        assert_eq!(new.hunks[0].lines[1].new, Some(2));
        let gone = parse(
            "b.txt",
            'D',
            "--- a/b.txt\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-one\n-two\n",
        );
        assert_eq!((gone.added, gone.removed), (0, 2));
        let binary = parse(
            "logo.png",
            'M',
            "diff --git a/logo.png b/logo.png\nBinary files a/logo.png and b/logo.png differ\n",
        );
        assert!(binary.binary && binary.hunks.is_empty());
    }

    #[test]
    fn split_rows_pair_removed_with_added() {
        let diff = parse("src/routes.js", 'M', PATCH);
        let rows = split_rows(&diff);
        let shape: Vec<_> = rows
            .iter()
            .map(|r| match r {
                SplitRow::Hunk(_) => "hunk".to_string(),
                SplitRow::Pair(l, r) => format!(
                    "{}|{}",
                    l.as_ref().map_or("", |l| l.text.as_str()),
                    r.as_ref().map_or("", |r| r.text.as_str())
                ),
            })
            .collect();
        assert_eq!(
            shape,
            [
                "hunk",
                "const a = 1;|const a = 1;",
                "const b = 2;|const b = 3;",
                "|\tconst c = 4;",
                "module.exports = a;|module.exports = a;",
                "hunk",
                "old|new",
            ]
        );
    }

    #[test]
    fn copies_have_no_gutter_and_keep_tabs() {
        let diff = parse("src/routes.js", 'M', PATCH);
        let rows = unified_rows(&diff);
        // From the start of "const b = 3;" to the end of the tabbed line: whole lines, no
        // "+", no numbers, the tab kept.
        assert_eq!(
            copy_text(&rows, (3, 0), (4, usize::MAX)),
            "const b = 3;\n\tconst c = 4;"
        );
        // Part of a line: shown columns 6..11 of "const a = 1;" is "a = 1".
        assert_eq!(copy_text(&rows, (1, 6), (1, 11)), "a = 1");
        // Backwards, and across a hunk header (left out).
        assert_eq!(copy_text(&rows, (7, 3), (5, 0)), "module.exports = a;\nold");
        // A tab is in when the selection starts inside its columns.
        assert_eq!(display("\tconst c"), "    const c");
        assert_eq!(copy_text(&rows, (4, 4), (4, 9)), "const");
        // ⌘A: the new side only.
        assert_eq!(
            new_side(&rows),
            "const a = 1;\nconst b = 3;\n\tconst c = 4;\nmodule.exports = a;\nnew"
        );
    }

    #[test]
    fn large_diffs_are_capped_until_show_all() {
        assert_eq!(shown_rows(120, false), 120);
        assert_eq!(shown_rows(ROW_CAP + 1, false), ROW_CAP);
        assert_eq!(shown_rows(ROW_CAP * 3, true), ROW_CAP * 3);
        let mut patch = String::from("@@ -1,0 +1,9000 @@\n");
        for i in 0..9000 {
            patch.push_str(&format!("+line {i}\n"));
        }
        let rows = unified_rows(&parse("big.txt", 'A', &patch));
        assert_eq!(rows.len(), 9001);
        assert_eq!(shown_rows(rows.len(), false), ROW_CAP);
    }

    #[test]
    fn words_for_double_click() {
        assert_eq!(word_at("let foo_bar = 1;", 6), (4, 11));
        assert_eq!(word_at("a + b", 2), (2, 3));
        assert_eq!(word_at("x", 5), (5, 1));
    }

    #[test]
    fn loads_a_real_diff() {
        let dir = std::env::temp_dir().join(format!("kiln-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            assert!(
                Command::new("git")
                    .args(["-c", "user.name=T", "-c", "user.email=t@t", "-C"])
                    .arg(&dir)
                    .args(args)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .unwrap()
                    .success(),
                "git {args:?}"
            );
        };
        git(&["init", "-q"]);
        std::fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        git(&["add", "a.txt"]);
        git(&["commit", "-q", "-m", "a"]);
        // Staged and unstaged together, against HEAD.
        std::fs::write(dir.join("a.txt"), "one\nTWO\n").unwrap();
        git(&["add", "a.txt"]);
        std::fs::write(dir.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(dir.join("new.txt"), "fresh\n").unwrap();
        let root = dir.to_string_lossy().to_string();
        let modified = load(
            &root,
            &FileChange {
                kind: 'M',
                path: "a.txt".into(),
            },
        );
        assert_eq!((modified.added, modified.removed), (2, 1));
        let untracked = load(
            &root,
            &FileChange {
                kind: '?',
                path: "new.txt".into(),
            },
        );
        assert_eq!((untracked.added, untracked.removed), (1, 0));
        assert!(
            untracked.patch.contains("+++ b/new.txt"),
            "{}",
            untracked.patch
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
