//! Shell integration: ships the scripts and makes each shell load them, without touching the
//! user's startup files.
//!
//! - zsh: `ZDOTDIR` points at Kiln's folder, whose `.zshenv` restores `ZDOTDIR`, runs the user's
//!   own `.zshenv` and loads the integration. The rest of the user's files load as usual.
//! - bash: an exported `PROMPT_COMMAND` sources the integration at the first prompt.
//! - PowerShell: `-NoExit -Command ". 'kiln.ps1'"`.
//! - WSL: the same variables, passed into Linux through `WSLENV`.
//! - Other shells (fish, nu, …) run without integration: Kiln shows a classic terminal.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::shell::ShellCommand;

const ZSHENV: &str = include_str!("../shell/zshenv");
const ZSH: &str = include_str!("../shell/kiln.zsh");
const BASH: &str = include_str!("../shell/kiln.bash");
const POWERSHELL: &str = include_str!("../shell/kiln.ps1");

const BASH_BOOTSTRAP: &str = r#"[ -n "$KILN_BASH_INTEGRATION" ] && . "$KILN_BASH_INTEGRATION""#;

/// Where the scripts were written.
#[derive(Clone, Debug)]
pub struct Installed {
    pub zsh_dir: PathBuf,
    pub bash: PathBuf,
    pub powershell: PathBuf,
}

/// The per-user folder for the scripts, versioned so upgrades never mix files.
pub fn default_dir() -> Option<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"))
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    }?;
    Some(
        base.join("kiln")
            .join("shell-integration")
            .join(env!("CARGO_PKG_VERSION")),
    )
}

/// Writes the scripts into `dir` (only files whose content changed).
pub fn install(dir: &Path) -> io::Result<Installed> {
    let zsh_dir = dir.join("zsh");
    create_private_dir(&zsh_dir)?;
    write_if_changed(&zsh_dir.join(".zshenv"), ZSHENV)?;
    write_if_changed(&zsh_dir.join("kiln.zsh"), ZSH)?;
    let bash = dir.join("kiln.bash");
    write_if_changed(&bash, BASH)?;
    let powershell = dir.join("kiln.ps1");
    write_if_changed(&powershell, POWERSHELL)?;
    Ok(Installed {
        zsh_dir,
        bash,
        powershell,
    })
}

/// Adjusts the command and environment so the shell loads the integration. Returns `false`
/// for shells Kiln has no integration for.
pub fn apply(
    installed: &Installed,
    shell: &mut ShellCommand,
    env: &mut HashMap<String, String>,
) -> bool {
    env.insert("KILN_SHELL_INTEGRATION".into(), "1".into());
    match shell_kind(&shell.program).as_str() {
        "zsh" => {
            if let Ok(zdotdir) = std::env::var("ZDOTDIR") {
                env.insert("KILN_ZDOTDIR".into(), zdotdir);
            }
            env.insert("ZDOTDIR".into(), path_string(&installed.zsh_dir));
            true
        }
        "bash" => {
            env.insert("KILN_BASH_INTEGRATION".into(), path_string(&installed.bash));
            env.insert("PROMPT_COMMAND".into(), BASH_BOOTSTRAP.into());
            true
        }
        "pwsh" | "powershell" => {
            let has_command = shell.args.iter().any(|arg| {
                let arg = arg.to_ascii_lowercase();
                arg == "-command" || arg == "-c" || arg == "-file" || arg == "-f"
            });
            if has_command {
                return false;
            }
            let path = path_string(&installed.powershell).replace('\'', "''");
            shell
                .args
                .extend(["-NoExit".into(), "-Command".into(), format!(". '{path}'")]);
            true
        }
        "wsl" => {
            // Paths are translated to /mnt/… by WSL (the /p flag).
            env.insert("ZDOTDIR".into(), path_string(&installed.zsh_dir));
            env.insert("KILN_BASH_INTEGRATION".into(), path_string(&installed.bash));
            env.insert("PROMPT_COMMAND".into(), BASH_BOOTSTRAP.into());
            let mut wslenv = std::env::var("WSLENV").unwrap_or_default();
            for var in [
                "ZDOTDIR/p",
                "KILN_BASH_INTEGRATION/p",
                "PROMPT_COMMAND/u",
                "KILN_SHELL_INTEGRATION/u",
                "TERM_PROGRAM/u",
                "TERM_PROGRAM_VERSION/u",
                "COLORTERM/u",
            ] {
                if !wslenv.is_empty() {
                    wslenv.push(':');
                }
                wslenv.push_str(var);
            }
            env.insert("WSLENV".into(), wslenv);
            true
        }
        _ => {
            env.remove("KILN_SHELL_INTEGRATION");
            false
        }
    }
}

fn shell_kind(program: &str) -> String {
    Path::new(program)
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn create_private_dir(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        // Scripts in here run in every shell: nobody else may write them.
        let mut ancestors = dir.ancestors();
        while let Some(path) = ancestors.next() {
            if path.file_name().is_some_and(|n| n == "kiln") {
                fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
                break;
            }
        }
    }
    Ok(())
}

fn write_if_changed(path: &Path, content: &str) -> io::Result<()> {
    if fs::read_to_string(path).is_ok_and(|current| current == content) {
        return Ok(());
    }
    // Write next to it, then rename, so a shell starting meanwhile never reads half a file.
    // The temporary name is unique: several sessions may start at once.
    static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_file_name(format!(
        ".{}.{}-{n}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    fs::write(&tmp, content)?;
    let renamed = fs::rename(&tmp, path);
    let _ = fs::remove_file(&tmp);
    match renamed {
        Ok(()) => Ok(()),
        // Another session won the race with the same content.
        Err(_) if fs::read_to_string(path).is_ok_and(|current| current == content) => Ok(()),
        Err(err) => Err(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_gets_zdotdir_and_powershell_gets_the_script() {
        let dir = std::env::temp_dir().join(format!("kiln-int-{}", std::process::id()));
        let installed = install(&dir).unwrap();
        assert!(installed.zsh_dir.join(".zshenv").is_file());

        let mut env = HashMap::new();
        let mut zsh = ShellCommand::new("/bin/zsh", ["-l"]);
        assert!(apply(&installed, &mut zsh, &mut env));
        assert_eq!(env["ZDOTDIR"], path_string(&installed.zsh_dir));

        let mut env = HashMap::new();
        let mut pwsh = ShellCommand::new("pwsh.exe", ["-NoLogo"]);
        assert!(apply(&installed, &mut pwsh, &mut env));
        assert_eq!(
            pwsh.args[1..3],
            ["-NoExit".to_string(), "-Command".to_string()]
        );

        let mut env = HashMap::new();
        let mut fish = ShellCommand::new("/opt/homebrew/bin/fish", ["-l"]);
        assert!(!apply(&installed, &mut fish, &mut env));
        assert!(env.is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}
