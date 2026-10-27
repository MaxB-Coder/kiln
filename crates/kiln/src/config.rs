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
    /// The app icon's design (the Dock on macOS, the window icon on Windows and Linux).
    pub icon: IconKey,
    /// Each design's colour; a design not listed is terracotta.
    pub icon_colours: IconColours,
    /// The custom icon: Kiln's own PNG copy of the chosen image (see `custom_icon`).
    pub icon_path: Option<PathBuf>,
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

/// The app icon's designs (assets/icon/variants, from scripts/make-icon-masters.py), and
/// Custom, an image of the user's.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IconDesign {
    /// The kiln's door with the prompt's flame; the icon the app ships with.
    #[default]
    Kiln,
    Flame,
    Beehive,
    Line,
    Custom,
}

impl IconDesign {
    /// The drawn designs, in the Settings row's order (Custom comes last).
    pub const DRAWN: [IconDesign; 4] = [
        IconDesign::Kiln,
        IconDesign::Flame,
        IconDesign::Beehive,
        IconDesign::Line,
    ];
    pub const ALL: [IconDesign; 5] = [
        IconDesign::Kiln,
        IconDesign::Flame,
        IconDesign::Beehive,
        IconDesign::Line,
        IconDesign::Custom,
    ];

    /// The value in kiln.toml.
    pub fn key(self) -> &'static str {
        match self {
            IconDesign::Kiln => "kiln",
            IconDesign::Flame => "flame",
            IconDesign::Beehive => "beehive",
            IconDesign::Line => "line",
            IconDesign::Custom => "custom",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            IconDesign::Kiln => "Kiln",
            IconDesign::Flame => "Flame",
            IconDesign::Beehive => "Beehive",
            IconDesign::Line => "Line",
            IconDesign::Custom => "Custom",
        }
    }

    pub fn from_key(key: &str) -> Option<IconDesign> {
        Self::ALL.into_iter().find(|d| d.key() == key)
    }
}

/// The colours every drawn design comes in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum IconColour {
    #[default]
    Terracotta,
    Black,
    White,
    Ember,
    Slate,
    Sage,
    Sand,
}

impl IconColour {
    pub const ALL: [IconColour; 7] = [
        IconColour::Terracotta,
        IconColour::Black,
        IconColour::White,
        IconColour::Ember,
        IconColour::Slate,
        IconColour::Sage,
        IconColour::Sand,
    ];

    /// The value in kiln.toml.
    pub fn key(self) -> &'static str {
        match self {
            IconColour::Terracotta => "terracotta",
            IconColour::Black => "black",
            IconColour::White => "white",
            IconColour::Ember => "ember",
            IconColour::Slate => "slate",
            IconColour::Sage => "sage",
            IconColour::Sand => "sand",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            IconColour::Terracotta => "Terracotta",
            IconColour::Black => "Black",
            IconColour::White => "White",
            IconColour::Ember => "Ember",
            IconColour::Slate => "Slate",
            IconColour::Sage => "Sage",
            IconColour::Sand => "Sand",
        }
    }

    /// The tile's gradient, top and bottom (the swatches in Settings).
    pub fn swatch(self) -> (u32, u32) {
        match self {
            IconColour::Terracotta => (0xee8a48, 0xc25b22),
            IconColour::Black => (0x3d3834, 0x161413),
            IconColour::White => (0xf8f3ec, 0xe1d5c4),
            IconColour::Ember => (0xb0473a, 0x6e2219),
            IconColour::Slate => (0x5d6b78, 0x323b45),
            IconColour::Sage => (0x8a9a78, 0x55654a),
            IconColour::Sand => (0xe3c89a, 0xbf9a62),
        }
    }

    pub fn from_key(key: &str) -> Option<IconColour> {
        Self::ALL.into_iter().find(|c| c.key() == key)
    }
}

impl<'de> Deserialize<'de> for IconColour {
    /// An unknown colour warns and is terracotta.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let key = String::deserialize(d)?;
        Ok(IconColour::from_key(&key).unwrap_or_else(|| {
            log::warn!("unknown icon colour {key:?}; using terracotta");
            IconColour::default()
        }))
    }
}

/// `icon` in kiln.toml: a design, or (from before the designs) a colour, which means the Kiln
/// design in that colour. Settings rewrites the old form the first time it writes the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IconKey {
    pub design: IconDesign,
    /// `icon = "black"` and the like.
    pub old_colour: Option<IconColour>,
}

