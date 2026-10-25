//! The Theme section of Settings: the colour keys and their labels, the small previews drawn
//! from each theme's own palette, and the colour arithmetic behind the picker.

use gpui::{Bounds, PathBuilder, Pixels, Window, point, px};

use crate::config::ThemeColours;
use crate::theme::{self, Palette};

/// The interface colours, as kiln.toml names them, with their labels in Settings.
pub const COLOURS: [(&str, &str); 13] = [
    ("bg", "Background"),
    ("bar", "Sidebar & header"),
    ("raised", "Pop-ups & input"),
    ("bg1", "Borders & selection"),
    ("bg2", "Dividers"),
    ("bg3", "Faint text"),
    ("muted", "Muted text"),
    ("fg", "Text"),
    ("fg_dim", "Dim text"),
    ("accent", "Accent & flame"),
    ("failed", "Failed"),
    ("ok", "Succeeded"),
    ("modified", "Changed"),
];

/// The theme cards, in order: the name in kiln.toml and the card's title.
pub const CARDS: [(&str, &str); 7] = [
    (theme::SYSTEM, "Follow system"),
    ("kiln-dark", "Kiln Dark"),
    ("kiln-light", "Kiln Light"),
    ("catppuccin-mocha", "Catppuccin Mocha"),
    ("tokyo-night", "Tokyo Night"),
    ("nord", "Nord"),
    (theme::CUSTOM, "Custom"),
];

/// The interface colour `i` (an index into `COLOURS`).
pub fn slot(p: &Palette, i: usize) -> u32 {
    [
        p.bg, p.bar, p.raised, p.bg1, p.bg2, p.bg3, p.muted, p.fg, p.fg_dim, p.accent, p.failed,
        p.ok, p.modified,
    ][i]
}

/// Colour `i` in a table of theme colours.
pub fn override_slot(t: &mut ThemeColours, i: usize) -> &mut Option<String> {
    match i {
        0 => &mut t.bg,
        1 => &mut t.bar,
        2 => &mut t.raised,
        3 => &mut t.bg1,
        4 => &mut t.bg2,
        5 => &mut t.bg3,
        6 => &mut t.muted,
        7 => &mut t.fg,
        8 => &mut t.fg_dim,
        9 => &mut t.accent,
        10 => &mut t.failed,
        11 => &mut t.ok,
        _ => &mut t.modified,
    }
}

pub fn hex(color: u32) -> String {
    format!("#{color:06x}")
}

// ----- the picker's colour space ------------------------------------------------------------

/// Hue (0–360), saturation and value (0–1) of a colour.
pub fn to_hsv(color: u32) -> (f32, f32, f32) {
    let [r, g, b] = [16, 8, 0].map(|s| ((color >> s) & 255) as f32 / 255.);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let hue = if delta == 0. {
        0.
    } else if max == r {
        60. * (((g - b) / delta).rem_euclid(6.))
    } else if max == g {
        60. * ((b - r) / delta + 2.)
    } else {
        60. * ((r - g) / delta + 4.)
    };
    let saturation = if max == 0. { 0. } else { delta / max };
    (hue, saturation, max)
}

pub fn from_hsv(hue: f32, saturation: f32, value: f32) -> u32 {
    let c = value * saturation;
    let h = (hue.rem_euclid(360.)) / 60.;
    let x = c * (1. - (h % 2. - 1.).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.),
        1 => (x, c, 0.),
        2 => (0., c, x),
        3 => (0., x, c),
        4 => (x, 0., c),
        _ => (c, 0., x),
    };
    let m = value - c;
    let byte = |v: f32| ((v + m) * 255.).round().clamp(0., 255.) as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

// ----- the previews -------------------------------------------------------------------------

pub const CARD_W: f32 = 96.;
pub const CARD_H: f32 = 60.;

type Shape = Vec<(f32, f32)>;

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Shape {
    let r = r.min(w / 2.).min(h / 2.);
    let mut points = Vec::new();
    for (cx, cy, start) in [
        (x + w - r, y + r, -90.),
        (x + w - r, y + h - r, 0.),
        (x + r, y + h - r, 90.),
        (x + r, y + r, 180.),
    ] {
        for step in 0..=4 {
            let angle = (start + step as f32 * 22.5_f32).to_radians();
            points.push((cx + r * angle.cos(), cy + r * angle.sin()));
        }
    }
    points
}

fn circle(cx: f32, cy: f32, r: f32) -> Shape {
    (0..16)
        .map(|i| {
            let angle = (i as f32 * 22.5).to_radians();
            (cx + r * angle.cos(), cy + r * angle.sin())
        })
        .collect()
}

