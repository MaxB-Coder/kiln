//! One file's changes for the diff viewer: `git diff` against HEAD (staged and unstaged
//! together; an untracked file as new), parsed into hunks and lines, and laid out as rows for
//! the unified and the split views. Copies take the lines' text only, never the gutter. Changed
//! lines already in the index are marked staged (`mark_staged`).

use std::collections::HashSet;
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
    /// Syntax colouring: (from, to, kind) in shown columns; empty for plain text.
    pub tokens: Vec<(usize, usize, Token)>,
    /// An added or removed line that is in the index already (`git add`ed).
    pub staged: bool,
}

/// What a stretch of code is, for its colour (the theme picks the colour).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Token {
    Comment,
    String,
    Keyword,
    Number,
    Function,
    Type,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hunk {
    /// "@@ -12,7 +12,9 @@ fn main() {"
    pub header: String,
    pub lines: Vec<DiffLine>,
}

/// How much of a hunk is staged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Staged {
    None,
    Part,
    All,
}

impl Hunk {
    /// Where the hunk's first change is in the new file: an added line's number, or for a
    /// removed one the line after the context before it.
    pub fn first_changed_line(&self) -> u32 {
        let mut last_new = new_range(&self.header).0.saturating_sub(1);
        for line in &self.lines {
            if line.kind != LineKind::Context {
                return line.new.unwrap_or(last_new + 1);
            }
            last_new = line.new.unwrap_or(last_new);
        }
        last_new.max(1)
    }

    pub fn staged(&self) -> Staged {
        let changed = self.lines.iter().filter(|l| l.kind != LineKind::Context);
        let (mut staged, mut total) = (0, 0);
        for line in changed {
            total += 1;
            staged += usize::from(line.staged);
        }
        match staged {
            0 => Staged::None,
            s if s == total => Staged::All,
            _ => Staged::Part,
        }
    }
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
    /// How many of the added and removed lines are staged.
    pub staged: usize,
    /// The patch as git wrote it (Copy diff; `git apply` takes it).
    pub patch: String,
    /// A binary file's sizes, and its pictures when it is an image.
    pub binary_change: Option<crate::image_diff::BinaryChange>,
}

impl FileDiff {
    /// The first changed line in the new file (Open in editor goes there).
    pub fn first_changed_line(&self) -> u32 {
        self.hunks.first().map_or(1, Hunk::first_changed_line)
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
            tokens: Vec::new(),
            staged: false,
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
    let mut diff = parse(&file.path, file.kind, &patch);
    if tracked {
        // HEAD to the index, and the index to the worktree.
        let staged = git_diff(root, &["--cached", "--", &file.path]);
        let unstaged = git_diff(root, &["--", &file.path]);
        if let (Some(staged), Some(unstaged)) = (staged, unstaged) {
            mark_staged(
                &mut diff,
                &parse(&file.path, file.kind, &staged),
                &parse(&file.path, file.kind, &unstaged),
            );
        }
    }
    if diff.binary {
        diff.binary_change = Some(crate::image_diff::load(root, &file.path));
    }
    highlight(&mut diff);
    diff
}

/// Marks the lines of `diff` (HEAD to the worktree) that are staged, from `staged` (HEAD to the
/// index) and `unstaged` (the index to the worktree). A removed line is staged when the index
/// has removed it too: both diffs number it in HEAD. An added line is staged when it is in the
/// index as added: its worktree number maps to an index number through `unstaged` (a line
/// `unstaged` adds is not in the index at all), and `staged` adds that index line.
pub fn mark_staged(diff: &mut FileDiff, staged: &FileDiff, unstaged: &FileDiff) {
    let lines = |d: &FileDiff, kind: LineKind| -> HashSet<u32> {
        d.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|l| l.kind == kind)
            .filter_map(|l| {
                if kind == LineKind::Removed {
                    l.old
                } else {
                    l.new
                }
            })
            .collect()
    };
    let removed_in_index = lines(staged, LineKind::Removed);
    let added_in_index = lines(staged, LineKind::Added);
    let mut count = 0;
    for line in diff.hunks.iter_mut().flat_map(|h| &mut h.lines) {
        line.staged = match line.kind {
            LineKind::Context => false,
            LineKind::Removed => line.old.is_some_and(|o| removed_in_index.contains(&o)),
            LineKind::Added => line
                .new
                .and_then(|n| old_number(unstaged, n))
                .is_some_and(|i| added_in_index.contains(&i)),
        };
        count += usize::from(line.staged);
    }
    diff.staged = count;
}

