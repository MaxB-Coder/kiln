//! What Kiln remembers between launches, in `state.json` in its data folder: the last window's
//! sessions (folders and splits), and a layout per project for "open project" in the palette.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// How many projects to remember.
const KEEP_PROJECTS: usize = 24;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    /// The window that changed last.
    pub window: Option<WindowState>,
    /// Most recently used first.
    pub projects: Vec<ProjectState>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub sidebar: bool,
    pub rail: bool,
    pub active: usize,
    pub tabs: Vec<TabState>,
}

/// A session: its panes' folders, left to right, and the name it was given, if any.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabState {
    pub panes: Vec<Option<String>>,
    pub focused: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectState {
    pub root: String,
    /// Seconds since 1970.
    pub last_used: u64,
    pub tabs: Vec<TabState>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

/// Kiln's per-user data folder (logs and state live here).
pub fn data_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/Kiln"))
    } else if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Kiln"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
            .map(|p| p.join("kiln"))
    }
}

fn state_path() -> Option<PathBuf> {
    // Tests and the snapshot tool point this elsewhere so they never touch the real file.
    if let Some(path) = std::env::var_os("KILN_STATE_FILE") {
        return Some(PathBuf::from(path));
    }
    data_dir().map(|d| d.join("state.json"))
}

/// The saved state (read from disk once).
pub fn current() -> State {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get_or_insert_with(|| state_path().map(|p| read(&p)).unwrap_or_default())
        .clone()
}

fn read(path: &Path) -> State {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|err| {
            log::warn!("ignoring {}: {err}", path.display());
            State::default()
        }),
        Err(_) => State::default(),
    }
}

/// Records a window's sessions, and each project's part of them, then writes the file if
/// anything changed. `project_of` maps a folder to its project root (None: not a project).
pub fn save_window(window: WindowState, project_of: impl Fn(&str) -> Option<String>) {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let state = guard.get_or_insert_with(|| state_path().map(|p| read(&p)).unwrap_or_default());
    let before = state.clone();
    merge(state, window, project_of, now());
    if *state != before
        && let Some(path) = state_path()
        && let Err(err) = write(&path, state)
    {
        log::warn!("could not save {}: {err}", path.display());
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn merge(
    state: &mut State,
    window: WindowState,
    project_of: impl Fn(&str) -> Option<String>,
    now: u64,
) {
    state.version = 1;
    // Each project's sessions, in window order. A session belongs to its focused pane's project.
    let mut projects: Vec<ProjectState> = Vec::new();
    for tab in &window.tabs {
        let Some(root) = tab
            .panes
            .get(tab.focused)
            .cloned()
            .flatten()
            .and_then(|cwd| project_of(&cwd))
        else {
            continue;
        };
        match projects.iter_mut().find(|p| p.root == root) {
            Some(project) => project.tabs.push(tab.clone()),
            None => projects.push(ProjectState {
                root,
                last_used: now,
                tabs: vec![tab.clone()],
            }),
        }
    }
    for project in projects {
        match state.projects.iter_mut().find(|p| p.root == project.root) {
            // Unchanged layouts keep their time, so an idle window does not rewrite the file.
            Some(existing) if existing.tabs == project.tabs => {}
            Some(existing) => *existing = project,
            None => state.projects.push(project),
        }
    }
    state
        .projects
        .sort_by_key(|p| std::cmp::Reverse(p.last_used));
    state.projects.truncate(KEEP_PROJECTS);
    state.window = Some(window);
}

/// Marks a project as just used (opened from the palette).
pub fn touch_project(root: &str) {
    let mut guard = STATE.lock().unwrap_or_else(|e| e.into_inner());
    let state = guard.get_or_insert_with(|| state_path().map(|p| read(&p)).unwrap_or_default());
    if let Some(project) = state.projects.iter_mut().find(|p| p.root == root) {
        project.last_used = now();
        state
            .projects
            .sort_by_key(|p| std::cmp::Reverse(p.last_used));
        if let Some(path) = state_path() {
            let _ = write(&path, state);
        }
    }
}

fn write(path: &Path, state: &State) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(state).map_err(io::Error::other)?;
    // Write next to it and rename, so a crash never leaves half a file.
    let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Folder names can be private: only the user may read the file.
        options.mode(0o600);
    }
    {
        use std::io::Write as _;
        let mut file = options.open(&tmp)?;
        file.write_all(text.as_bytes())?;
    }
    fs::rename(&tmp, path)
}

