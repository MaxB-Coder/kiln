//! The flame Kiln draws where the prompt starts: four vector frames from `assets/flame`
//! (viewBox 12×16), swapped every 150 ms. The shell prints two blank cells there (see
//! `kiln_term::integration::MinimalPrompt`).

use std::sync::OnceLock;
use std::time::Duration;

use gpui::{Bounds, PathBuilder, Pixels, Point, Window, point, px};

use crate::theme::{self, palette};

/// How long each frame shows.
pub const FRAME_EVERY: Duration = Duration::from_millis(150);
pub const FRAMES: usize = 4;

const SOURCES: [&str; FRAMES] = [
    include_str!("../../../assets/flame/flame-1.svg"),
    include_str!("../../../assets/flame/flame-2.svg"),
    include_str!("../../../assets/flame/flame-3.svg"),
    include_str!("../../../assets/flame/flame-4.svg"),
];
const VIEW_W: f32 = 12.;
const VIEW_H: f32 = 16.;

/// How much of the text colour lightens the inner flame.
const INNER_LIGHT: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Move(f32, f32),
    /// Control points, then the end point.
    Cubic([f32; 6]),
    Close,
}

/// One frame: the outer shape, then the inner one.
pub type Frame = [Vec<Segment>; 2];

/// The frames, read from the SVG files once.
pub fn frames() -> &'static [Frame; FRAMES] {
    static FRAMES_CELL: OnceLock<[Frame; FRAMES]> = OnceLock::new();
    FRAMES_CELL.get_or_init(|| {
        SOURCES.map(|svg| {
            let mut paths = path_data(svg).map(parse_path);
            let outer = paths.next().unwrap_or_default();
            let inner = paths.next().unwrap_or_default();
            [outer, inner]
        })
    })
}

