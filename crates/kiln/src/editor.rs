//! Opening a file in the user's editor at a line (the diff viewer's Open, ⌘O). The `editor`
//! setting picks it: "auto" (`$VISUAL`, else `$EDITOR`, as the login shell sets them, since
//! an app started from the Dock has neither), "system" (the default app for the file), one of
//! the IDEs below by name, or any command. Commands run through the login shell, so they are
//! found on the user's PATH (an app gets only the system's); terminal editors run in a new
//! session.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// How an IDE is told the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LineStyle {
    /// `-g file:12` (VS Code and its forks).
    Goto,
    /// `file:12`.
    Colon,
    /// `--line 12 file` (JetBrains).
    DashLine,
    /// `-l 12 file` (Xcode's xed).
    DashL,
}

/// An IDE Kiln knows: its name (as the setting holds it), its app on macOS, and the command
/// inside it (or on the PATH elsewhere).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ide {
    pub name: &'static str,
    bundle: &'static str,
    /// The command inside the bundle (macOS); "" for "open the app with arguments".
    inside: &'static str,
    /// The command on the PATH (Windows, Linux).
    command: &'static str,
    style: LineStyle,
}

pub const IDES: [Ide; 11] = [
    Ide {
        name: "Visual Studio Code",
        bundle: "Visual Studio Code.app",
        inside: "Contents/Resources/app/bin/code",
        command: "code",
        style: LineStyle::Goto,
    },
    Ide {
        name: "Cursor",
        bundle: "Cursor.app",
        inside: "Contents/Resources/app/bin/cursor",
        command: "cursor",
        style: LineStyle::Goto,
    },
    Ide {
        name: "Windsurf",
        bundle: "Windsurf.app",
        inside: "Contents/Resources/app/bin/windsurf",
        command: "windsurf",
        style: LineStyle::Goto,
    },
    Ide {
        name: "Zed",
        bundle: "Zed.app",
        inside: "Contents/MacOS/cli",
        command: "zed",
        style: LineStyle::Colon,
    },
    Ide {
        name: "Sublime Text",
        bundle: "Sublime Text.app",
        inside: "Contents/SharedSupport/bin/subl",
        command: "subl",
        style: LineStyle::Colon,
    },
    // JetBrains apps open through `open -na … --args`, which hands the file to the running IDE.
    Ide {
        name: "IntelliJ IDEA",
        bundle: "IntelliJ IDEA.app",
        inside: "",
        command: "idea",
        style: LineStyle::DashLine,
    },
    Ide {
        name: "IntelliJ IDEA CE",
        bundle: "IntelliJ IDEA CE.app",
        inside: "",
        command: "idea",
        style: LineStyle::DashLine,
    },
    Ide {
        name: "RustRover",
        bundle: "RustRover.app",
        inside: "",
        command: "rustrover",
        style: LineStyle::DashLine,
    },
    Ide {
        name: "WebStorm",
        bundle: "WebStorm.app",
        inside: "",
        command: "webstorm",
        style: LineStyle::DashLine,
    },
    Ide {
        name: "PyCharm",
        bundle: "PyCharm.app",
        inside: "",
        command: "pycharm",
        style: LineStyle::DashLine,
    },
    Ide {
        name: "Xcode",
        bundle: "Xcode.app",
        inside: "",
        command: "xed",
        style: LineStyle::DashL,
    },
];

/// How to open a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launch {
    /// A program by its full path, in the background.
    App { program: PathBuf, args: Vec<String> },
    /// A command (found on the user's PATH: see `through_shell`), in the background: an editor
    /// with its own window.
    Shell { program: String, args: Vec<String> },
    /// A command in a new session: an editor that needs a terminal.
    Terminal { program: String, args: Vec<String> },
    /// The system's default app for the file.
    System,
}

/// The IDEs installed here: on macOS by their app in /Applications or ~/Applications, elsewhere
/// by their command on the PATH.
pub fn installed() -> Vec<(Ide, PathBuf)> {
    IDES.iter()
        .filter_map(|ide| locate(ide).map(|place| (*ide, place)))
        .collect()
}

/// The Settings row's choices, as the setting holds them: "auto", each installed IDE, "system".
pub fn choices() -> Vec<&'static str> {
    std::iter::once("auto")
        .chain(installed().into_iter().map(|(ide, _)| ide.name))
        .chain(["system"])
        .collect()
}

