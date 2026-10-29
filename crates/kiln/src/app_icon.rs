//! The app icon chosen in Settings (`icon`, `[icon_colours]` and `icon_path` in kiln.toml),
//! switched while Kiln runs: the Dock icon on macOS, each window's icon on Windows and on Linux
//! under X11. The app bundle, the exe and the installer keep the default (Kiln, terracotta):
//! changing the bundle would break its signature. The drawn icons' PNGs come from
//! assets/icon/variants (scripts/make-icon-masters.py, then scripts/make-icons.sh); each
//! platform embeds only the sizes it uses. A custom icon is made from its saved copy.

#[cfg(not(target_os = "macos"))]
use std::borrow::Cow;
use std::path::Path;
use std::sync::Mutex;

use gpui::Window;

use crate::config::{AppIcon, IconColour, IconDesign};

/// One PNG for every drawn design (rows, in `IconDesign::DRAWN` order) and colour (columns,
/// in `IconColour::ALL` order).
type Table = [[&'static [u8]; 7]; 4];

macro_rules! png {
    ($design:literal, $colour:literal, $file:literal) => {
        include_bytes!(concat!(
            "../../../assets/icon/variants/",
            $design,
            "-",
            $colour,
            "/",
            $file
        ))
        .as_slice()
    };
}

macro_rules! colours {
    ($design:literal, $file:literal) => {
        [
            png!($design, "terracotta", $file),
            png!($design, "black", $file),
            png!($design, "white", $file),
            png!($design, "ember", $file),
            png!($design, "slate", $file),
            png!($design, "sage", $file),
            png!($design, "sand", $file),
        ]
    };
}

macro_rules! table {
    ($file:literal) => {
        [
            colours!("kiln", $file),
            colours!("flame", $file),
            colours!("beehive", $file),
            colours!("line", $file),
        ]
    };
}

/// The Settings tiles: 128 px, with the macOS grid's margin.
static THUMBNAILS: Table = table!("mac-128.png");

/// The macOS Dock: 256 px (128 pt at 2x, its largest), with the margin.
#[cfg(target_os = "macos")]
static DOCK: Table = table!("mac-256.png");

/// The window icons, filling the tile: Windows picks by DPI (16 and 32 pt, up to 128 px at 300%).
#[cfg(windows)]
static TILES: [(u32, Table); 6] = [
    (16, table!("tile-16.png")),
    (24, table!("tile-24.png")),
    (32, table!("tile-32.png")),
    (48, table!("tile-48.png")),
    (64, table!("tile-64.png")),
    (128, table!("tile-128.png")),
];

/// The window icons, filling the tile: what X11's `_NET_WM_ICON` gets.
#[cfg(not(any(windows, target_os = "macos")))]
static TILES: [(u32, Table); 5] = [
    (16, table!("tile-16.png")),
    (32, table!("tile-32.png")),
    (48, table!("tile-48.png")),
    (64, table!("tile-64.png")),
    (128, table!("tile-128.png")),
];

fn index(design: IconDesign, colour: IconColour) -> (usize, usize) {
    let row = IconDesign::DRAWN
        .iter()
        .position(|d| *d == design)
        .unwrap_or(0);
    let column = IconColour::ALL
        .iter()
        .position(|c| *c == colour)
        .unwrap_or(0);
    (row, column)
}

/// The Settings tile for a drawn design in a colour.
pub fn thumbnail(design: IconDesign, colour: IconColour) -> &'static [u8] {
    let (row, column) = index(design, colour);
    THUMBNAILS[row][column]
}

/// The custom image, decoded once per path.
static CUSTOM: Mutex<Option<(std::path::PathBuf, image::RgbaImage)>> = Mutex::new(None);

fn custom_image(path: &Path) -> Result<image::RgbaImage, String> {
    let mut cache = CUSTOM.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached, image)) = cache.as_ref()
        && cached == path
    {
        return Ok(image.clone());
    }
    let image = crate::custom_icon::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
    *cache = Some((path.to_path_buf(), image.clone()));
    Ok(image)
}

/// The Settings tile for the custom image (128 px, with the margin).
pub fn custom_thumbnail(path: &Path) -> Result<(Vec<u8>, u32), String> {
    let image = custom_image(path)?;
    Ok((crate::custom_icon::png(&image, 128, true), image.width()))
}