/// A new-side line number's number on the old side of `diff`; `None` when `diff` adds it.
fn old_number(diff: &FileDiff, new: u32) -> Option<u32> {
    // Old minus new, from the hunks before.
    let mut shift: i64 = 0;
    for hunk in &diff.hunks {
        let (start, count) = new_range(&hunk.header);
        // A hunk with no new lines comes after line `start`.
        let before = if count == 0 {
            new <= start
        } else {
            new < start
        };
        if before {
            break;
        }
        if let Some(line) = hunk.lines.iter().find(|l| l.new == Some(new)) {
            return line.old;
        }
        let added = hunk
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Added)
            .count() as i64;
        let removed = hunk
            .lines
            .iter()
            .filter(|l| l.kind == LineKind::Removed)
            .count() as i64;
        shift += removed - added;
    }
    u32::try_from(i64::from(new) + shift).ok()
}

/// "@@ -a,b +c,d @@": (c, d), d being 1 when left out.
fn new_range(header: &str) -> (u32, u32) {
    let part = header
        .split(' ')
        .find_map(|p| p.strip_prefix('+'))
        .unwrap_or("1");
    let mut numbers = part.split(',').map(|n| n.parse::<u32>().unwrap_or(1));
    let start = numbers.next().unwrap_or(1);
    (start, numbers.next().unwrap_or(1))
}

/// The grammars syntect ships, loaded once (on the first diff opened).
fn syntaxes() -> &'static syntect::parsing::SyntaxSet {
    static SET: std::sync::OnceLock<syntect::parsing::SyntaxSet> = std::sync::OnceLock::new();
    SET.get_or_init(syntect::parsing::SyntaxSet::load_defaults_newlines)
}

/// Colours the diff's lines by the file's language, if syntect knows it (by extension, else
/// the first line). Each hunk's old side (context and removed lines) and new side (context and
/// added) are read in order, from the hunk's start, so strings and comments that span lines
/// colour right within a hunk.
pub fn highlight(diff: &mut FileDiff) {
    use syntect::parsing::{ParseState, ScopeStack};
    let set = syntaxes();
    let extension = std::path::Path::new(&diff.path)
        .extension()
        .map(|e| e.to_string_lossy().to_string())
        .unwrap_or_default();
    let first = diff
        .hunks
        .first()
        .and_then(|h| h.lines.first())
        .map(|l| l.text.clone())
        .unwrap_or_default();
    let Some(syntax) = set
        .find_syntax_by_extension(&extension)
        .or_else(|| set.find_syntax_by_first_line(&first))
        .filter(|s| s.name != "Plain Text")
    else {
        return;
    };
    for hunk in &mut diff.hunks {
        for new_side in [true, false] {
            let mut state = ParseState::new(syntax);
            let mut stack = ScopeStack::new();
            for line in &mut hunk.lines {
                let on_side = match line.kind {
                    LineKind::Context => true,
                    LineKind::Added => new_side,
                    LineKind::Removed => !new_side,
                };
                if !on_side {
                    continue;
                }
                let text = format!("{}\n", line.text);
                let Ok(ops) = state.parse_line(&text, set) else {
                    break;
                };
                let mut tokens = Vec::new();
                let mut at = 0;
                for (to, op) in ops
                    .iter()
                    .map(|(i, op)| (*i, op))
                    .chain([(text.len(), &syntect::parsing::ScopeStackOp::Noop)])
                {
                    if to > at
                        && let Some(token) = classify(&stack)
                    {
                        tokens.push((at, to.min(line.text.len()), token));
                    }
                    at = to;
                    let _ = stack.apply(op);
                }
                // The context lines take the new side's colours.
                if new_side || line.kind == LineKind::Removed {
                    line.tokens = to_columns(&line.text, &tokens);
                }
            }
        }
    }
}

