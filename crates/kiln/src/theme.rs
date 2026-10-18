//! Kiln's palette (Gruvbox dark plus the orange accent) and terminal colour resolution.
//!
//! Same values as ~/dotfiles/home/dot_config/wezterm/palette.lua.

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