/// The part of `shape` where `keep(point) >= 0` (a half-plane; `keep` is linear).
fn clip(shape: &Shape, keep: impl Fn((f32, f32)) -> f32) -> Shape {
    let mut out = Vec::new();
    for i in 0..shape.len() {
        let (a, b) = (shape[i], shape[(i + 1) % shape.len()]);
        let (ka, kb) = (keep(a), keep(b));
        if ka >= 0. {
            out.push(a);
        }
        if (ka >= 0.) != (kb >= 0.) {
            let t = ka / (ka - kb);
            out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
        }
    }
    out
}

/// The light half of the "Follow system" card: below the diagonal from top right to bottom
/// left.
fn light_half(p: (f32, f32)) -> f32 {
    p.1 - (CARD_H - p.0 * CARD_H / CARD_W)
}

fn fill(shape: &Shape, color: u32, origin: gpui::Point<Pixels>, window: &mut Window) {
    if shape.len() < 3 {
        return;
    }
    let mut path = PathBuilder::fill();
    for (i, (x, y)) in shape.iter().enumerate() {
        let at = point(origin.x + px(*x), origin.y + px(*y));
        if i == 0 {
            path.move_to(at);
        } else {
            path.line_to(at);
        }
    }
    path.close();
    if let Ok(path) = path.build() {
        window.paint_path(path, theme::hsla(color));
    }
}

/// A small picture of Kiln in `p`: the sidebar, a flame, lines of text and the ANSI colours.
/// `half`: only the light half of the "Follow system" card.
fn paint_preview(p: &Palette, origin: gpui::Point<Pixels>, half: bool, window: &mut Window) {
    let shapes: Vec<(Shape, u32)> = vec![
        (rounded_rect(0., 0., CARD_W, CARD_H, 8.), p.bg),
        (
            clip(&rounded_rect(0., 0., CARD_W, CARD_H, 8.), |(x, _)| 22. - x),
            p.bar,
        ),
        (rounded_rect(5., 9., 12., 3., 1.5), p.fg),
        (rounded_rect(5., 17., 10., 3., 1.5), p.muted),
        (rounded_rect(5., 25., 11., 3., 1.5), p.muted),
        (rounded_rect(38., 10., 34., 3.4, 1.7), p.fg),
        (rounded_rect(29., 20., 44., 3.4, 1.7), p.ansi[1]),
        (rounded_rect(29., 28., 52., 3.4, 1.7), p.ansi[2]),
        (rounded_rect(29., 36., 30., 3.4, 1.7), p.fg_dim),
    ];
    let dots = (1..=6).map(|i| (circle(32. + (i - 1) as f32 * 9., 50., 2.8), p.ansi[i]));
    for (shape, color) in shapes.into_iter().chain(dots) {
        let shape = if half {
            clip(&shape, light_half)
        } else {
            shape
        };
        fill(&shape, color, origin, window);
    }
    if !half {
        let bounds = Bounds::new(
            point(origin.x + px(29.), origin.y + px(8.)),
            gpui::size(px(6.6), px(8.8)),
        );
        crate::flame::paint_in(p, 0, false, 1., bounds, window);
    }
}

/// Paints the card for theme `name` (its preview from its own palette; "system" split
/// diagonally, Kiln Dark above, Kiln Light below; "custom" from `custom`).
pub fn paint_card(name: &str, custom: &Palette, bounds: Bounds<Pixels>, window: &mut Window) {
    let origin = bounds.origin;
    match name {
        theme::SYSTEM => {
            paint_preview(&theme::PRESETS[0].palette, origin, false, window);
            let light =
                theme::preset_by_id("kiln-light").map_or(theme::DEFAULT_PALETTE, |p| p.palette);
            paint_preview(&light, origin, true, window);
        }
        theme::CUSTOM => paint_preview(custom, origin, false, window),
        id => {
            let p = theme::preset_by_id(id).map_or(theme::DEFAULT_PALETTE, |p| p.palette);
            paint_preview(&p, origin, false, window);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips() {
        for color in [0xe07b39, 0x000000, 0xffffff, 0x458588, 0x1e1e2e, 0xff0000] {
            let (h, s, v) = to_hsv(color);
            let back = from_hsv(h, s, v);
            for shift in [16, 8, 0] {
                let d = ((color >> shift) & 255) as i32 - ((back >> shift) & 255) as i32;
                assert!(d.abs() <= 1, "{color:06x} -> {back:06x}");
            }
        }
        assert_eq!(from_hsv(120., 1., 1.), 0x00ff00);
    }

    #[test]
    fn the_light_half_is_clipped_to_the_diagonal() {
        let card = rounded_rect(0., 0., CARD_W, CARD_H, 8.);
        let half = clip(&card, light_half);
        assert!(half.iter().all(|&p| light_half(p) >= -0.01));
        assert!(half.len() >= 3);
        // A shape above the diagonal disappears.
        assert!(clip(&rounded_rect(5., 9., 12., 3., 1.5), light_half).is_empty());
    }
}
