//! The dim context in the input bar: the Node version in JavaScript projects, and the WSL
//! distribution on Windows.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use kiln_term::ShellCommand;

/// `node --version` results per binary, so each binary runs at most once.
static VERSIONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

/// Whether `cwd` is inside a JavaScript project (stops at the home folder).
pub fn is_node_project(cwd: &str) -> bool {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    for dir in Path::new(cwd).ancestors() {
        if ["package.json", ".nvmrc", ".node-version"]
            .iter()
            .any(|f| dir.join(f).is_file())
        {
            return true;
        }
        if dir.as_os_str() == home.as_str() || dir.join(".git").exists() {
            return false;
        }
    }
    false
}

/// A version read from the install path, when version managers put one there
/// (`…/node/v20.11.0/bin/node`, `…/installs/node/22.3.0/bin/node`, `…/Cellar/node/22.3.0/…`).
pub fn version_from_path(path: &str) -> Option<String> {
    path.split(['/', '\\']).find_map(|part| {
        let v = part.strip_prefix('v').unwrap_or(part);
        let mut pieces = v.split('.');
        let ok = pieces
            .next()
            .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
            && pieces
                .next()
                .is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()));
        ok.then(|| v.to_string())
    })
}

/// The version of a node binary: from its path, a cache, or by running it once. Blocking;
/// call it off the UI thread.
pub fn node_version(path: &str) -> Option<String> {
    if let Some(v) = version_from_path(path) {
        return Some(v);
    }
    if let Some(cached) = VERSIONS
        .lock()
        .ok()?
        .get_or_insert_with(HashMap::new)
        .get(path)
    {
        return cached.clone();
    }
    let version = std::process::Command::new(path)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| {
            String::from_utf8_lossy(&o.stdout)
                .trim()
                .trim_start_matches('v')
                .to_string()
        })
        .filter(|v| !v.is_empty());
    VERSIONS
        .lock()
        .ok()?
        .get_or_insert_with(HashMap::new)
        .insert(path.to_string(), version.clone());
    version
}

/// "WSL · Ubuntu" for WSL sessions.
pub fn wsl_label(shell: &ShellCommand) -> Option<String> {
    let program = Path::new(&shell.program)
        .file_stem()?
        .to_string_lossy()
        .to_ascii_lowercase();
    if program != "wsl" {
        return None;
    }
    let distro = shell
        .args
        .iter()
        .position(|a| a == "--distribution" || a == "-d")
        .and_then(|i| shell.args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "default".into());
    Some(format!("WSL · {distro}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_versions_from_install_paths() {
        assert_eq!(
            version_from_path("/Users/m/.nvm/versions/node/v20.11.0/bin/node").as_deref(),
            Some("20.11.0")
        );
        assert_eq!(
            version_from_path("/Users/m/.local/share/mise/installs/node/22.3.0/bin/node")
                .as_deref(),
            Some("22.3.0")
        );
        assert_eq!(version_from_path("/opt/homebrew/bin/node"), None);
    }

    #[test]
    fn labels_wsl_sessions() {
        assert_eq!(
            wsl_label(&ShellCommand::wsl_for_test("Ubuntu")).as_deref(),
            Some("WSL · Ubuntu")
        );
        assert_eq!(wsl_label(&ShellCommand::new("/bin/zsh", ["-l"])), None);
    }
}
