//! Context for the input bar and the rail: language runtimes the current project uses, and the
//! WSL distribution on Windows.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use kiln_term::ShellCommand;

/// `--version` results per binary path, so each binary runs at most once.
static VERSIONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

/// Runtimes Kiln knows: the tool on PATH, its display name, and the files that mark a project
/// as using it.
const RUNTIMES: &[(&str, &str, &[&str])] = &[
    ("node", "node", &["package.json", ".nvmrc", ".node-version"]),
    (
        "python3",
        "python",
        &[
            "pyproject.toml",
            "requirements.txt",
            "setup.py",
            ".python-version",
            "Pipfile",
        ],
    ),
    (
        "python",
        "python",
        &[
            "pyproject.toml",
            "requirements.txt",
            "setup.py",
            ".python-version",
            "Pipfile",
        ],
    ),
    (
        "rustc",
        "rust",
        &["Cargo.toml", "rust-toolchain.toml", "rust-toolchain"],
    ),
    ("go", "go", &["go.mod"]),
    ("ruby", "ruby", &["Gemfile", ".ruby-version"]),
    (
        "java",
        "java",
        &["pom.xml", "build.gradle", "build.gradle.kts"],
    ),
    ("deno", "deno", &["deno.json", "deno.jsonc"]),
    ("bun", "bun", &["bun.lockb", "bun.lock", "bunfig.toml"]),
];

/// Marker files of the project at `cwd`: from `cwd` up to its repository's root, or in `cwd`
/// alone outside a repository. Never the home folder's own files: Home is not a project.
fn markers(cwd: &str) -> Vec<String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    markers_in(Path::new(cwd), Path::new(&home))
}

fn markers_in(cwd: &Path, home: &Path) -> Vec<String> {
    let below_home = |dir: &&Path| home.as_os_str().is_empty() || *dir != home;
    let in_repo = cwd
        .ancestors()
        .take_while(below_home)
        .any(|dir| dir.join(".git").exists());
    let mut found = Vec::new();
    for dir in cwd.ancestors().take_while(below_home) {
        for (_, _, files) in RUNTIMES {
            for file in *files {
                if !found.iter().any(|f| f == file) && dir.join(file).exists() {
                    found.push(file.to_string());
                }
            }
        }
        if !in_repo || dir.join(".git").exists() {
            break;
        }
    }
    found
}

/// Runtimes the project at `cwd` uses, with versions: `[("node", "22.3.0"), …]`. Blocking (may
/// run `--version` once per binary); call it off the UI thread.
pub fn project_runtimes(cwd: &str, tools: &[(String, String)]) -> Vec<(String, String)> {
    let markers = markers(cwd);
    let mut out: Vec<(String, String)> = Vec::new();
    for (tool, display, files) in RUNTIMES {
        if out.iter().any(|(d, _)| d == display)
            || !files.iter().any(|f| markers.iter().any(|m| m == f))
        {
            continue;
        }
        let Some((_, path)) = tools.iter().find(|(name, _)| name == tool) else {
            continue;
        };
        if let Some(version) = tool_version(tool, path) {
            out.push((display.to_string(), version));
        }
    }
    out
}

/// A version read from the install path, when version managers put one there
/// (`…/node/v20.11.0/bin/node`, `…/installs/node/22.3.0/bin/node`).
pub fn version_from_path(path: &str) -> Option<String> {
    path.split(['/', '\\']).find_map(|part| {
        let v = part.strip_prefix('v').unwrap_or(part);
        looks_like_version(v).then(|| v.to_string())
    })
}

fn looks_like_version(v: &str) -> bool {
    let mut pieces = v.split('.');
    pieces
        .next()
        .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        && pieces
            .next()
            .is_some_and(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// First version-looking word in a tool's `--version` output ("go1.22.1" counts).
fn parse_version(output: &str) -> Option<String> {
    output
        .split(|c: char| c.is_whitespace() || c == '"' || c == ',')
        .map(|w| w.trim_start_matches('v').trim_start_matches("go"))
        .find(|w| looks_like_version(w))
        .map(|w| {
            w.trim_end_matches(|c: char| !c.is_ascii_digit())
                .to_string()
        })
}

/// The version of a runtime binary: from its path, a cache, or by running it once.
pub fn tool_version(tool: &str, path: &str) -> Option<String> {
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
    let args: &[&str] = match tool {
        "go" => &["version"],
        "java" => &["-version"],
        _ => &["--version"],
    };
    let version = std::process::Command::new(path)
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            // java prints its version on stderr.
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&o.stdout),
                String::from_utf8_lossy(&o.stderr)
            );
            parse_version(&text)
        });
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
    fn runtimes_only_where_the_project_uses_them() {
        let home = std::env::temp_dir().join(format!("kiln-markers-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let touch = |path: &str| {
            let path = home.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "").unwrap();
        };
        touch("Cargo.toml");
        touch("code/app/.git/HEAD");
        touch("code/app/package.json");
        touch("code/app/src/main.ts");
        touch("code/tool/go.mod");
        touch("notes/todo.md");
        let at = |dir: &str| markers_in(&home.join(dir), &home);
        // Home is not a project, even with a Cargo.toml in it.
        assert!(markers_in(&home, &home).is_empty());
        // In a repository, from the folder up to its root.
        assert_eq!(at("code/app/src"), ["package.json"]);
        // Outside one, the folder's own files only.
        assert_eq!(at("code/tool"), ["go.mod"]);
        assert!(at("notes").is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn reads_versions_from_install_paths() {
        assert_eq!(
            version_from_path("/Users/m/.nvm/versions/node/v20.11.0/bin/node").as_deref(),
            Some("20.11.0")
        );
        assert_eq!(version_from_path("/opt/homebrew/bin/node"), None);
    }

    #[test]
    fn parses_version_output() {
        assert_eq!(parse_version("v22.3.0\n").as_deref(), Some("22.3.0"));
        assert_eq!(
            parse_version("rustc 1.98.1 (48a229cea 2026-09-01)").as_deref(),
            Some("1.98.1")
        );
        assert_eq!(
            parse_version("go version go1.22.1 darwin/arm64").as_deref(),
            Some("1.22.1")
        );
        assert_eq!(
            parse_version("openjdk version \"21.0.2\" 2024-01-16").as_deref(),
            Some("21.0.2")
        );
        assert_eq!(parse_version("Python 3.14.0").as_deref(), Some("3.14.0"));
    }

    #[test]
    fn finds_this_repos_rust_project() {
        assert!(
            markers(env!("CARGO_MANIFEST_DIR"))
                .iter()
                .any(|m| m == "Cargo.toml")
        );
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
