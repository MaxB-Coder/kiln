//! Opening a file in the user's editor at a line (the diff viewer's ⌘O): `$VISUAL` or
//! `$EDITOR` (from Kiln's environment, else the login shell's, as an app started from the
//! Dock has neither), each editor with its own way of naming the line; terminal editors run in
//! a new session; with none set, the system's default app opens the file.

use std::path::Path;
use std::process::{Command, Stdio};

/// How to open a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// A window of its own: run it in the background.
    App { program: String, args: Vec<String> },
    /// It needs a terminal: a new session runs it.
    Terminal { program: String, args: Vec<String> },
    /// No editor set: the system's default app.
    System,
}

/// The editor command (as `$EDITOR` holds it, arguments and all) applied to a file and line.
pub fn plan(editor: Option<&str>, path: &Path, line: u32) -> Launch {
    let Some(mut words) = editor.map(|e| e.split_whitespace().map(str::to_string)) else {
        return Launch::System;
    };
    let Some(program) = words.next() else {
        return Launch::System;
    };
    // "code --wait" is for git; here the editor need not hold anything up.
    let mut args: Vec<String> = words
        .filter(|w| !matches!(w.as_str(), "--wait" | "-w" | "-W"))
        .collect();
    let file = path.to_string_lossy().to_string();
    let at = format!("{file}:{line}");
    let name = Path::new(&program)
        .file_stem()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let terminal = |args: Vec<String>| Launch::Terminal {
        program: program.clone(),
        args,
    };
    match name.as_str() {
        "vi" | "vim" | "nvim" | "nano" | "micro" | "kak" | "emacs" | "ne" | "mg" | "joe" => {
            args.extend([format!("+{line}"), file]);
            terminal(args)
        }
        "hx" | "helix" => {
            args.push(at);
            terminal(args)
        }
        "code" | "code-insiders" | "codium" | "cursor" | "windsurf" => {
            args.extend(["-g".to_string(), at]);
            Launch::App { program, args }
        }
        "zed" | "subl" | "sublime_text" => {
            args.push(at);
            Launch::App { program, args }
        }
        "mate" => {
            args.extend(["-l".to_string(), line.to_string(), file]);
            Launch::App { program, args }
        }
        "idea" | "clion" | "pycharm" | "webstorm" | "goland" | "rustrover" | "phpstorm" => {
            args.extend(["--line".to_string(), line.to_string(), file]);
            Launch::App { program, args }
        }
        _ => {
            args.push(file);
            Launch::App { program, args }
        }
    }
}

/// The user's editor: `$VISUAL`, else `$EDITOR`, from Kiln's environment, else as the login
/// shell sets them (read once per call; blocking, so call it off the UI thread).
pub fn user_editor() -> Option<String> {
    let from_env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    if let Some(editor) = from_env("VISUAL").or_else(|| from_env("EDITOR")) {
        return Some(editor);
    }
    if cfg!(windows) {
        return None;
    }
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let output = Command::new(shell)
        .args(["-lic", "printf '\\n%s' \"${VISUAL:-$EDITOR}\""])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    // The last line: startup files may print something first.
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .last()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_editor_gets_the_line_its_way() {
        let path = Path::new("/code/app/src/main.rs");
        let app = |program: &str, args: &[&str]| Launch::App {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
        };
        let terminal = |program: &str, args: &[&str]| Launch::Terminal {
            program: program.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
        };
        assert_eq!(plan(None, path, 12), Launch::System);
        assert_eq!(plan(Some("  "), path, 12), Launch::System);
        assert_eq!(
            plan(Some("code --wait"), path, 12),
            app("code", &["-g", "/code/app/src/main.rs:12"])
        );
        assert_eq!(
            plan(Some("nvim"), path, 12),
            terminal("nvim", &["+12", "/code/app/src/main.rs"])
        );
        assert_eq!(
            plan(Some("/opt/homebrew/bin/hx"), path, 3),
            terminal("/opt/homebrew/bin/hx", &["/code/app/src/main.rs:3"])
        );
        assert_eq!(
            plan(Some("subl -w"), path, 7),
            app("subl", &["/code/app/src/main.rs:7"])
        );
        assert_eq!(
            plan(Some("gedit"), path, 7),
            app("gedit", &["/code/app/src/main.rs"])
        );
    }
}