/// A setting as the Settings row shows it; `env_editor` is the "auto" editor.
pub fn label(setting: &str, env_editor: Option<&str>) -> String {
    match setting.trim() {
        "" | "auto" => match env_editor {
            Some(editor) => format!("$EDITOR ({editor})"),
            None => "$EDITOR".into(),
        },
        "system" => "Default app".into(),
        other => IDES
            .iter()
            .find(|ide| ide.name.eq_ignore_ascii_case(other))
            .map_or_else(|| other.to_string(), |ide| ide.name.to_string()),
    }
}

/// Where an IDE is: its app bundle (macOS) or its command (elsewhere).
pub fn locate(ide: &Ide) -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_default();
        [PathBuf::from("/Applications"), home.join("Applications")]
            .into_iter()
            .map(|dir| dir.join(ide.bundle))
            .find(|app| app.is_dir())
    } else {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path).find_map(|dir| {
            [
                ide.command.to_string(),
                format!("{}.cmd", ide.command),
                format!("{}.exe", ide.command),
            ]
            .into_iter()
            .map(|name| dir.join(name))
            .find(|p| p.is_file())
        })
    }
}

/// `file` and `line` in an IDE's way.
fn line_args(style: LineStyle, file: &str, line: u32) -> Vec<String> {
    match style {
        LineStyle::Goto => vec!["-g".into(), format!("{file}:{line}")],
        LineStyle::Colon => vec![format!("{file}:{line}")],
        LineStyle::DashLine => vec!["--line".into(), line.to_string(), file.into()],
        LineStyle::DashL => vec!["-l".into(), line.to_string(), file.into()],
    }
}

/// For a shell: `'it''s'` style quoting.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// What the setting means for `path` at `line`. `env_editor`: `$VISUAL`/`$EDITOR` (for
/// "auto"); `ide_at`: where an IDE is installed (`installed`), for a setting naming one.
pub fn plan(
    setting: &str,
    env_editor: Option<&str>,
    ide_at: impl Fn(&Ide) -> Option<PathBuf>,
    path: &Path,
    line: u32,
) -> Launch {
    let file = path.to_string_lossy().to_string();
    let setting = setting.trim();
    match setting {
        "" | "auto" => match env_editor.map(str::trim).filter(|e| !e.is_empty()) {
            Some(editor) => command_plan(editor, &file, line),
            None => Launch::System,
        },
        "system" => Launch::System,
        name => match IDES.iter().find(|ide| ide.name.eq_ignore_ascii_case(name)) {
            Some(ide) => match ide_at(ide) {
                Some(place) => ide_plan(ide, &place, &file, line),
                // Not installed (any more): try its command.
                None => command_plan(ide.command, &file, line),
            },
            None => command_plan(name, &file, line),
        },
    }
}

/// An installed IDE: the command in its bundle, or `open -na App --args …` (macOS); its
/// command elsewhere.
fn ide_plan(ide: &Ide, place: &Path, file: &str, line: u32) -> Launch {
    let args = line_args(ide.style, file, line);
    if !cfg!(target_os = "macos") {
        return Launch::App {
            program: place.to_path_buf(),
            args,
        };
    }
    if ide.style == LineStyle::DashL {
        return Launch::App {
            program: "/usr/bin/xed".into(),
            args,
        };
    }
    if ide.inside.is_empty() {
        let mut open = vec![
            "-na".to_string(),
            place.to_string_lossy().to_string(),
            "--args".into(),
        ];
        open.extend(args);
        return Launch::App {
            program: "/usr/bin/open".into(),
            args: open,
        };
    }
    Launch::App {
        program: place.join(ide.inside),
        args,
    }
}

/// A command (as `$EDITOR` holds it, arguments and all): terminal editors in a new session,
/// the rest in the background, each told the line its way.
fn command_plan(editor: &str, file: &str, line: u32) -> Launch {
    let mut words = editor.split_whitespace();
    let Some(program) = words.next() else {
        return Launch::System;
    };
    // "code --wait" is for git; here nothing need wait.
    let mut args: Vec<String> = words
        .filter(|w| !matches!(*w, "--wait" | "-w" | "-W"))
        .map(str::to_string)
        .collect();
    let name = Path::new(program)
        .file_stem()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let (terminal, file_args) = match name.as_str() {
        "vi" | "vim" | "nvim" | "nano" | "micro" | "kak" | "emacs" | "ne" | "mg" | "joe" => {
            (true, vec![format!("+{line}"), file.to_string()])
        }
        "hx" | "helix" => (true, vec![format!("{file}:{line}")]),
        "code" | "code-insiders" | "codium" | "cursor" | "windsurf" => {
            (false, line_args(LineStyle::Goto, file, line))
        }
        "zed" | "subl" | "sublime_text" => (false, line_args(LineStyle::Colon, file, line)),
        "idea" | "clion" | "pycharm" | "webstorm" | "goland" | "rustrover" | "phpstorm" => {
            (false, line_args(LineStyle::DashLine, file, line))
        }
        "xed" => (false, line_args(LineStyle::DashL, file, line)),
        "mate" => (false, vec!["-l".into(), line.to_string(), file.to_string()]),
        _ => (false, vec![file.to_string()]),
    };
    args.extend(file_args);
    let program = program.to_string();
    if terminal {
        Launch::Terminal { program, args }
    } else {
        Launch::Shell { program, args }
    }
}