/// A scope stack's kind: the innermost scope that names one.
fn classify(stack: &syntect::parsing::ScopeStack) -> Option<Token> {
    for scope in stack.as_slice().iter().rev() {
        let name = scope.build_string();
        let token = if name.starts_with("comment") {
            Token::Comment
        } else if name.starts_with("string") {
            Token::String
        } else if name.starts_with("constant.numeric") || name.starts_with("constant.language") {
            Token::Number
        } else if name.starts_with("keyword") && !name.starts_with("keyword.operator")
            || name.starts_with("storage")
        {
            Token::Keyword
        } else if name.starts_with("entity.name.function") || name.starts_with("support.function") {
            Token::Function
        } else if name.starts_with("entity.name.type")
            || name.starts_with("support.type")
            || name.starts_with("entity.name.class")
        {
            Token::Type
        } else {
            continue;
        };
        return Some(token);
    }
    None
}

/// Byte ranges of `text` as shown columns (tabs take their width).
fn to_columns(text: &str, tokens: &[(usize, usize, Token)]) -> Vec<(usize, usize, Token)> {
    let mut columns = Vec::with_capacity(text.len() + 1);
    let mut column = 0;
    for (i, c) in text.char_indices() {
        columns.resize(i + 1, column);
        column += if c == '\t' {
            (column / TAB + 1) * TAB - column
        } else {
            1
        };
    }
    columns.resize(text.len() + 1, column);
    tokens
        .iter()
        .filter(|(from, to, _)| from < to)
        .map(|&(from, to, token)| {
            (
                columns[from.min(text.len())],
                columns[to.min(text.len())],
                token,
            )
        })
        .collect()
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

/// A side of the split view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Old,
    New,
}

/// The line on `side` of a split row, if there is one.
pub fn split_line(row: &SplitRow, side: Side) -> Option<&DiffLine> {
    match (row, side) {
        (SplitRow::Pair(old, _), Side::Old) => old.as_ref(),
        (SplitRow::Pair(_, new), Side::New) => new.as_ref(),
        (SplitRow::Hunk(_), _) => None,
    }
}

