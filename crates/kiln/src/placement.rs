//! Where a window opens: like the last one (its size, position, display, and whether it was
//! maximised or full screen), full screen, or filling the screen (`window.open`). The last
//! window's placement is kept in the state file with its sessions.

use gpui::{Bounds, Pixels, Size, WindowBounds, point, px, size};
use serde::{Deserialize, Serialize};

/// The `window.open` setting.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenMode {
    /// Like the last window when Kiln quit.
    #[default]
    Last,
    /// Native full screen (its own Space on macOS).
    FullScreen,
    /// Maximised: the screen less the menu bar and Dock, not a Space of its own.
    Fill,
}

impl OpenMode {
    pub const ALL: [OpenMode; 3] = [OpenMode::Last, OpenMode::FullScreen, OpenMode::Fill];

    pub fn key(self) -> &'static str {
        match self {
            OpenMode::Last => "last",
            OpenMode::FullScreen => "full-screen",
            OpenMode::Fill => "fill",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OpenMode::Last => "Like last time",
            OpenMode::FullScreen => "Full screen",
            OpenMode::Fill => "Fill the screen",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Windowed,
    Maximized,
    FullScreen,
}

/// A window's placement as saved: its windowed bounds (what it goes back to from maximised or
/// full screen), in global logical pixels, the display it was on, and its mode.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Placement {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub mode: Mode,
    /// The display's UUID, when the system gives one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

impl Placement {
    pub fn of(bounds: WindowBounds, display: Option<String>) -> Self {
        let (mode, rect) = match bounds {
            WindowBounds::Windowed(b) => (Mode::Windowed, b),
            WindowBounds::Maximized(b) => (Mode::Maximized, b),
            WindowBounds::Fullscreen(b) => (Mode::FullScreen, b),
        };
        Placement {
            x: f32::from(rect.origin.x),
            y: f32::from(rect.origin.y),
            width: f32::from(rect.size.width),
            height: f32::from(rect.size.height),
            mode,
            display,
        }
    }

    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::new(
            point(px(self.x), px(self.y)),
            size(px(self.width), px(self.height)),
        )
    }
}

/// A display Kiln can open on: its UUID and the part windows may use.
#[derive(Clone, Debug, PartialEq)]
pub struct Screen {
    pub uuid: Option<String>,
    pub bounds: Bounds<Pixels>,
}

/// Where the first window opens: the bounds and mode, and which of `screens` (the first is the
/// main display). `fresh` is the size for a first launch with nothing saved.
pub fn initial(
    saved: Option<&Placement>,
    open: OpenMode,
    screens: &[Screen],
    fresh: Size<Pixels>,
) -> (WindowBounds, usize) {
    let screen = saved
        .and_then(|p| {
            screens
                .iter()
                .position(|s| p.display.is_some() && s.uuid == p.display)
                .or_else(|| {
                    // The display went away (or gives no UUID): the one the window's middle was
                    // over, if any is still there.
                    let centre = p.bounds().center();
                    screens.iter().position(|s| s.bounds.contains(&centre))
                })
        })
        .unwrap_or(0);
    let area = screens.get(screen).map(|s| s.bounds);
    let rect = match (saved, area) {
        (Some(p), Some(area)) => clamp(p.bounds(), area),
        (Some(p), None) => p.bounds(),
        (None, Some(area)) => clamp(
            Bounds::new(area.center() - point(fresh.width, fresh.height) / 2., fresh),
            area,
        ),
        (None, None) => Bounds::new(point(px(0.), px(0.)), fresh),
    };
    let mode = match open {
        OpenMode::FullScreen => Mode::FullScreen,
        OpenMode::Fill => Mode::Maximized,
        OpenMode::Last => saved.map_or(Mode::Windowed, |p| p.mode),
    };
    (with_mode(rect, mode), screen)
}

/// A new window beside `current` (the window in front): its windowed size, a little down and to
/// the right, on the same display; full screen or filling it only when the setting says so.
pub fn beside(
    current: Bounds<Pixels>,
    open: OpenMode,
    area: Option<Bounds<Pixels>>,
) -> WindowBounds {
    let offset = point(px(28.), px(28.));
    let mut rect = Bounds::new(current.origin + offset, current.size);
    if let Some(area) = area {
        // Past the display's edge, it goes back to the top left instead.
        if rect.bottom_right().x > area.bottom_right().x
            || rect.bottom_right().y > area.bottom_right().y
        {
            rect.origin = area.origin + offset;
        }
        rect = clamp(rect, area);
    }
    let mode = match open {
        OpenMode::FullScreen => Mode::FullScreen,
        OpenMode::Fill => Mode::Maximized,
        OpenMode::Last => Mode::Windowed,
    };
    with_mode(rect, mode)
}

fn with_mode(rect: Bounds<Pixels>, mode: Mode) -> WindowBounds {
    match mode {
        Mode::Windowed => WindowBounds::Windowed(rect),
        Mode::Maximized => WindowBounds::Maximized(rect),
        Mode::FullScreen => WindowBounds::Fullscreen(rect),
    }
}