/// The colours a project can be given, by name. They come from the theme (mostly its ANSI
/// colours), so they suit whichever theme is on.
pub const PROJECT_COLOURS: [&str; 8] = [
    "red", "orange", "yellow", "green", "cyan", "blue", "magenta", "grey",
];

/// A project colour's value in palette `p`: a name from `PROJECT_COLOURS`, or "#rrggbb".
pub fn colour_value(colour: &str, p: &crate::theme::Palette) -> Option<u32> {
    Some(match colour.trim() {
        "red" => p.ansi[1],
        "orange" => p.accent,
        "yellow" => p.ansi[3],
        "green" => p.ansi[2],
        "cyan" => p.ansi[6],
        "blue" => p.ansi[4],
        "magenta" => p.ansi[5],
        "grey" => p.muted,
        hex => return crate::config::parse_hex(hex),
    })
}

/// The config's `[projects."…"]` table for the project at `root`, with its key.
pub fn project_config<'a>(
    root: &str,
    config: &'a crate::config::Config,
) -> Option<(&'a str, &'a crate::config::ProjectConfig)> {
    // The folder as written, else with its links resolved (Kiln's roots are resolved).
    let matches = |key: &str| {
        let path = expand_home(key);
        same_folder(&path, root)
            || fs::canonicalize(&path).is_ok_and(|real| same_folder(&real.to_string_lossy(), root))
    };
    config
        .projects
        .iter()
        .find(|(key, _)| matches(key))
        .map(|(key, project)| (key.as_str(), project))
}

/// The key a project gets in the config: the one it has, else its folder with "~" for home.
pub fn project_key(root: &str, config: &crate::config::Config) -> String {
    if let Some((key, _)) = project_config(root, config) {
        return key.to_string();
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    match root.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with(['/', '\\'])) => {
            format!("~{rest}")
        }
        _ => root.to_string(),
    }
}

