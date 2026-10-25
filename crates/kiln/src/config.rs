//! The config file: `~/.config/kiln/kiln.toml` (macOS, Linux; `$XDG_CONFIG_HOME` respected) or
//! `%APPDATA%\Kiln\kiln.toml` (Windows). Everything is optional; the file is reloaded when it
//! changes.

use std::path::PathBuf;
use std::sync::RwLock;
use std::time::SystemTime;

use serde::Deserialize;

use crate::theme::{DEFAULT_PALETTE, Palette};

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// The app icon (the Dock on macOS, the window icon on Windows and Linux).
    pub icon: AppIcon,
    /// Font size in points.
    pub font_size: f32,
    /// Lines of scrollback per running command.
    pub scrollback: usize,
    pub cursor_blink: bool,
    /// macOS: Option sends Esc+key (like Alt elsewhere) instead of composing characters.
    pub option_as_alt: bool,
    /// Commands running at least this long notify you when they finish out of sight.
    pub notify_after_secs: u64,
    pub shell: ShellConfig,
    pub window: WindowConfig,
    pub theme: ThemeConfig,
    pub tints: TintConfig,
    pub updates: UpdatesConfig,
    /// Per-project settings, keyed by the project's folder ("~/code/app" works).
    pub projects: std::collections::BTreeMap<String, ProjectConfig>,
}

/// The app icon's variants (assets/icon/variants).
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AppIcon {
    /// Fired clay; the icon the app ships with.
    #[default]
    Terracotta,
    Black,
    White,
}

impl AppIcon {
    pub const ALL: [AppIcon; 3] = [AppIcon::Terracotta, AppIcon::Black, AppIcon::White];

    /// The value in kiln.toml.
    pub fn key(self) -> &'static str {
        match self {
            AppIcon::Terracotta => "terracotta",
            AppIcon::Black => "black",
            AppIcon::White => "white",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            AppIcon::Terracotta => "Terracotta",
            AppIcon::Black => "Black",
            AppIcon::White => "White",
        }
    }
}

/// Checking for updates: Kiln's only network use.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct UpdatesConfig {
    /// Check once a day (release builds installed from a Kiln installer only).
    pub check: bool,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self { check: true }
    }
}

/// Background tints for panes connected to other machines.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct TintConfig {
    /// Tint panes running ssh (or mosh) to another machine.
    pub remote: bool,
    /// Hosts that get the production tint (red). `*` matches anything; case-insensitive.
    pub production: Vec<String>,
}

impl Default for TintConfig {
    fn default() -> Self {
        Self {
            remote: true,
            production: vec!["*prod*".into()],
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ProjectConfig {
    /// The project's colour in the sidebar, as "#rrggbb".
    pub color: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ShellConfig {
    /// Program to run; empty means the login shell (PowerShell on Windows).
    pub program: Option<String>,
    pub args: Option<Vec<String>>,
    /// Load Kiln's shell integration (needed for blocks).
    pub integration: Option<bool>,
    /// Kiln's flame prompt, or the shell's own.
    pub prompt: PromptStyle,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PromptStyle {
    /// A small flame Kiln draws in the input box (orange; red after a failed command).
    #[default]
    Kiln,
    /// Whatever the shell's startup files set (Starship, a theme, PS1).
    Shell,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default)]
pub struct WindowConfig {
    pub columns: usize,
    pub rows: usize,
    pub sidebar: bool,
    /// Reopen the last window's sessions (folders and splits) at launch.
    pub restore: bool,
}

/// Palette overrides, as "#rrggbb" strings. Keys match the palette's names.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ThemeConfig {
    pub bg: Option<String>,
    pub bar: Option<String>,
    pub raised: Option<String>,
    pub bg1: Option<String>,
    pub bg2: Option<String>,
    pub bg3: Option<String>,
    pub muted: Option<String>,
    pub fg: Option<String>,
    pub fg_dim: Option<String>,
    pub accent: Option<String>,
    pub failed: Option<String>,
    pub ok: Option<String>,
    pub modified: Option<String>,
    /// The 16 terminal colours.
    pub ansi: Option<Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            icon: AppIcon::default(),
            font_size: 13.,
            scrollback: 10_000,
            cursor_blink: true,
            option_as_alt: false,
            notify_after_secs: 10,
            shell: ShellConfig::default(),
            window: WindowConfig::default(),
            theme: ThemeConfig::default(),
            tints: TintConfig::default(),
            updates: UpdatesConfig::default(),
            projects: Default::default(),
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            columns: 120,
            rows: 36,
            sidebar: true,
            restore: true,
        }
    }
}

impl Config {
    /// The palette with this config's overrides applied. Bad colours are ignored (and logged).
    pub fn palette(&self) -> Palette {
        let mut p = DEFAULT_PALETTE;
        let t = &self.theme;
        let set = |slot: &mut u32, value: &Option<String>, name: &str| {
            if let Some(value) = value {
                match parse_hex(value) {
                    Some(hex) => *slot = hex,
                    None => log::warn!("config: theme.{name} = {value:?} is not a #rrggbb colour"),
                }
            }
        };
        set(&mut p.bg, &t.bg, "bg");
        set(&mut p.bar, &t.bar, "bar");
        set(&mut p.raised, &t.raised, "raised");
        set(&mut p.bg1, &t.bg1, "bg1");
        set(&mut p.bg2, &t.bg2, "bg2");
        set(&mut p.bg3, &t.bg3, "bg3");
        set(&mut p.muted, &t.muted, "muted");
        set(&mut p.fg, &t.fg, "fg");
        set(&mut p.fg_dim, &t.fg_dim, "fg_dim");
        set(&mut p.accent, &t.accent, "accent");
        set(&mut p.failed, &t.failed, "failed");
        set(&mut p.ok, &t.ok, "ok");
        set(&mut p.modified, &t.modified, "modified");
        if let Some(ansi) = &t.ansi {
            for (i, value) in ansi.iter().take(16).enumerate() {
                set(&mut p.ansi[i], &Some(value.clone()), "ansi");
            }
        }
        p
    }
}

pub fn parse_hex(value: &str) -> Option<u32> {
    let hex = value.strip_prefix('#')?;
    (hex.len() == 6)
        .then(|| u32::from_str_radix(hex, 16).ok())
        .flatten()
}

/// Whether `host` matches one of the glob patterns (`*` matches anything; case-insensitive).
pub fn host_matches(patterns: &[String], host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    patterns
        .iter()
        .any(|pattern| glob(&pattern.to_ascii_lowercase(), &host))
}

fn glob(pattern: &str, text: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == text,
        Some((head, rest)) => {
            let Some(text) = text.strip_prefix(head) else {
                return false;
            };
            if rest.is_empty() {
                return true;
            }
            (0..=text.len())
                .filter(|&i| text.is_char_boundary(i))
                .any(|i| glob(rest, &text[i..]))
        }
    }
}

