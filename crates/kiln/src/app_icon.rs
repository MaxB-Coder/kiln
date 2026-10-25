//! The app icon chosen in Settings (`icon` in kiln.toml), switched while Kiln runs: the Dock
//! icon on macOS, each window's icon on Windows and on Linux under X11. The app bundle, the exe
//! and the installer keep the default (terracotta) icon: changing the bundle would break its
//! signature. The PNGs come from assets/icon/variants (scripts/make-icons.sh).

use gpui::Window;

use crate::config::AppIcon;

/// The Settings tile for a variant: 128 px, with the macOS grid's margin.
pub fn thumbnail(icon: AppIcon) -> &'static [u8] {
    match icon {
        AppIcon::Terracotta => {
            include_bytes!("../../../assets/icon/variants/terracotta/mac-128.png")
        }
        AppIcon::Black => include_bytes!("../../../assets/icon/variants/black/mac-128.png"),
        AppIcon::White => include_bytes!("../../../assets/icon/variants/white/mac-128.png"),
    }
}

/// Shows `icon` as the app's icon. Call it for each new window and when the setting changes.
/// `startup`: the default icon needs nothing then (it is the bundle's).
pub fn apply(icon: AppIcon, window: &Window, startup: bool) {
    if startup && icon == AppIcon::default() && cfg!(target_os = "macos") {
        return;
    }
    if let Err(err) = platform::apply(icon, window) {
        log::warn!("could not set the app icon: {err}");
    }
}

/// The window icons for Windows and Linux: (size, PNG), filling the tile.
#[cfg(not(target_os = "macos"))]
fn tiles(icon: AppIcon) -> [(u32, &'static [u8]); 7] {
    macro_rules! tiles {
        ($name:literal) => {
            [
                (
                    16,
                    include_bytes!(concat!(
                        "../../../assets/icon/variants/",
                        $name,
                        "/tile-16.png"
                    ))
                    .as_slice(),
                ),
                (
                    24,
                    include_bytes!(concat!(
                        "../../../assets/icon/variants/",
                        $name,
                        "/tile-24.png"
                    ))
                    .as_slice(),
                ),
                (
                    32,
                    include_bytes!(concat!(
                        "../../../assets/icon/variants/",
                        $name,
                        "/tile-32.png"
                    ))
                    .as_slice(),
                ),
                (
                    48,
                    include_bytes!(concat!(
                        "../../../assets/icon/variants/",
                        $name,
                        "/tile-48.png"
                    ))
                    .as_slice(),
                ),
                (
                    64,
                    include_bytes!(concat!(
                        "../../../assets/icon/variants/",
                        $name,
                        "/tile-64.png"
                    ))
                    .as_slice(),
                ),
                (
                    128,
                    include_bytes!(concat!(
                        "../../../assets/icon/variants/",
                        $name,
                        "/tile-128.png"
                    ))
                    .as_slice(),
                ),
                (
                    256,
                    include_bytes!(concat!(
                        "../../../assets/icon/variants/",
                        $name,
                        "/tile-256.png"
                    ))
                    .as_slice(),
                ),
            ]
        };
    }
    match icon {
        AppIcon::Terracotta => tiles!("terracotta"),
        AppIcon::Black => tiles!("black"),
        AppIcon::White => tiles!("white"),
    }
}

/// The smallest tile at least `size` px (or the largest).
#[cfg(not(target_os = "macos"))]
fn tile_for(icon: AppIcon, size: u32) -> &'static [u8] {
    let tiles = tiles(icon);
    tiles
        .iter()
        .find(|(s, _)| *s >= size)
        .unwrap_or(&tiles[tiles.len() - 1])
        .1
}

#[cfg(target_os = "macos")]
mod platform {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    use super::AppIcon;

    /// The Dock icon (app-wide). The default goes back to the bundle's icon.
    pub fn apply(icon: AppIcon, _: &gpui::Window) -> Result<(), String> {
        let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
        let app = NSApplication::sharedApplication(mtm);
        if icon == AppIcon::default() {
            // SAFETY: nil restores the bundle's icon.
            unsafe { app.setApplicationIconImage(None) };
            return Ok(());
        }
        let png: &[u8] = match icon {
            AppIcon::Terracotta => &[],
            AppIcon::Black => include_bytes!("../../../assets/icon/variants/black/mac-512.png"),
            AppIcon::White => include_bytes!("../../../assets/icon/variants/white/mac-512.png"),
        };
        let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png))
            .ok_or("the icon did not decode")?;
        // SAFETY: an image, on the main thread.
        unsafe { app.setApplicationIconImage(Some(&image)) };
        Ok(())
    }
}

#[cfg(windows)]
mod platform {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::{AppIcon, tile_for};

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
    pub fn apply(icon: AppIcon, window: &gpui::Window) -> Result<(), String> {
        let handle = window.window_handle().map_err(|e| e.to_string())?;
        let RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return Err("not a Win32 window".into());
        };
        let hwnd = handle.hwnd.get();
        // SAFETY: a live window of this process.
        let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96);
        for (which, points) in [(ICON_SMALL, 16), (ICON_BIG, 32)] {
            let size = points * dpi / 96;
            let png = tile_for(icon, size);
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
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt, PropMode};
    use x11rb::wrapper::ConnectionExt as _;

    use super::{AppIcon, tiles};

    /// X11: the window's `_NET_WM_ICON`. Wayland has no way for an app to set its window's
    /// icon: the compositor shows the desktop file's (the default icon).
    pub fn apply(icon: AppIcon, window: &gpui::Window) -> Result<(), String> {
        let handle = window.window_handle().map_err(|e| e.to_string())?;
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
        conn.change_property32(PropMode::REPLACE, id, atom, AtomEnum::CARDINAL, &data)
            .map_err(|e| e.to_string())?;
        conn.flush().map_err(|e| e.to_string())?;
        Ok(())
    }

    /// The icons as `_NET_WM_ICON` wants them: width, height, then ARGB pixels, for each size.
    pub fn net_wm_icon(icon: AppIcon) -> Result<Vec<u32>, String> {
        let mut data = Vec::new();
        for (_, png) in tiles(icon)
            .iter()
            .filter(|(s, _)| matches!(s, 16 | 32 | 48 | 64 | 128))
        {
            let mut decoder = png::Decoder::new(std::io::Cursor::new(*png));
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
    fn every_variant_has_a_tile_for_settings() {
        for icon in AppIcon::ALL {
            assert!(thumbnail(icon).starts_with(b"\x89PNG"), "{icon:?}");
        }
        assert_ne!(thumbnail(AppIcon::Black), thumbnail(AppIcon::White));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn window_icons_come_in_every_size() {
        for icon in AppIcon::ALL {
            assert_eq!(tile_for(icon, 16), tiles(icon)[0].1);
            assert_eq!(tile_for(icon, 40), tiles(icon)[3].1);
            assert_eq!(tile_for(icon, 999), tiles(icon)[6].1);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x11_icons_decode() {
        let data = platform::net_wm_icon(AppIcon::Black).unwrap();
        assert_eq!(&data[..2], &[16, 16]);
        let total: usize = [16, 32, 48, 64, 128].iter().map(|s| 2 + s * s).sum();
        assert_eq!(data.len(), total);
    }
}
