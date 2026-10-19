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
    pub dir: PathBuf,
    pub zsh_dir: PathBuf,
    pub bash: PathBuf,
    pub powershell: PathBuf,
}

/// Kiln's own prompt inside Kiln: "❯ " in `ok`, or in `failed` after a failed command
/// (colours as 0xRRGGBB). The shell's line editor stays as it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinimalPrompt {
    pub ok: u32,
    pub failed: u32,
}

impl MinimalPrompt {
    fn rgb(color: u32) -> String {
        format!(
            "{};{};{}",
            (color >> 16) & 0xff,
            (color >> 8) & 0xff,
            color & 0xff
        )
    }

    /// A Starship config with only the prompt character, for users of Starship.
    fn starship_config(&self) -> String {
        format!(
            "# Kiln's prompt inside Kiln: the character only. Outside Kiln your own\n\
             # starship.toml applies. Written by Kiln; changes are overwritten.\n\
             add_newline = false\n\
             format = \"$character\"\n\
             right_format = \"\"\n\
             \n\
             [character]\n\
             success_symbol = \"[❯](#{ok:06x})\"\n\
             error_symbol = \"[❯](#{failed:06x})\"\n\
             vimcmd_symbol = \"[❮](#{ok:06x})\"\n",
            ok = self.ok,
            failed = self.failed,
        )
    }
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
        dir: dir.to_path_buf(),
        zsh_dir,
        bash,
        powershell,
    })
}

/// Adjusts the command and environment so the shell loads the integration (and, with
/// `prompt`, shows Kiln's minimal prompt). Returns `false` for shells Kiln has no integration
/// for.
pub fn apply(
    installed: &Installed,
    shell: &mut ShellCommand,
    env: &mut HashMap<String, String>,
    prompt: Option<MinimalPrompt>,
) -> bool {
    env.insert("KILN_SHELL_INTEGRATION".into(), "1".into());
    if let Some(prompt) = prompt {
        env.insert("KILN_PROMPT".into(), "kiln".into());
        env.insert("KILN_PROMPT_OK".into(), MinimalPrompt::rgb(prompt.ok));
        env.insert("KILN_PROMPT_FAIL".into(), MinimalPrompt::rgb(prompt.failed));
        let path = installed.dir.join(format!(
            "starship-{:06x}-{:06x}.toml",
            prompt.ok, prompt.failed
        ));
        match write_if_changed(&path, &prompt.starship_config()) {
            Ok(()) => {
                env.insert("KILN_STARSHIP_CONFIG".into(), path_string(&path));
            }
            Err(err) => log::warn!("could not write the Starship prompt config: {err}"),
        }
    }
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
            // Inline, not a script file: Windows client machines default to an execution
            // policy that blocks .ps1 files, but it does not apply to -EncodedCommand.
            shell.args.extend([
                "-NoExit".into(),
                "-EncodedCommand".into(),
                encode_powershell(POWERSHELL),
            ]);
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
                "KILN_PROMPT/u",
                "KILN_PROMPT_OK/u",
                "KILN_PROMPT_FAIL/u",
                "KILN_STARSHIP_CONFIG/p",
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
            for var in [
                "KILN_SHELL_INTEGRATION",
                "KILN_PROMPT",
                "KILN_PROMPT_OK",
                "KILN_PROMPT_FAIL",
                "KILN_STARSHIP_CONFIG",
            ] {
                env.remove(var);
            }
            false
        }
    }
}

/// Base64 of the UTF-16LE text, as `powershell -EncodedCommand` expects.
fn encode_powershell(script: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = (chunk[0] as u32) << 16
            | (*chunk.get(1).unwrap_or(&0) as u32) << 8
            | *chunk.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
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
        let ancestors = dir.ancestors();
        for path in ancestors {
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
    fn encodes_powershell_like_the_reference() {
        // [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes("ls"))
        assert_eq!(encode_powershell("ls"), "bABzAA==");
        assert_eq!(encode_powershell("echo é"), "ZQBjAGgAbwAgAOkA");
    }

    #[test]
    fn zsh_gets_zdotdir_and_powershell_gets_the_script() {
        let dir = std::env::temp_dir().join(format!("kiln-int-{}", std::process::id()));
        let installed = install(&dir).unwrap();
        assert!(installed.zsh_dir.join(".zshenv").is_file());

        let mut env = HashMap::new();
        let mut zsh = ShellCommand::new("/bin/zsh", ["-l"]);
        assert!(apply(&installed, &mut zsh, &mut env, None));
        assert_eq!(env["ZDOTDIR"], path_string(&installed.zsh_dir));

        let mut env = HashMap::new();
        let mut pwsh = ShellCommand::new("pwsh.exe", ["-NoLogo"]);
        assert!(apply(&installed, &mut pwsh, &mut env, None));
        assert_eq!(
            pwsh.args[1..3],
            ["-NoExit".to_string(), "-EncodedCommand".to_string()]
        );

        let mut env = HashMap::new();
        let mut fish = ShellCommand::new("/opt/homebrew/bin/fish", ["-l"]);
        assert!(!apply(&installed, &mut fish, &mut env, None));
        assert!(env.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn the_minimal_prompt_brings_its_colours_and_a_starship_config() {
        let dir = std::env::temp_dir().join(format!("kiln-prompt-{}", std::process::id()));
        let installed = install(&dir).unwrap();
        let mut env = HashMap::new();
        let mut zsh = ShellCommand::new("/bin/zsh", ["-l"]);
        let prompt = MinimalPrompt {
            ok: 0xe07b39,
            failed: 0xb8645a,
        };
        assert!(apply(&installed, &mut zsh, &mut env, Some(prompt)));
        assert_eq!(env["KILN_PROMPT"], "kiln");
        assert_eq!(env["KILN_PROMPT_OK"], "224;123;57");
        assert_eq!(env["KILN_PROMPT_FAIL"], "184;100;90");
        let starship = fs::read_to_string(&env["KILN_STARSHIP_CONFIG"]).unwrap();
        assert!(starship.contains("format = \"$character\""));
        assert!(starship.contains("[❯](#e07b39)"));
        let _ = fs::remove_dir_all(dir);
    }
}