/// The text selected on one side of the split view, between two places (rows of `rows`,
/// shown columns): that side's lines only, as plain text.
pub fn copy_split(rows: &[SplitRow], side: Side, a: Place, b: Place) -> String {
    let (start, end) = if a <= b { (a, b) } else { (b, a) };
    let mut lines = Vec::new();
    for (i, row) in rows.iter().enumerate().take(end.0 + 1).skip(start.0) {
        let Some(line) = split_line(row, side) else {
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
    fn split_copies_take_one_side() {
        let diff = parse("src/routes.js", 'M', PATCH);
        let rows = split_rows(&diff);
        // Rows 1..=4 of the first hunk: the old side has three lines, the new side four.
        assert_eq!(
            copy_split(&rows, Side::Old, (1, 0), (4, usize::MAX)),
            "const a = 1;\nconst b = 2;\nmodule.exports = a;"
        );
        assert_eq!(
            copy_split(&rows, Side::New, (4, usize::MAX), (2, 6)),
            "b = 3;\n\tconst c = 4;\nmodule.exports = a;"
        );
        // Across a hunk header, which is left out.
        assert_eq!(
            copy_split(&rows, Side::New, (4, 0), (6, 3)),
            "module.exports = a;\nnew"
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
    fn code_is_coloured_by_its_language() {
        let mut diff = parse(
            "src/main.rs",
            'M',
            "@@ -1,2 +1,2 @@\n-let n = 1; // one\n+\tlet s = \"two\";\n",
        );
        highlight(&mut diff);
        let kinds = |line: &DiffLine| {
            let shown: Vec<char> = display(&line.text).chars().collect();
            line.tokens
                .iter()
                .map(|(from, to, t)| (shown[*from..*to].iter().collect::<String>(), *t))
                .collect::<Vec<_>>()
        };
        let old = kinds(&diff.hunks[0].lines[0]);
        assert!(old.contains(&("let".into(), Token::Keyword)), "{old:?}");
        assert!(old.contains(&("1".into(), Token::Number)), "{old:?}");
        assert!(
            old.iter()
                .any(|(s, t)| *t == Token::Comment && s.contains("one")),
            "{old:?}"
        );
        // Shown columns: after the tab, "let" is at 4..7.
        let new = &diff.hunks[0].lines[1];
        assert!(
            new.tokens.contains(&(4, 7, Token::Keyword)),
            "{:?}",
            new.tokens
        );
        assert!(
            kinds(new)
                .iter()
                .any(|(s, t)| *t == Token::String && s.contains("two"))
        );
        // Unknown languages stay plain.
        let mut plain = parse("notes.xyz", 'M', "@@ -1 +1 @@\n-a\n+b\n");
        highlight(&mut plain);
        assert!(plain.hunks[0].lines.iter().all(|l| l.tokens.is_empty()));
    }

    #[test]
    fn each_hunk_knows_its_first_change() {
        let diff = parse("routes.js", 'M', PATCH);
        assert_eq!(
            diff.hunks[0].first_changed_line(),
            2,
            "the added line after context"
        );
        assert_eq!(diff.hunks[1].first_changed_line(), 11);
        // Only removed lines: where they were, in the new file.
        let removal = parse("f", 'M', "@@ -1,3 +1,2 @@\n a\n-b\n c\n");
        assert_eq!(removal.hunks[0].first_changed_line(), 2);
        assert_eq!(removal.first_changed_line(), 2);
    }

    #[test]
    fn staged_lines_map_through_the_index() {
        // HEAD: a b c d e f g h. The index: line 2 changed (B), a line added after f (f2).
        // The worktree: that, plus "new" at the top and line 7 removed (g), unstaged.
        let head_to_index = "@@ -2 +2 @@\n-b\n+B\n@@ -6,0 +7 @@\n+f2\n";
        let index_to_worktree = "@@ -0,0 +1 @@\n+new\n@@ -8 +8,0 @@\n-g\n";
        let head_to_worktree = "@@ -1,8 +1,9 @@\n+new\n a\n-b\n+B\n c\n d\n e\n f\n+f2\n-g\n h\n";
        let mut diff = parse("f", 'M', head_to_worktree);
        mark_staged(
            &mut diff,
            &parse("f", 'M', head_to_index),
            &parse("f", 'M', index_to_worktree),
        );
        let marks: Vec<(&str, bool)> = diff.hunks[0]
            .lines
            .iter()
            .filter(|l| l.kind != LineKind::Context)
            .map(|l| (l.text.as_str(), l.staged))
            .collect();
        assert_eq!(
            marks,
            [
                ("new", false),
                ("b", true),
                ("B", true),
                ("f2", true),
                ("g", false)
            ]
        );
        assert_eq!(diff.staged, 3);
        // Line numbers through a diff: before, inside and after its hunks.
        let unstaged = parse("f", 'M', index_to_worktree);
        assert_eq!(old_number(&unstaged, 1), None, "added in the worktree");
        assert_eq!(old_number(&unstaged, 2), Some(1));
        assert_eq!(old_number(&unstaged, 8), Some(7));
        assert_eq!(old_number(&unstaged, 9), Some(9), "after the removed line");
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
        // "two" to "TWO" is staged; "three" is not.
        let marks: Vec<(LineKind, &str, bool)> = modified.hunks[0]
            .lines
            .iter()
            .map(|l| (l.kind, l.text.as_str(), l.staged))
            .collect();
        assert_eq!(
            marks,
            [
                (LineKind::Context, "one", false),
                (LineKind::Removed, "two", true),
                (LineKind::Added, "TWO", true),
                (LineKind::Added, "three", false),
            ]
        );
        assert_eq!(modified.staged, 2);
        assert_eq!(modified.hunks[0].staged(), Staged::Part);
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