/// `rect` moved (and if need be shrunk) to lie inside `area`.
pub fn clamp(rect: Bounds<Pixels>, area: Bounds<Pixels>) -> Bounds<Pixels> {
    let width = rect.size.width.min(area.size.width);
    let height = rect.size.height.min(area.size.height);
    let max_x = area.origin.x + area.size.width - width;
    let max_y = area.origin.y + area.size.height - height;
    let x = rect.origin.x.max(area.origin.x).min(max_x);
    let y = rect.origin.y.max(area.origin.y).min(max_y);
    Bounds::new(point(x, y), size(width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds::new(point(px(x), px(y)), size(px(w), px(h)))
    }

    fn screens() -> Vec<Screen> {
        vec![
            // The main display (under the menu bar), and one to its right.
            Screen {
                uuid: Some("main".into()),
                bounds: rect(0., 25., 1512., 920.),
            },
            Screen {
                uuid: Some("side".into()),
                bounds: rect(1512., 0., 1920., 1080.),
            },
        ]
    }

    fn saved(x: f32, y: f32, w: f32, h: f32, mode: Mode, display: Option<&str>) -> Placement {
        Placement {
            x,
            y,
            width: w,
            height: h,
            mode,
            display: display.map(str::to_string),
        }
    }

    const FRESH: Size<Pixels> = Size {
        width: px(1000.),
        height: px(600.),
    };

    #[test]
    fn a_saved_window_comes_back_as_it_was() {
        let p = saved(1600., 100., 900., 700., Mode::Windowed, Some("side"));
        let (bounds, screen) = initial(Some(&p), OpenMode::Last, &screens(), FRESH);
        assert_eq!(screen, 1);
        assert_eq!(
            bounds,
            WindowBounds::Windowed(rect(1600., 100., 900., 700.))
        );
        let full = saved(100., 100., 900., 700., Mode::FullScreen, Some("main"));
        assert_eq!(
            initial(Some(&full), OpenMode::Last, &screens(), FRESH),
            (WindowBounds::Fullscreen(rect(100., 100., 900., 700.)), 0)
        );
        let max = saved(100., 100., 900., 700., Mode::Maximized, Some("main"));
        assert!(matches!(
            initial(Some(&max), OpenMode::Last, &screens(), FRESH).0,
            WindowBounds::Maximized(_)
        ));
        // Placement round trip through what GPUI reports.
        assert_eq!(
            Placement::of(
                WindowBounds::Fullscreen(rect(100., 100., 900., 700.)),
                Some("main".into())
            ),
            full
        );
    }

    #[test]
    fn a_window_on_a_display_that_went_away_comes_back_on_one_that_is_there() {
        // It was on a third display, left of the main one.
        let p = saved(-1800., 200., 900., 700., Mode::Windowed, Some("gone"));
        let (bounds, screen) = initial(Some(&p), OpenMode::Last, &screens(), FRESH);
        assert_eq!(screen, 0, "the main display");
        assert_eq!(bounds, WindowBounds::Windowed(rect(0., 200., 900., 700.)));
        // Bigger than any display: shrunk to fit.
        let huge = saved(0., 0., 4000., 3000., Mode::Windowed, None);
        let (bounds, _) = initial(Some(&huge), OpenMode::Last, &screens(), FRESH);
        assert_eq!(bounds, WindowBounds::Windowed(rect(0., 25., 1512., 920.)));
        // No UUID saved: the display under the window's middle.
        let p = saved(2000., 300., 800., 600., Mode::Windowed, None);
        assert_eq!(initial(Some(&p), OpenMode::Last, &screens(), FRESH).1, 1);
    }

    #[test]
    fn the_setting_wins_and_a_first_launch_is_centred() {
        let p = saved(100., 100., 900., 700., Mode::Windowed, Some("main"));
        assert!(matches!(
            initial(Some(&p), OpenMode::FullScreen, &screens(), FRESH).0,
            WindowBounds::Fullscreen(_)
        ));
        assert!(matches!(
            initial(Some(&p), OpenMode::Fill, &screens(), FRESH).0,
            WindowBounds::Maximized(_)
        ));
        assert_eq!(
            initial(None, OpenMode::Last, &screens(), FRESH),
            (WindowBounds::Windowed(rect(256., 185., 1000., 600.)), 0)
        );
    }

    #[test]
    fn new_windows_open_beside_the_current_one() {
        let area = rect(0., 25., 1512., 920.);
        let current = rect(100., 100., 900., 600.);
        assert_eq!(
            beside(current, OpenMode::Last, Some(area)),
            WindowBounds::Windowed(rect(128., 128., 900., 600.))
        );
        // Near the bottom right: back to the top left.
        let low = rect(600., 330., 900., 600.);
        assert_eq!(
            beside(low, OpenMode::Last, Some(area)),
            WindowBounds::Windowed(rect(28., 53., 900., 600.))
        );
        assert!(matches!(
            beside(current, OpenMode::FullScreen, Some(area)),
            WindowBounds::Fullscreen(_)
        ));
    }
}