impl<'de> Deserialize<'de> for IconKey {
    /// An unknown value warns and is the Kiln design.
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let key = String::deserialize(d)?;
        if let Some(design) = IconDesign::from_key(&key) {
            return Ok(IconKey {
                design,
                old_colour: None,
            });
        }
        if let Some(colour) = IconColour::from_key(&key) {
            return Ok(IconKey {
                design: IconDesign::Kiln,
                old_colour: Some(colour),
            });
        }
        log::warn!("unknown icon {key:?}; using kiln");
        Ok(IconKey::default())
    }
}

/// `[icon_colours]`: each drawn design's colour.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct IconColours {
    pub kiln: Option<IconColour>,
    pub flame: Option<IconColour>,
    pub beehive: Option<IconColour>,
    pub line: Option<IconColour>,
}

impl IconColours {
    pub fn get(&self, design: IconDesign) -> Option<IconColour> {
        match design {
            IconDesign::Kiln => self.kiln,
            IconDesign::Flame => self.flame,
            IconDesign::Beehive => self.beehive,
            IconDesign::Line => self.line,
            IconDesign::Custom => None,
        }
    }
}

/// The icon to show: a drawn design in a colour, or the custom image.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AppIcon {
    Drawn(IconDesign, IconColour),
    Custom(PathBuf),
}

impl Default for AppIcon {
    fn default() -> Self {
        AppIcon::Drawn(IconDesign::Kiln, IconColour::Terracotta)
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
    /// The project's colour (its sidebar dot and header pill): a colour of the theme by name
    /// (`state::PROJECT_COLOURS`), or "#rrggbb".
    pub color: Option<String>,
    /// The name Kiln shows for the project instead of its folder's.
    pub name: Option<String>,
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

/// The theme: a built-in one by name, or "custom" from `[theme.custom]`. Colour keys right in
/// `[theme]` still apply on top of whichever theme (as they did before themes had names); a
/// `[theme]` with colours and no name is Custom.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ThemeConfig {
    /// "kiln-dark" (the default), "kiln-light", "system", "catppuccin-mocha", "everforest",
    /// "nord", "kanagawa" or "custom".
    pub name: Option<String>,
    /// Where an older Custom theme started (now `[theme.custom] base`).
    pub base: Option<String>,
    #[serde(flatten)]
    pub colours: ThemeColours,
    /// The custom theme, kept while another one is chosen.
    pub custom: Option<CustomTheme>,
}

/// `[theme.custom]`: a preset to start from and the colours that differ.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct CustomTheme {
    /// The theme it starts from (default "kiln-dark").
    pub base: Option<String>,
    #[serde(flatten)]
    pub colours: ThemeColours,
}

