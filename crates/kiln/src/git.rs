//! Git status for the rail's "Changes" section, read from `git status --porcelain=v2`.

use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitStatus {
    pub root: String,
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileChange>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileChange {
    /// `M` modified, `A` added, `D` deleted, `R` renamed, `U` conflict, `?` untracked.
    pub kind: char,
    pub path: String,
}

/// Status of the repository containing `cwd`, or `None` outside a repository (or without git).
/// Blocking; call it off the UI thread.
pub fn status(cwd: &str) -> Option<GitStatus> {
    if !Path::new(cwd).is_dir() {
        return None;
    }
    let root = run(cwd, &["rev-parse", "--show-toplevel"])?;
    let root = String::from_utf8_lossy(&root).trim().to_string();
    let out = run(
        &root,
        &[
            "status",
            "--porcelain=v2",
            "--branch",
            "-z",
            "--untracked-files=normal",
        ],
    )?;
    let mut status = parse(&String::from_utf8_lossy(&out));
    status.root = root;
    Some(status)
}

/// A pane command that pages one file's changes against HEAD (untracked files: the whole file)
/// and exits when the pager closes.
pub fn diff_command(root: &str, file: &FileChange) -> kiln_term::ShellCommand {
    let untracked = file.kind == '?';
    if cfg!(windows) {
        let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let what = if untracked {
            format!("--no-index -- /dev/null {}", quote(&file.path))
        } else {
            format!("HEAD -- {}", quote(&file.path))
        };
        kiln_term::ShellCommand::new(
            "powershell.exe",
            [
                "-NoLogo".to_string(),
                "-NoProfile".to_string(),
                "-Command".to_string(),
                format!(
                    "git --literal-pathspecs -C {} --no-pager diff {what} | more",
                    quote(root)
                ),
            ],
        )
    } else {
        let what = if untracked {
            "--no-index -- /dev/null \"$2\""
        } else {
            "HEAD -- \"$2\""
        };
        // -+F -+E: stay open for short diffs even when $LESS says otherwise.
        let script = format!(
            "git --literal-pathspecs -C \"$1\" --no-pager diff --color=always {what} | less -R -+F -+E"
        );
        kiln_term::ShellCommand::new(
            "/bin/sh",
            [
                "-c".to_string(),
                script,
                "kiln-diff".to_string(),
                root.to_string(),
                file.path.clone(),
            ],
        )
    }
}

fn run(dir: &str, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new("git")
        // A repository's own config must not run commands here: the rail reads status in any
        // folder the user enters.
        .args(["-c", "core.fsmonitor=false"])
        .arg("-C")
        .arg(dir)
        .args(args)
        // Never prompt, never page, never run hooks or fsmonitor from here.
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn parse(text: &str) -> GitStatus {
    let mut status = GitStatus::default();
    let mut fields = text.split('\0').peekable();
    while let Some(entry) = fields.next() {
        if let Some(head) = entry.strip_prefix("# branch.head ") {
            status.branch = (head != "(detached)").then(|| head.to_string());
        } else if let Some(ab) = entry.strip_prefix("# branch.ab ") {
            for part in ab.split(' ') {
                if let Some(n) = part.strip_prefix('+') {
                    status.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = part.strip_prefix('-') {
                    status.behind = n.parse().unwrap_or(0);
                }
            }
        } else if let Some(rest) = entry.strip_prefix("1 ") {
            // 1 XY sub mH mI mW hH hI path
            let xy = rest.get(..2).unwrap_or("..");
            if let Some(path) = rest.splitn(8, ' ').nth(7) {
                status.files.push(FileChange {
                    kind: kind_of(xy),
                    path: path.to_string(),
                });
            }
        } else if let Some(rest) = entry.strip_prefix("2 ") {
            // 2 XY sub mH mI mW hH hI Xscore path, then the original path as the next field.
            if let Some(path) = rest.splitn(9, ' ').nth(8) {
                status.files.push(FileChange {
                    kind: 'R',
                    path: path.to_string(),
                });
            }
            fields.next();
        } else if let Some(rest) = entry.strip_prefix("u ") {
            if let Some(path) = rest.splitn(10, ' ').nth(9) {
                status.files.push(FileChange {
                    kind: 'U',
                    path: path.to_string(),
                });
            }
        } else if let Some(path) = entry.strip_prefix("? ") {
            status.files.push(FileChange {
                kind: '?',
                path: path.to_string(),
            });
        }
    }
    status
}

fn kind_of(xy: &str) -> char {
    let mut chars = xy.chars();
    let (x, y) = (chars.next().unwrap_or('.'), chars.next().unwrap_or('.'));
    for c in [y, x] {
        match c {
            'A' => return 'A',
            'D' => return 'D',
            'R' | 'C' => return 'R',
            'M' | 'T' => return 'M',
            _ => {}
        }
    }
    'M'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_porcelain_v2() {
        let text = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
1 .M N... 100644 100644 100644 a b src/lib.rs\0\
1 A. N... 000000 100644 100644 0 b new file.txt\0\
2 R. N... 100644 100644 100644 a b R100 renamed.rs\0old.rs\0\
? notes/todo.md\0";
        let s = parse(text);
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        let files: Vec<_> = s.files.iter().map(|f| (f.kind, f.path.as_str())).collect();
        assert_eq!(
            files,
            vec![
                ('M', "src/lib.rs"),
                ('A', "new file.txt"),
                ('R', "renamed.rs"),
                ('?', "notes/todo.md")
            ]
        );
    }

    #[test]
    fn reads_this_repository() {
        let here = env!("CARGO_MANIFEST_DIR");
        let status = status(here).expect("this crate is in a git repository");
        let root = std::fs::canonicalize(&status.root).unwrap();
        assert!(std::fs::canonicalize(here).unwrap().starts_with(root));
    }
}
