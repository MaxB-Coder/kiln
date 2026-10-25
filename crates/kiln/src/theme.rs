//! Kiln's palettes (the built-in themes; Kiln Dark is Gruvbox dark plus the orange accent) and
//! terminal colour resolution.

use gpui::{Hsla, Rgba};
use kiln_term::engine::term::color::Colors;
use kiln_term::engine::vte::ansi::{Color, NamedColor, Rgb};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub bg: u32,
    pub bar: u32,
    pub raised: u32,
    pub bg1: u32,
    pub bg2: u32,
    pub bg3: u32,
    pub muted: u32,
    pub fg: u32,
    pub fg_dim: u32,
    pub accent: u32,
    pub failed: u32,
    pub ok: u32,
    pub modified: u32,
    pub ansi: [u32; 16],
}

pub const DEFAULT_PALETTE: Palette = Palette {
    bg: 0x282828,
    bar: 0x1d2021,
    raised: 0x32302f,
    bg1: 0x3c3836,
    bg2: 0x504945,
    bg3: 0x665c54,
    muted: 0x928374,
    fg: 0xebdbb2,
    fg_dim: 0xbdae93,
    accent: 0xe07b39,
    failed: 0xb8645a,
    ok: 0x8f9a6c,
    modified: 0xc4a35e,
    ansi: [
        0x282828, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a,
        0xa89984, // normal
        0x928374, 0xfb4934, 0xb8bb26, 0xfabd2f, 0x83a598, 0xd3869b, 0x8ec07c,
        0xebdbb2, // bright
    ],
};

const fn preset(p: [u32; 13], ansi: [u32; 16]) -> Palette {
    Palette {
        bg: p[0],
        bar: p[1],
        raised: p[2],
        bg1: p[3],
        bg2: p[4],
        bg3: p[5],
        muted: p[6],
        fg: p[7],
        fg_dim: p[8],
        accent: p[9],
        failed: p[10],
        ok: p[11],
        modified: p[12],
        ansi,
    }
}

/// A built-in theme: its key in kiln.toml (`[theme] name`), its title, its palette.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Preset {
    pub id: &'static str,
    pub title: &'static str,
    pub palette: Palette,
}

/// The built-in themes. Kiln Dark is the default. Catppuccin Mocha
/// (github.com/catppuccin/catppuccin), Everforest (github.com/sainnhe/everforest), Nord
/// (github.com/nordtheme/nord) and Kanagawa (github.com/rebelot/kanagawa.nvim) are MIT-licensed
/// palettes, credited in
/// assets/themes/THEME-LICENSES.md (shipped in every package).
pub const PRESETS: [Preset; 6] = [
    Preset {
        id: "kiln-dark",
        title: "Kiln Dark",
        palette: DEFAULT_PALETTE,
    },
    Preset {
        id: "kiln-light",
        title: "Kiln Light",
        palette: preset(
            [
                0xf8f3ea, 0xefe7d9, 0xfffcf6, 0xe4d9c6, 0xd3c5ae, 0xa99a83, 0x8a7d6c, 0x3c3836,
                0x5f554b, 0xc4622a, 0xa8564c, 0x6b7a45, 0x9a7a32,
            ],
            [
                0xebe3d5, 0xcc241d, 0x98971a, 0xd79921, 0x458588, 0xb16286, 0x689d6a, 0x7c6f64,
                0x928374, 0x9d0006, 0x79740e, 0xb57614, 0x076678, 0x8f3f71, 0x427b58, 0x3c3836,
            ],
        ),
    },
    Preset {
        id: "catppuccin-mocha",
        title: "Catppuccin Mocha",
        palette: preset(
            [
                0x1e1e2e, 0x181825, 0x262637, 0x313244, 0x45475a, 0x585b70, 0x7f849c, 0xcdd6f4,
                0xa6adc8, 0xfab387, 0xeba0ac, 0xa6e3a1, 0xf9e2af,
            ],
            [
                0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de,
                0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
            ],
        ),
    },
    Preset {
        id: "everforest",
        title: "Everforest",
        palette: preset(
            [
                0x2d353b, 0x232a2e, 0x343f44, 0x3d484d, 0x475258, 0x56635f, 0x859289, 0xd3c6aa,
                0x9da9a0, 0xe69875, 0xe67e80, 0xa7c080, 0xdbbc7f,
            ],
            [
                0x475258, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
                0x56635f, 0xe67e80, 0xa7c080, 0xdbbc7f, 0x7fbbb3, 0xd699b6, 0x83c092, 0xd3c6aa,
            ],
        ),
    },
    Preset {
        id: "nord",
        title: "Nord",
        palette: preset(
            [
                0x2e3440, 0x272c36, 0x353b49, 0x3b4252, 0x434c5e, 0x4c566a, 0x616e88, 0xd8dee9,
                0xaeb7c6, 0xd08770, 0xbf616a, 0xa3be8c, 0xebcb8b,
            ],
            [
                0x3b4252, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x88c0d0, 0xe5e9f0,
                0x4c566a, 0xbf616a, 0xa3be8c, 0xebcb8b, 0x81a1c1, 0xb48ead, 0x8fbcbb, 0xeceff4,
            ],
        ),
    },
    Preset {
        id: "kanagawa",
        title: "Kanagawa",
        palette: preset(
            [
                0x1f1f28, 0x16161d, 0x2a2a37, 0x363646, 0x54546d, 0x54546d, 0x727169, 0xdcd7ba,
                0xc8c093, 0xffa066, 0xe46876, 0x98bb6c, 0xe6c384,
            ],
            [
                0x16161d, 0xc34043, 0x76946a, 0xc0a36e, 0x7e9cd8, 0x957fb8, 0x6a9589, 0xc8c093,
                0x727169, 0xe82424, 0x98bb6c, 0xe6c384, 0x7fb4ca, 0x938aa9, 0x7aa89f, 0xdcd7ba,
            ],
        ),
    },
];