/// A command run by the login shell (`$SHELL -lic 'exec …'`), so it is found on the user's
/// PATH: an app started from the Dock or Finder gets only the system's. Windows runs it as it
/// is. The program stays unquoted, so `~/bin/edit` works.
pub fn through_shell(program: &str, args: &[String]) -> (String, Vec<String>) {
    if cfg!(windows) {
        return (program.to_string(), args.to_vec());
    }
    let line = std::iter::once(program.to_string())
        .chain(args.iter().map(|a| quote(a)))
        .collect::<Vec<_>>()
        .join(" ");
    (login_shell(), vec!["-lic".into(), format!("exec {line}")])
}

/// The user's shell, to run commands with their PATH.
pub fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".into())
}

/// The user's editor: `$VISUAL`, else `$EDITOR`, from Kiln's environment, else as the login
/// shell sets them (blocking; call it off the UI thread).
pub fn user_editor() -> Option<String> {
    let from_env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    if let Some(editor) = from_env("VISUAL").or_else(|| from_env("EDITOR")) {
        return Some(editor);
    }
    if cfg!(windows) {
        return None;
    }
    let output = Command::new(login_shell())
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

    const FILE: &str = "/code/app/src/main.rs";

    fn plan_for(setting: &str, env: Option<&str>, installed: &[&str]) -> Launch {
        plan(
            setting,
            env,
            |ide| {
                installed
                    .contains(&ide.name)
                    .then(|| PathBuf::from("/Applications").join(ide.bundle))
            },
            Path::new(FILE),
            12,
        )
    }

    #[test]
    fn auto_uses_the_shells_editor_through_the_shell() {
        // nvim is found on the user's PATH because the login shell runs it.
        assert_eq!(
            plan_for("auto", Some("nvim"), &[]),
            Launch::Terminal {
                program: "nvim".into(),
                args: vec!["+12".into(), FILE.into()]
            }
        );
        assert_eq!(
            plan_for("auto", Some("code --wait"), &[]),
            Launch::Shell {
                program: "code".into(),
                args: vec!["-g".into(), format!("{FILE}:12")]
            }
        );
        assert_eq!(plan_for("auto", None, &[]), Launch::System);
        assert_eq!(plan_for("system", Some("nvim"), &[]), Launch::System);
        assert_eq!(
            plan_for("hx", None, &[]),
            Launch::Terminal {
                program: "hx".into(),
                args: vec![format!("{FILE}:12")]
            }
        );
    }

    #[test]
    fn installed_ides_open_at_the_line() {
        if !cfg!(target_os = "macos") {
            return;
        }
        assert_eq!(
            plan_for("IntelliJ IDEA", None, &["IntelliJ IDEA"]),
            Launch::App {
                program: "/usr/bin/open".into(),
                args: vec![
                    "-na".into(),
                    "/Applications/IntelliJ IDEA.app".into(),
                    "--args".into(),
                    "--line".into(),
                    "12".into(),
                    FILE.into()
                ]
            }
        );
        assert_eq!(
            plan_for("visual studio code", None, &["Visual Studio Code"]),
            Launch::App {
                program: "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code"
                    .into(),
                args: vec!["-g".into(), format!("{FILE}:12")]
            }
        );
        // Named but not installed: its command, through the shell.
        assert_eq!(
            plan_for("Zed", None, &[]),
            Launch::Shell {
                program: "zed".into(),
                args: vec![format!("{FILE}:12")]
            }
        );
    }

    #[test]
    fn commands_run_through_the_login_shell() {
        if cfg!(windows) {
            return;
        }
        let (shell, args) = through_shell("nvim", &["+12".into(), "/code/it's here.rs".into()]);
        assert_eq!(shell, login_shell());
        assert_eq!(args, ["-lic", "exec nvim '+12' '/code/it'\\''s here.rs'"]);
    }
}