/// The window icon PNG closest above `size` px.
#[cfg(not(target_os = "macos"))]
fn tile(icon: &AppIcon, size: u32) -> Result<Cow<'static, [u8]>, String> {
    Ok(match icon {
        AppIcon::Drawn(design, colour) => {
            let (row, column) = index(*design, *colour);
            let (_, table) = TILES
                .iter()
                .find(|(s, _)| *s >= size)
                .unwrap_or(&TILES[TILES.len() - 1]);
            Cow::Borrowed(table[row][column])
        }
        AppIcon::Custom(path) => {
            Cow::Owned(crate::custom_icon::png(&custom_image(path)?, size, false))
        }
    })
}

/// Shows `icon` as the app's icon. Call it for each new window and when the setting changes.
/// `startup`: the default icon needs nothing then (it is the bundle's). A custom image that
/// cannot be read any more warns and gives the default.
pub fn apply(icon: &AppIcon, window: &Window, startup: bool) {
    if startup && *icon == AppIcon::default() && cfg!(target_os = "macos") {
        return;
    }
    let result = platform::apply(icon, window).or_else(|err| {
        if matches!(icon, AppIcon::Custom(_)) {
            log::warn!("the custom icon: {err}; using the default");
            platform::apply(&AppIcon::default(), window)
        } else {
            Err(err)
        }
    });
    if let Err(err) = result {
        log::warn!("could not set the app icon: {err}");
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    use std::borrow::Cow;

    use super::{AppIcon, DOCK, custom_image, index};

    /// The Dock icon (app-wide). The default goes back to the bundle's icon.
    pub fn apply(icon: &AppIcon, _: &gpui::Window) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        if *icon == AppIcon::default() {
            // SAFETY: nil restores the bundle's icon.
            unsafe { app.setApplicationIconImage(None) };
            return Ok(());
        }
        let png: Cow<[u8]> = match icon {
            AppIcon::Drawn(design, colour) => {
                let (row, column) = index(*design, *colour);
                Cow::Borrowed(DOCK[row][column])
            }
            AppIcon::Custom(path) => {
                Cow::Owned(crate::custom_icon::png(&custom_image(path)?, 256, true))
            }
        };
        let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(&png))
            .ok_or("the icon did not decode")?;
        // SAFETY: an image, on the main thread.
        unsafe { app.setApplicationIconImage(Some(&image)) };
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::{AppIcon, tile};

    #[link(name = "user32")]
    unsafe extern "system" {
        fn GetDpiForWindow(hwnd: isize) -> u32;
        fn CreateIconFromResourceEx(
            bits: *const u8,
            size: u32,
            icon: i32,
            version: u32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> isize;
        fn SendMessageW(hwnd: isize, message: u32, wparam: usize, lparam: isize) -> isize;
    }
    const WM_SETICON: u32 = 0x0080;
    const ICON_SMALL: usize = 0;
    const ICON_BIG: usize = 1;

    /// The window's title-bar and taskbar icons, at the window's scale.
    pub fn apply(icon: &AppIcon, window: &gpui::Window) -> Result<(), String> {
        let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err("not a Win32 window".into());
        };
        let hwnd = handle.hwnd.get();
        // SAFETY: a live window of this process.
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        for (which, points) in [(ICON_SMALL, 16), (ICON_BIG, 32)] {
            let size = points * dpi / 96;
            let png = tile(icon, size)?;
            // SAFETY: PNG icon data (Windows Vista and later read it directly); the window
            // keeps the icon, which lives as long as the process.
            let hicon = unsafe {
                CreateIconFromResourceEx(
                    png.as_ptr(),
                    png.len() as u32,
                    1,
                    0x0003_0000,
                    size as i32,
                    size as i32,
                    0,
                )
            };
            if hicon == 0 {
                return Err("CreateIconFromResourceEx failed".into());
            }
            // SAFETY: as above.
            unsafe { SendMessageW(hwnd, WM_SETICON, which, hicon) };
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
    use x11rb::wrapper::ConnectionExt as _;

    use super::{AppIcon, TILES, tile};

    /// X11: the window's `_NET_WM_ICON`. Wayland has no way for an app to set its window's
    /// icon: the compositor shows the desktop file's (the default icon).
    pub fn apply(icon: &AppIcon, window: &gpui::Window) -> Result<(), String> {
        let handle = HasWindowHandle::window_handle(window).map_err(|e| e.to_string())?;
        let id = match handle.as_raw() {
            RawWindowHandle::Xcb(h) => h.window.get(),
            RawWindowHandle::Xlib(h) => h.window as u32,
            _ => return Ok(()),
        };
        let data = net_wm_icon(icon)?;
        let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let atom = conn
            .intern_atom(false, b"_NET_WM_ICON")
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?
            .atom;
        // This is a connection of its own: at start-up the window may not exist on the server
        // yet (GPUI's connection has not sent it), and the property would be lost with a
        // BadWindow nobody reads. So each try is checked, and a missing window tried again.
        let mut last = String::new();
        for _ in 0..40 {
            let checked = conn
                .change_property32(PropMode::REPLACE, id, atom, AtomEnum::CARDINAL, &data)
                .map_err(|e| e.to_string())?
                .check();
            match checked {
                Ok(()) => return Ok(()),
                Err(err) => last = err.to_string(),
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        Err(format!("the window never took its icon: {last}"))
    }

    /// The icons as `_NET_WM_ICON` wants them: width, height, then ARGB pixels, for each size.
    pub fn net_wm_icon(icon: &AppIcon) -> Result<Vec<u32>, String> {
        let mut data = Vec::new();
        for (size, _) in &TILES {
            let png = tile(icon, *size)?;
            let mut decoder = png::Decoder::new(std::io::Cursor::new(&*png));
            decoder
                .set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
            let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
            let mut buf = vec![0; reader.output_buffer_size().unwrap_or(0)];
            let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
            if info.color_type != png::ColorType::Rgba {
                return Err(format!("{:?} icon", info.color_type));
            }
            data.push(info.width);
            data.push(info.height);
            data.extend(buf[..info.buffer_size()].as_chunks::<4>().0.iter().map(
                |&[r, g, b, a]| {
                    u32::from(a) << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
                },
            ));
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_design_and_colour_has_a_tile_for_settings() {
        let mut seen = std::collections::HashSet::new();
        for design in IconDesign::DRAWN {
            for colour in IconColour::ALL {
                let png = thumbnail(design, colour);
                assert!(png.starts_with(b"\x89PNG"), "{design:?} {colour:?}");
                assert!(seen.insert(png), "{design:?} {colour:?} is a copy");
            }
        }
    }

    #[test]
    fn the_bundle_icon_is_the_default_master() {
        // scripts/make-icon-masters.py writes the masters; the default must stay the bundle's.
        assert_eq!(
            include_str!("../../../assets/icon/kiln.svg"),
            include_str!("../../../assets/icon/variants/kiln-terracotta.svg")
        );
        assert_eq!(
            AppIcon::default(),
            AppIcon::Drawn(IconDesign::Kiln, IconColour::Terracotta)
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn window_icons_come_in_every_size() {
        let icon = AppIcon::Drawn(IconDesign::Beehive, IconColour::Slate);
        let (row, column) = index(IconDesign::Beehive, IconColour::Slate);
        assert_eq!(*tile(&icon, 16).unwrap(), *TILES[0].1[row][column]);
        assert_eq!(
            *tile(&icon, 40).unwrap(),
            *TILES[TILES.len() - 3].1[row][column]
        );
        assert_eq!(
            *tile(&icon, 999).unwrap(),
            *TILES[TILES.len() - 1].1[row][column]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dock_icons_decode_as_appkit_images() {
        use objc2::AnyThread;
        use objc2_app_kit::NSImage;
        use objc2_foundation::NSData;

        let decode = |png: &[u8]| {
            let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png))
                .expect("an image");
            let size = image.size();
            (size.width, size.height)
        };
        for row in &DOCK {
            for png in row {
                assert!(decode(png).0 > 0.);
            }
        }
        // A custom image, as the Dock gets it.
        let mut source = Vec::new();
        image::RgbaImage::from_pixel(300, 200, image::Rgba([10, 120, 200, 255]))
            .write_to(
                &mut std::io::Cursor::new(&mut source),
                image::ImageFormat::Png,
            )
            .unwrap();
        let prepared = crate::custom_icon::prepare(&source).unwrap();
        assert!(decode(&crate::custom_icon::png(&prepared, 256, true)).0 > 0.);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x11_icons_decode() {
        let icon = AppIcon::Drawn(IconDesign::Line, IconColour::Black);
        let data = platform::net_wm_icon(&icon).unwrap();
        assert_eq!(&data[..2], &[16, 16]);
        let total: usize = [16, 32, 48, 64, 128].iter().map(|s| 2 + s * s).sum();
        assert_eq!(data.len(), total);
    }
}