pub fn preset_by_id(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// The theme names `[theme] name` takes: the presets, "system" (Kiln Dark or Light with the
/// OS) and "custom" (`base` plus the colours in the file).
pub const SYSTEM: &str = "system";
pub const CUSTOM: &str = "custom";

/// The preset a theme name stands for, given whether the system appearance is dark. Unknown
/// names are `None` (the caller warns and uses Kiln Dark).
pub fn resolve_name(name: &str, base: Option<&str>, system_dark: bool) -> Option<&'static Preset> {
    match name {
        SYSTEM => preset_by_id(if system_dark {
            "kiln-dark"
        } else {
            "kiln-light"
        }),
        CUSTOM => preset_by_id(base.unwrap_or("kiln-dark")),
        id => preset_by_id(id),
    }
}

static SYSTEM_DARK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Whether the OS appearance is dark (true where it cannot be told).
pub fn system_dark() -> bool {
    SYSTEM_DARK.load(std::sync::atomic::Ordering::Relaxed)
}

/// Records the OS appearance; returns whether it changed.
pub fn set_system_dark(dark: bool) -> bool {
    SYSTEM_DARK.swap(dark, std::sync::atomic::Ordering::Relaxed) != dark
}

/// Perceived brightness, 0 (black) to 1 (white).
pub fn luminance(hex: u32) -> f32 {
    let (r, g, b) = ((hex >> 16) & 255, (hex >> 8) & 255, hex & 255);
    (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.
}

impl Palette {
    /// A light theme (by its background).
    pub fn is_light(&self) -> bool {
        luminance(self.bg) > 0.5
    }

    /// The veil behind pop-ups: warm and light on light themes, near black on dark ones.
    pub fn veil(&self) -> Hsla {
        if self.is_light() {
            gpui::rgba(0x3c2d1e47).into()
        } else {
            gpui::rgba(0x0a0a0a99).into()
        }
    }

    /// A pop-up's drop shadow.
    pub fn shadow(&self) -> Hsla {
        if self.is_light() {
            gpui::rgba(0x3c2d1e2e).into()
        } else {
            gpui::rgba(0x00000073).into()
        }
    }
}

static CURRENT: std::sync::RwLock<Palette> = std::sync::RwLock::new(DEFAULT_PALETTE);

/// The palette in use (the default, or the config file's).
pub fn palette() -> Palette {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

pub fn set_palette(palette: Palette) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = palette;
}

pub fn hsla(hex: u32) -> Hsla {
    gpui::rgb(hex).into()
}

pub fn rgb_to_hsla(rgb: Rgb) -> Hsla {
    Rgba {
        r: rgb.r as f32 / 255.,
        g: rgb.g as f32 / 255.,
        b: rgb.b as f32 / 255.,
        a: 1.,
    }
    .into()
}

/// `base` with `amount` (0–1) of `over` mixed in.
pub fn blend(base: u32, over: u32, amount: f32) -> u32 {
    let channel = |shift: u32| {
        let a = ((base >> shift) & 0xff) as f32;
        let b = ((over >> shift) & 0xff) as f32;
        ((a + (b - a) * amount).round() as u32).min(255) << shift
    };
    channel(16) | channel(8) | channel(0)
}

pub fn rgb_from_hex(hex: u32) -> Rgb {
    Rgb {
        r: (hex >> 16) as u8,
        g: (hex >> 8) as u8,
        b: hex as u8,
    }
}

/// Resolves an engine colour, honouring palette changes programs made with OSC 4/10/11.
pub fn resolve(color: Color, overrides: &Colors) -> Rgb {
    match color {
        Color::Spec(rgb) => rgb,
        Color::Indexed(index) => overrides[index as usize].unwrap_or_else(|| indexed(index)),
        Color::Named(name) => overrides[name as usize].unwrap_or_else(|| named(name)),
    }
}

/// The colour for a palette index when no program overrode it.
pub fn indexed(index: u8) -> Rgb {
    match index {
        0..=15 => rgb_from_hex(palette().ansi[index as usize]),
        // 6×6×6 colour cube.
        16..=231 => {
            let i = index - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Rgb {
                r: level(i / 36),
                g: level((i / 6) % 6),
                b: level(i % 6),
            }
        }
        // Grey ramp.
        232..=255 => {
            let v = 8 + (index - 232) * 10;
            Rgb { r: v, g: v, b: v }
        }
    }
}

fn named(name: NamedColor) -> Rgb {
    use NamedColor::*;
    match name {
        Foreground | BrightForeground => rgb_from_hex(palette().fg),
        Background => rgb_from_hex(palette().bg),
        Cursor => rgb_from_hex(palette().accent),
        DimForeground => rgb_from_hex(palette().fg_dim),
        DimBlack | DimRed | DimGreen | DimYellow | DimBlue | DimMagenta | DimCyan | DimWhite => {
            dim(indexed((name as usize - DimBlack as usize) as u8))
        }
        other => indexed(other as u8),
    }
}

/// Dims a colour the way alacritty does (two thirds brightness).
pub fn dim(rgb: Rgb) -> Rgb {
    let f = |v: u8| (v as f32 * 0.66) as u8;
    Rgb {
        r: f(rgb.r),
        g: f(rgb.g),
        b: f(rgb.b),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_is_complete_and_distinct() {
        assert_eq!(PRESETS[0].palette, DEFAULT_PALETTE);
        for (i, preset) in PRESETS.iter().enumerate() {
            assert_eq!(preset.palette.ansi.len(), 16);
            assert!(
                preset.palette.ansi.iter().all(|c| *c <= 0xffffff),
                "{}",
                preset.id
            );
            assert_eq!(preset_by_id(preset.id), Some(preset));
            for other in &PRESETS[i + 1..] {
                assert_ne!(preset.palette, other.palette, "{} {}", preset.id, other.id);
            }
        }
        assert!(preset_by_id("kiln-light").unwrap().palette.is_light());
        assert!(PRESETS.iter().filter(|p| p.palette.is_light()).count() == 1);
    }

    #[test]
    fn system_follows_the_appearance_and_custom_its_base() {
        assert_eq!(resolve_name(SYSTEM, None, true).unwrap().id, "kiln-dark");
        assert_eq!(resolve_name(SYSTEM, None, false).unwrap().id, "kiln-light");
        assert_eq!(resolve_name(CUSTOM, Some("nord"), true).unwrap().id, "nord");
        assert_eq!(resolve_name(CUSTOM, None, false).unwrap().id, "kiln-dark");
        assert_eq!(
            resolve_name("everforest", None, false).unwrap().id,
            "everforest"
        );
        // Tokyo Night was removed: a config naming it gets the fallback.
        assert!(resolve_name("tokyo-night", None, true).is_none());
        assert!(resolve_name("solarized", None, true).is_none());
    }
}