pub fn config_path() -> Option<PathBuf> {
    if cfg!(windows) {
        return std::env::var_os("APPDATA")
            .map(|p| PathBuf::from(p).join("Kiln").join("kiln.toml"));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("kiln").join("kiln.toml"))
}

/// Reads the config file. A missing file is the default config; a broken one is logged and
/// also gives the default, so Kiln always starts.
pub fn load() -> (Config, Option<SystemTime>) {
    let Some(path) = config_path() else {
        return (Config::default(), None);
    };
    let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return (Config::default(), modified);
    };
    match parse(&text) {
        Ok(config) => (config, modified),
        Err(err) => {
            log::error!("config {}: {err}", path.display());
            (Config::default(), modified)
        }
    }
}

pub fn parse(text: &str) -> Result<Config, toml::de::Error> {
    toml::from_str(text)
}

static CURRENT: RwLock<Option<Config>> = RwLock::new(None);

pub fn current() -> Config {
    CURRENT
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
        .unwrap_or_default()
}

pub fn set_current(config: Config) {
    crate::theme::set_palette(config.palette());
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = Some(config);
}

/// The commented template written when the user opens the config for the first time.
pub const TEMPLATE: &str = r##"# Kiln config. Every setting is optional; delete what you don't change.
# Kiln reloads this file when you save it. Settings (⌘, or Ctrl+Shift+,) changes the common
# ones here too, keeping your comments.

# The app icon: "terracotta", "black" or "white".
icon = "terracotta"
font_size = 13
scrollback = 10000
cursor_blink = true
# macOS only: Option sends Esc+key instead of composing characters like é.
option_as_alt = false
# Commands that run at least this long notify you when they finish while you look elsewhere
# (0 turns it off).
notify_after_secs = 10

[shell]
# program = "/bin/zsh"
# args = ["-l"]
# Shell integration draws each command as a block. Turn off for a classic terminal.
# integration = true
# The prompt in the input box: "kiln" (a small flame) or "shell" (your shell's own prompt, such
# as Starship). Only inside Kiln; your shell elsewhere is unchanged.
prompt = "kiln"

[window]
columns = 120
rows = 36
sidebar = true
# Reopen the last window's sessions (their folders and splits) at launch.
restore = true

[tints]
# Panes running ssh or mosh to another machine get a blue tint.
remote = true
# Hosts matching these get a red tint instead ("*" matches anything).
production = ["*prod*"]

[updates]
# Check for a new version once a day: the only time Kiln uses the network. Updates are signed,
# and install only when you choose to.
check = true

# Per project, keyed by folder:
# [projects."~/developer/kiln"]
# color = "#e07b39"

[theme]
# Any palette colour, as "#rrggbb":
# bg = "#282828"
# bar = "#1d2021"
# fg = "#ebdbb2"
# fg_dim = "#bdae93"
# muted = "#928374"
# accent = "#e07b39"
# failed = "#b8645a"
# ok = "#8f9a6c"
# modified = "#c4a35e"
# ansi = ["#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
#         "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2"]
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn template_parses_to_the_defaults() {
        assert_eq!(parse(TEMPLATE).unwrap(), Config::default());
    }

    #[test]
    fn the_icon_key_reads_each_variant() {
        assert_eq!(Config::default().icon, AppIcon::Terracotta);
        for icon in AppIcon::ALL {
            let config = parse(&format!("icon = \"{}\"\n", icon.key())).unwrap();
            assert_eq!(config.icon, icon);
        }
        assert!(parse("icon = \"pink\"\n").is_err());
    }

    #[test]
    fn production_patterns_match_hosts() {
        let tints = TintConfig::default();
        assert!(host_matches(&tints.production, "db.prod.example.com"));
        assert!(host_matches(&tints.production, "PROD-1"));
        assert!(!host_matches(&tints.production, "staging.example.com"));
        assert!(host_matches(&["*.internal".into()], "a.b.internal"));
        assert!(!host_matches(&["*.internal".into()], "internal.com"));
    }

    #[test]
    fn theme_overrides_apply_and_bad_values_are_ignored() {
        let config =
            parse("font_size = 14\n[theme]\naccent = \"#ff0000\"\nfg = \"red\"\n").unwrap();
        assert_eq!(config.font_size, 14.);
        let p = config.palette();
        assert_eq!(p.accent, 0xff0000);
        assert_eq!(p.fg, DEFAULT_PALETTE.fg);
    }
}