/// Palette colours as "#rrggbb" strings; keys match the palette's names.
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct ThemeColours {
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

impl ThemeColours {
    /// Whether any colour is set.
    pub fn any(&self) -> bool {
        self != &ThemeColours::default()
    }

    /// Sets the palette's colours from these; bad ones are logged and skipped.
    fn apply(&self, p: &mut Palette, table: &str) {
        let set = |slot: &mut u32, value: &Option<String>, name: &str| {
            if let Some(value) = value {
                match parse_hex(value) {
                    Some(hex) => *slot = hex,
                    None => {
                        log::warn!("config: {table}.{name} = {value:?} is not a #rrggbb colour")
                    }
                }
            }
        };
        set(&mut p.bg, &self.bg, "bg");
        set(&mut p.bar, &self.bar, "bar");
        set(&mut p.raised, &self.raised, "raised");
        set(&mut p.bg1, &self.bg1, "bg1");
        set(&mut p.bg2, &self.bg2, "bg2");
        set(&mut p.bg3, &self.bg3, "bg3");
        set(&mut p.muted, &self.muted, "muted");
        set(&mut p.fg, &self.fg, "fg");
        set(&mut p.fg_dim, &self.fg_dim, "fg_dim");
        set(&mut p.accent, &self.accent, "accent");
        set(&mut p.failed, &self.failed, "failed");
        set(&mut p.ok, &self.ok, "ok");
        set(&mut p.modified, &self.modified, "modified");
        if let Some(ansi) = &self.ansi {
            for (i, value) in ansi.iter().take(16).enumerate() {
                set(&mut p.ansi[i], &Some(value.clone()), "ansi");
            }
        }
    }
}

impl ThemeConfig {
    /// The theme in effect: its name, or "custom" for colours without one (an older config).
    pub fn effective_name(&self) -> &str {
        match self.name.as_deref() {
            Some(name) => name,
            None if self.colours.any() => crate::theme::CUSTOM,
            None => "kiln-dark",
        }
    }

    /// The preset the custom theme starts from.
    pub fn custom_base(&self) -> &str {
        self.custom
            .as_ref()
            .and_then(|c| c.base.as_deref())
            .or(self.base.as_deref())
            .unwrap_or("kiln-dark")
    }

    /// Whether there is a custom theme to go back to (`[theme.custom]`, or older colours).
    pub fn has_custom(&self) -> bool {
        self.custom.is_some() || self.colours.any()
    }

    /// The custom theme's palette, whether or not it is the one in effect.
    pub fn custom_palette(&self) -> Palette {
        let mut p = crate::theme::preset_by_id(self.custom_base())
            .map_or(DEFAULT_PALETTE, |preset| preset.palette);
        if let Some(custom) = &self.custom {
            custom.colours.apply(&mut p, "theme.custom");
        }
        self.colours.apply(&mut p, "theme");
        p
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            icon: IconKey::default(),
            icon_colours: IconColours::default(),
            icon_path: None,
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
    /// The colour `design` shows in: its own, or terracotta.
    pub fn icon_colour(&self, design: IconDesign) -> IconColour {
        self.icon_colours
            .get(design)
            .or(self.icon.old_colour.filter(|_| design == IconDesign::Kiln))
            .unwrap_or_default()
    }

    /// The icon in use. Custom without an image is the Kiln design.
    pub fn app_icon(&self) -> AppIcon {
        match (self.icon.design, &self.icon_path) {
            (IconDesign::Custom, Some(path)) => AppIcon::Custom(path.clone()),
            (IconDesign::Custom, None) => {
                AppIcon::Drawn(IconDesign::Kiln, self.icon_colour(IconDesign::Kiln))
            }
            (design, _) => AppIcon::Drawn(design, self.icon_colour(design)),
        }
    }

    /// The palette: the theme's preset (for "system", the one that suits the OS appearance)
    /// with this config's colour overrides on top. Unknown names and bad colours are logged
    /// and ignored.
    pub fn palette(&self) -> Palette {
        self.palette_for(crate::theme::system_dark())
    }

    pub fn palette_for(&self, system_dark: bool) -> Palette {
        let t = &self.theme;
        let name = t.effective_name();
        if name == crate::theme::CUSTOM {
            return t.custom_palette();
        }
        let mut p = match crate::theme::resolve_name(name, None, system_dark) {
            Some(preset) => preset.palette,
            None => {
                log::warn!("config: theme {name:?} is not a theme Kiln has");
                DEFAULT_PALETTE
            }
        };
        t.colours.apply(&mut p, "theme");
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

# The app icon: "kiln", "flame", "beehive", "line", or "custom" (an image, chosen in Settings).
icon = "kiln"
# Each design's colour: "terracotta", "black", "white", "ember", "slate", "sage" or "sand".
# [icon_colours]
# kiln = "terracotta"
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

# Per project, keyed by folder (the sidebar sets these: right-click a project, or ⌘⇧R and
# "Project colour" in the palette). Colours: red, vermilion, orange, amber, yellow, lime,
# green, teal, cyan, sky, blue, indigo, violet, magenta, pink or grey, or "#rrggbb":
# [projects."~/developer/kiln"]
# name = "Kiln"
# color = "orange"

[theme]
# "kiln-dark", "kiln-light", "system" (dark or light with your OS), "catppuccin-mocha",
# "everforest", "nord", "kanagawa", or "custom" (the colours in [theme.custom] below).
name = "kiln-dark"
# Any palette colour, as "#rrggbb", on top of the theme. For a theme of your own, put them in
# [theme.custom] with the preset it starts from (`base = "kiln-dark"`) and set name = "custom":
# Settings keeps it there while you try other themes.
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
        // The template names the default theme; no name means the same.
        let mut defaults = Config::default();
        defaults.theme.name = Some("kiln-dark".into());
        assert_eq!(parse(TEMPLATE).unwrap(), defaults);
    }

    #[test]
    fn the_icon_keys_read_each_design_and_colour() {
        assert_eq!(Config::default().app_icon(), AppIcon::default());
        for design in IconDesign::DRAWN {
            for colour in IconColour::ALL {
                let text = format!(
                    "icon = \"{}\"\n[icon_colours]\n{} = \"{}\"\n",
                    design.key(),
                    design.key(),
                    colour.key()
                );
                assert_eq!(
                    parse(&text).unwrap().app_icon(),
                    AppIcon::Drawn(design, colour)
                );
            }
        }
        // Each design keeps its own colour; one not listed is terracotta.
        let config = parse("icon = \"beehive\"\n[icon_colours]\nkiln = \"sand\"\n").unwrap();
        assert_eq!(config.icon_colour(IconDesign::Kiln), IconColour::Sand);
        assert_eq!(
            config.app_icon(),
            AppIcon::Drawn(IconDesign::Beehive, IconColour::Terracotta)
        );
        // Custom: the image, or the Kiln design while there is none.
        let custom = parse("icon = \"custom\"\nicon_path = \"/x/i.png\"\n").unwrap();
        assert_eq!(custom.app_icon(), AppIcon::Custom("/x/i.png".into()));
        assert_eq!(
            parse("icon = \"custom\"\n").unwrap().app_icon(),
            AppIcon::default()
        );
        // Unknown values warn and fall back.
        assert_eq!(
            parse("icon = \"pink\"\n").unwrap().app_icon(),
            AppIcon::default()
        );
        let odd = parse("icon = \"line\"\n[icon_colours]\nline = \"pink\"\n").unwrap();
        assert_eq!(
            odd.app_icon(),
            AppIcon::Drawn(IconDesign::Line, IconColour::Terracotta)
        );
    }

    #[test]
    fn icons_from_before_the_designs_are_the_kiln_design() {
        for colour in IconColour::ALL {
            let config = parse(&format!("icon = \"{}\"\n", colour.key())).unwrap();
            assert_eq!(config.icon.design, IconDesign::Kiln);
            assert_eq!(config.icon.old_colour, Some(colour));
            assert_eq!(config.app_icon(), AppIcon::Drawn(IconDesign::Kiln, colour));
        }
        // A colour set for Kiln wins over the old key.
        let both = parse("icon = \"black\"\n[icon_colours]\nkiln = \"sage\"\n").unwrap();
        assert_eq!(both.icon_colour(IconDesign::Kiln), IconColour::Sage);
    }

    #[test]
    fn themes_resolve_by_name_with_overrides_on_top() {
        let palette = |text: &str, dark: bool| parse(text).unwrap().palette_for(dark);
        // An old config: overrides, no name, is Kiln Dark with them, as before.
        let old = palette("[theme]\naccent = \"#ff0000\"\n", true);
        assert_eq!(
            old,
            Palette {
                accent: 0xff0000,
                ..DEFAULT_PALETTE
            }
        );
        let nord = crate::theme::preset_by_id("nord").unwrap().palette;
        assert_eq!(palette("[theme]\nname = \"nord\"\n", true), nord);
        assert_eq!(
            palette("[theme]\nname = \"nord\"\nfg = \"#010203\"\n", true),
            Palette {
                fg: 0x010203,
                ..nord
            }
        );
        let light = crate::theme::preset_by_id("kiln-light").unwrap().palette;
        assert_eq!(palette("[theme]\nname = \"system\"\n", false), light);
        assert_eq!(
            palette("[theme]\nname = \"system\"\n", true),
            DEFAULT_PALETTE
        );
        // Custom: its own table, on top of its base.
        let custom = "[theme.custom]\nbase = \"kiln-light\"\nbg = \"#ffffff\"\n";
        assert_eq!(
            palette(&format!("[theme]\nname = \"custom\"\n{custom}"), true),
            Palette {
                bg: 0xffffff,
                ..light
            }
        );
        // A preset ignores the custom table.
        assert_eq!(
            palette(&format!("[theme]\nname = \"nord\"\n{custom}"), true),
            nord
        );
        // Custom written before `[theme.custom]`: base and colours right in `[theme]`.
        assert_eq!(
            palette(
                "[theme]\nname = \"custom\"\nbase = \"kiln-light\"\nbg = \"#ffffff\"\n",
                true
            ),
            Palette {
                bg: 0xffffff,
                ..light
            }
        );
        // Unknown names fall back to Kiln Dark.
        assert_eq!(
            palette("[theme]\nname = \"solarized\"\n", false),
            DEFAULT_PALETTE
        );
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