/// The name the user gave the project, if any.
pub fn custom_project_name(root: &str, config: &crate::config::Config) -> Option<String> {
    project_config(root, config)
        .and_then(|(_, project)| project.name.as_deref())
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

/// The colour the user chose for the project (`None`: Auto).
pub fn chosen_project_color(root: &str, config: &crate::config::Config) -> Option<u32> {
    let colour = project_config(root, config)?.1.color.as_deref()?;
    colour_value(colour, &crate::theme::palette())
}

/// A project's colour: the one chosen, else picked from its path so it stays the same.
pub fn project_color(root: &str, config: &crate::config::Config) -> u32 {
    if let Some(color) = chosen_project_color(root, config) {
        return color;
    }
    // Muted colours from the theme; none of them is the accent.
    let p = crate::theme::palette();
    let colors: [u32; 8] = [
        p.ansi[12], p.ansi[14], p.ansi[13], p.modified, p.ok, p.ansi[7], p.ansi[6], p.ansi[5],
    ];
    // FNV-1a: stable across runs and Rust versions.
    let mut hash: u32 = 0x811c9dc5;
    for byte in root.as_bytes() {
        hash ^= *byte as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    colors[hash as usize % colors.len()]
}

fn expand_home(path: &str) -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    match path.strip_prefix('~') {
        Some(rest) if !home.is_empty() && (rest.is_empty() || rest.starts_with(['/', '\\'])) => {
            format!("{home}{rest}")
        }
        _ => path.to_string(),
    }
}

fn same_folder(a: &str, b: &str) -> bool {
    let trim = |s: &str| s.trim_end_matches(['/', '\\']).to_string();
    if cfg!(any(windows, target_os = "macos")) {
        // Case-insensitive file systems by default.
        trim(a).eq_ignore_ascii_case(&trim(b))
    } else {
        trim(a) == trim(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(panes: &[&str]) -> TabState {
        TabState {
            panes: panes.iter().map(|p| Some(p.to_string())).collect(),
            focused: 0,
            name: None,
        }
    }

    #[test]
    fn merges_sessions_into_projects() {
        let project_of = |cwd: &str| {
            cwd.strip_prefix("/code/")
                .map(|rest| format!("/code/{}", rest.split('/').next().unwrap()))
        };
        let mut state = State::default();
        let window = WindowState {
            sidebar: true,
            rail: false,
            active: 1,
            tabs: vec![
                tab(&["/code/a", "/code/a/src"]),
                tab(&["/tmp"]),
                tab(&["/code/b"]),
            ],
        };
        merge(&mut state, window.clone(), project_of, 100);
        let roots: Vec<_> = state.projects.iter().map(|p| p.root.as_str()).collect();
        assert_eq!(roots, ["/code/a", "/code/b"]);
        assert_eq!(state.projects[0].tabs[0].panes.len(), 2);

        // Same layout later: times stay, so nothing needs writing.
        let before = state.clone();
        merge(&mut state, window, project_of, 200);
        assert_eq!(state, before);

        // Project b changes: it moves to the front with the new time.
        let window = WindowState {
            tabs: vec![
                tab(&["/code/a", "/code/a/src"]),
                tab(&["/code/b", "/code/b"]),
            ],
            ..Default::default()
        };
        merge(&mut state, window, project_of, 300);
        assert_eq!(state.projects[0].root, "/code/b");
        assert_eq!(state.projects[0].last_used, 300);
    }

    #[test]
    fn round_trips_through_json_and_ignores_unknown_fields() {
        let state = State {
            version: 1,
            window: Some(WindowState {
                tabs: vec![tab(&["/x"])],
                ..Default::default()
            }),
            projects: Vec::new(),
        };
        let text = serde_json::to_string(&state).unwrap();
        assert_eq!(serde_json::from_str::<State>(&text).unwrap(), state);
        let newer = r#"{"version":2,"window":null,"projects":[],"later":true}"#;
        assert_eq!(serde_json::from_str::<State>(newer).unwrap().version, 2);
    }

    #[test]
    fn project_colours_are_stable_and_configurable() {
        let mut config = crate::config::Config::default();
        let a = project_color("/code/a", &config);
        assert_eq!(a, project_color("/code/a", &config));
        // "#rrggbb", as configs had it before the named colours.
        config.projects.insert(
            "/code/a/".into(),
            crate::config::ProjectConfig {
                color: Some("#123456".into()),
                ..Default::default()
            },
        );
        assert_eq!(project_color("/code/a", &config), 0x123456);
        // A colour by name follows the theme.
        let p = crate::theme::palette();
        for (name, value) in [("blue", p.ansi[4]), ("orange", p.accent), ("grey", p.muted)] {
            config.projects.get_mut("/code/a/").unwrap().color = Some(name.into());
            assert_eq!(project_color("/code/a", &config), value, "{name}");
            assert_eq!(chosen_project_color("/code/a", &config), Some(value));
        }
        for name in PROJECT_COLOURS {
            assert!(colour_value(name, &p).is_some(), "{name}");
        }
        // Anything else is Auto.
        config.projects.get_mut("/code/a/").unwrap().color = Some("pink".into());
        assert_eq!(project_color("/code/a", &config), a);
        assert_eq!(chosen_project_color("/code/a", &config), None);
    }

    #[test]
    fn projects_can_be_renamed_in_the_config() {
        let config = crate::config::parse(
            "[projects.\"/code/acme-web\"]\nname = \"Acme\"\n[projects.\"/code/blank\"]\nname = \"  \"\n",
        )
        .unwrap();
        assert_eq!(
            custom_project_name("/code/acme-web/", &config).as_deref(),
            Some("Acme")
        );
        assert_eq!(custom_project_name("/code/blank", &config), None);
        assert_eq!(custom_project_name("/code/other", &config), None);
        // A project keeps the key it has; a new one is written with "~" for home.
        assert_eq!(project_key("/code/acme-web", &config), "/code/acme-web");
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() {
            assert_eq!(
                project_key(&format!("{home}/src/app"), &config),
                "~/src/app"
            );
        }
    }

    #[test]
    fn session_names_are_saved_with_the_sessions() {
        let mut named = tab(&["/x"]);
        named.name = Some("deploys".into());
        let state = State {
            version: 1,
            window: Some(WindowState {
                tabs: vec![named.clone(), tab(&["/y"])],
                ..Default::default()
            }),
            projects: Vec::new(),
        };
        let text = serde_json::to_string(&state).unwrap();
        let back: State = serde_json::from_str(&text).unwrap();
        assert_eq!(
            back.window.unwrap().tabs[0].name.as_deref(),
            Some("deploys")
        );
        // Unnamed sessions write nothing for it, and older files read as unnamed.
        assert_eq!(text.matches("\"name\"").count(), 1, "{text}");
        let old: TabState = serde_json::from_str(r#"{"panes":["/x"],"focused":0}"#).unwrap();
        assert_eq!(old.name, None);
    }
}