/// The `d` attributes, in order.
fn path_data(svg: &str) -> impl Iterator<Item = &str> {
    svg.split(" d=\"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
}

/// Absolute M, C and Z commands, which is all the frames use.
pub fn parse_path(d: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut numbers: Vec<f32> = Vec::new();
    let mut command = ' ';
    let flush = |command: char, numbers: &mut Vec<f32>, out: &mut Vec<Segment>| {
        match command {
            'M' => {
                for &[x, y] in numbers.as_chunks::<2>().0 {
                    out.push(Segment::Move(x, y));
                }
            }
            'C' => {
                for six in numbers.as_chunks::<6>().0 {
                    out.push(Segment::Cubic(*six));
                }
            }
            _ => {}
        }
        numbers.clear();
    };
    // Commands are single letters, written next to their first number ("M6 1 C8 4 …").
    let spaced: String = d
        .chars()
        .flat_map(|c| {
            if c.is_ascii_alphabetic() {
                vec![' ', c, ' ']
            } else {
                vec![c]
            }
        })
        .collect();
    for token in spaced.split([' ', ',']).filter(|t| !t.is_empty()) {
        match token {
            "M" | "C" => {
                flush(command, &mut numbers, &mut out);
                command = token.chars().next().unwrap_or(' ');
            }
            "Z" | "z" => {
                flush(command, &mut numbers, &mut out);
                out.push(Segment::Close);
                command = ' ';
            }
            number => {
                if let Ok(n) = number.parse() {
                    numbers.push(n);
                }
            }
        }
    }
    flush(command, &mut numbers, &mut out);
    out
}

/// The flame's size for a text line: a little under the line's height, the frames' aspect.
pub fn size(line_height: Pixels) -> (Pixels, Pixels) {
    let height = line_height * 0.82;
    (height * (VIEW_W / VIEW_H), height)
}

/// Paints `frame` into `bounds`: accent orange, or muted red after a failed command; `opacity`
/// follows the pane's.
pub fn paint(
    frame: usize,
    failed: bool,
    opacity: f32,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    let p = palette();
    paint_in(&p, frame, failed, opacity, bounds, window);
}

/// `paint` with a given palette (the theme previews in Settings).
pub fn paint_in(
    p: &theme::Palette,
    frame: usize,
    failed: bool,
    opacity: f32,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) {
    // From the theme: the accent (the failed colour after a failure), lighter inside.
    let outer = if failed { p.failed } else { p.accent };
    let colors = [outer, theme::blend(outer, p.fg, INNER_LIGHT)];
    let scale = (
        f32::from(bounds.size.width) / VIEW_W,
        f32::from(bounds.size.height) / VIEW_H,
    );
    let at = |x: f32, y: f32| -> Point<Pixels> {
        point(
            bounds.origin.x + px(x * scale.0),
            bounds.origin.y + px(y * scale.1),
        )
    };
    for (segments, color) in frames()[frame % FRAMES].iter().zip(colors) {
        let mut path = PathBuilder::fill();
        for segment in segments {
            match *segment {
                Segment::Move(x, y) => path.move_to(at(x, y)),
                Segment::Cubic([x1, y1, x2, y2, x, y]) => {
                    path.cubic_bezier_to(at(x, y), at(x1, y1), at(x2, y2))
                }
                Segment::Close => path.close(),
            }
        }
        if let Ok(path) = path.build() {
            window.paint_path(path, theme::hsla(color).opacity(opacity));
        }
    }
}

static FORCE_REDUCED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static FORCE_ANIMATION: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Moves the flame even when its window or pane has no focus (`--bench-flame`).
pub fn force_animation(on: bool) {
    FORCE_ANIMATION.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn animation_forced() -> bool {
    FORCE_ANIMATION.load(std::sync::atomic::Ordering::Relaxed)
}

/// Acts as if the system asked for reduced motion (the headless screenshots use it).
pub fn force_reduced_motion(on: bool) {
    FORCE_REDUCED.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the system asks apps to keep motion down (macOS: Reduce motion; Windows: Animation
/// effects off; GNOME: animations off).
pub fn reduced_motion() -> bool {
    if FORCE_REDUCED.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWorkspace;
        NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
    }
    #[cfg(windows)]
    {
        #[link(name = "user32")]
        unsafe extern "system" {
            fn SystemParametersInfoW(action: u32, param: u32, value: *mut i32, ini: u32) -> i32;
        }
        const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;
        let mut on: i32 = 1;
        // SAFETY: the call writes one BOOL into `on`.
        let ok = unsafe { SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION, 0, &mut on, 0) };
        ok != 0 && on == 0
    }
    #[cfg(target_os = "linux")]
    {
        linux_reduced_motion()
    }
}

/// GNOME's setting, read once in the background (it needs a process).
#[cfg(target_os = "linux")]
fn linux_reduced_motion() -> bool {
    use std::sync::atomic::{AtomicU8, Ordering};
    // 0: not read yet, 1: animations on, 2: off.
    static STATE: AtomicU8 = AtomicU8::new(0);
    if STATE
        .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
        .is_ok()
    {
        std::thread::spawn(|| {
            let off = std::process::Command::new("gsettings")
                .args(["get", "org.gnome.desktop.interface", "enable-animations"])
                .output()
                .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "false");
            if off {
                STATE.store(2, Ordering::SeqCst);
            }
        });
    }
    STATE.load(Ordering::SeqCst) == 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_frame_has_an_outer_and_an_inner_shape_inside_the_view_box() {
        for (i, frame) in frames().iter().enumerate() {
            for shape in frame {
                assert!(
                    matches!(shape.first(), Some(Segment::Move(..))),
                    "frame {i}"
                );
                assert_eq!(shape.last(), Some(&Segment::Close), "frame {i}");
                let cubics = shape
                    .iter()
                    .filter(|s| matches!(s, Segment::Cubic(_)))
                    .count();
                assert!(cubics >= 4, "frame {i}: {cubics} curves");
                for segment in shape {
                    let points: Vec<f32> = match segment {
                        Segment::Move(x, y) => vec![*x, *y],
                        Segment::Cubic(c) => c.to_vec(),
                        Segment::Close => vec![],
                    };
                    for pair in points.chunks(2) {
                        assert!(
                            (0. ..=VIEW_W).contains(&pair[0]),
                            "frame {i}: x {}",
                            pair[0]
                        );
                        assert!(
                            (0. ..=VIEW_H).contains(&pair[1]),
                            "frame {i}: y {}",
                            pair[1]
                        );
                    }
                }
            }
        }
        // The frames differ: it flickers.
        assert_ne!(frames()[0], frames()[1]);
    }

    #[test]
    fn paths_parse() {
        assert_eq!(
            parse_path("M6 1 C8 4 10.5 6 10.5 9.5 Z"),
            [
                Segment::Move(6., 1.),
                Segment::Cubic([8., 4., 10.5, 6., 10.5, 9.5]),
                Segment::Close
            ]
        );
    }

    #[test]
    fn the_flame_sits_inside_a_line() {
        let (w, h) = size(px(20.));
        assert!(h < px(20.));
        assert!((f32::from(w) / f32::from(h) - 0.75).abs() < 1e-4);
    }
}
