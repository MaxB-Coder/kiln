//! The custom app icon: an image the user picks in Settings. Only PNG and JPEG, at most 10 MB
//! and 4096 px a side, read as image data and nothing else. It is cropped to a square from the
//! middle, scaled to at most 512 px, given the icons' rounded corners and saved as a PNG copy in
//! Kiln's config folder (`icons/`), so moving or deleting the original changes nothing.

use std::hash::{Hash, Hasher};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use image::imageops::{self, FilterType};
use image::{ImageFormat, ImageReader, Limits, RgbaImage};

pub const MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const MAX_SIDE: u32 = 4096;
/// The copy's size: twice the largest icon Kiln shows (the Dock's 128 pt).
pub const STORED: u32 = 512;
/// The corner radius of the icons' tile, as a share of its side (185 on 824).
const CORNER: f32 = 185. / 824.;

#[derive(Debug, PartialEq)]
pub enum Rejected {
    TooLarge(u64),
    TooBig(u32, u32),
    NotPngOrJpeg,
    Unreadable(String),
}

impl std::fmt::Display for Rejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Rejected::TooLarge(bytes) => write!(
                f,
                "The image is {:.1} MB; up to 10 MB works.",
                *bytes as f64 / (1024. * 1024.)
            ),
            Rejected::TooBig(w, h) => {
                write!(f, "The image is {w}×{h} px; up to 4096 px a side works.")
            }
            Rejected::NotPngOrJpeg => write!(f, "Only PNG and JPEG images work."),
            Rejected::Unreadable(why) => write!(f, "The image could not be read ({why})."),
        }
    }
}

/// The format, from the file's first bytes (never its name).
fn format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else {
        None
    }
}

/// Checks and decodes an image, crops it square from the middle, scales it to at most
/// `STORED` px and rounds its corners like the icons' tile.
pub fn prepare(bytes: &[u8]) -> Result<RgbaImage, Rejected> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Rejected::TooLarge(bytes.len() as u64));
    }
    let format = format(bytes).ok_or(Rejected::NotPngOrJpeg)?;
    let reader = || {
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_SIDE);
        limits.max_image_height = Some(MAX_SIDE);
        limits.max_alloc = Some(256 * 1024 * 1024);
        reader.limits(limits);
        reader
    };
    let (w, h) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .map_err(|e| Rejected::Unreadable(e.to_string()))?;
    if w > MAX_SIDE || h > MAX_SIDE {
        return Err(Rejected::TooBig(w, h));
    }
    if w == 0 || h == 0 {
        return Err(Rejected::Unreadable("it is empty".into()));
    }
    let image = reader()
        .decode()
        .map_err(|e| Rejected::Unreadable(e.to_string()))?
        .into_rgba8();
    let side = w.min(h);
    let square = imageops::crop_imm(&image, (w - side) / 2, (h - side) / 2, side, side).to_image();
    let size = side.min(STORED);
    let mut out = if size == side {
        square
    } else {
        imageops::resize(&square, size, size, FilterType::Lanczos3)
    };
    round_corners(&mut out);
    Ok(out)
}

/// Clears the corners outside the tile's rounded square, softening the edge.
fn round_corners(image: &mut RgbaImage) {
    let side = image.width() as f32;
    let r = side * CORNER;
    for (x, y, pixel) in image.enumerate_pixels_mut() {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let dx = (r - px).max(px - (side - r)).max(0.);
        let dy = (r - py).max(py - (side - r)).max(0.);
        if dx == 0. || dy == 0. {
            continue;
        }
        let cover = (r - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0., 1.);
        pixel[3] = (pixel[3] as f32 * cover).round() as u8;
    }
}

/// Reads at most `MAX_BYTES` (and one byte more, to tell it is too large).
fn read_limited(path: &Path) -> Result<Vec<u8>, Rejected> {
    let file = std::fs::File::open(path).map_err(|e| Rejected::Unreadable(e.to_string()))?;
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| Rejected::Unreadable(e.to_string()))?;
    if bytes.len() as u64 > MAX_BYTES {
        let size = std::fs::metadata(path).map_or(bytes.len() as u64, |m| m.len());
        return Err(Rejected::TooLarge(size));
    }
    Ok(bytes)
}

/// Where the copies go: `icons/` next to kiln.toml.
pub fn folder() -> Option<PathBuf> {
    Some(crate::config::config_path()?.parent()?.join("icons"))
}

/// Checks `source` and saves the prepared copy into `folder` as
/// `<hash>-<original name>.png` (a new name for a new image, so Kiln sees the change).
pub fn import(source: &Path, folder: &Path) -> Result<PathBuf, Rejected> {
    let bytes = read_limited(source)?;
    let image = prepare(&bytes)?;
    let mut hasher = std::hash::DefaultHasher::new();
    bytes.hash(&mut hasher);
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let safe: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                c
            } else {
                '_'
            }
        })
        .take(64)
        .collect();
    let safe = if safe.trim_matches(['.', '_']).is_empty() {
        "image".to_string()
    } else {
        safe
    };
    std::fs::create_dir_all(folder).map_err(|e| Rejected::Unreadable(e.to_string()))?;
    let path = folder.join(format!("{:08x}-{safe}.png", hasher.finish() as u32));
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .map_err(|e| Rejected::Unreadable(e.to_string()))?;
    let temp = path.with_extension("png.tmp");
    std::fs::write(&temp, &png)
        .and_then(|()| std::fs::rename(&temp, &path))
        .map_err(|e| Rejected::Unreadable(e.to_string()))?;
    Ok(path)
}

/// Deletes a copy Kiln made (only a PNG in `folder`), when it is replaced or removed.
pub fn remove_copy(path: &Path, folder: &Path) {
    if path.parent() == Some(folder) && path.extension().is_some_and(|e| e == "png") {
        let _ = std::fs::remove_file(path);
    }
}

/// The original file's name, from the copy's ("3fa9c1d2-sunset.jpg.png" → "sunset.jpg").
pub fn display_name(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = name.strip_suffix(".png").unwrap_or(&name);
    match name.split_once('-') {
        Some((hash, rest)) if hash.len() == 8 && hash.chars().all(|c| c.is_ascii_hexdigit()) => {
            rest.to_string()
        }
        _ => name.to_string(),
    }
}

/// The saved copy, read back for use (checked again: the file may have been changed).
pub fn load(path: &Path) -> Result<RgbaImage, Rejected> {
    prepare(&read_limited(path)?)
}

/// `image` as a PNG `size` px square; `margin`: on the macOS icon grid (824 of 1024).
pub fn png(image: &RgbaImage, size: u32, margin: bool) -> Vec<u8> {
    let inner = if margin { size * 824 / 1024 } else { size };
    let scaled = imageops::resize(image, inner, inner, FilterType::Lanczos3);
    let mut canvas = RgbaImage::new(size, size);
    let at = i64::from((size - inner) / 2);
    imageops::overlay(&mut canvas, &scaled, at, at);
    let mut out = Vec::new();
    let _ = canvas.write_to(&mut Cursor::new(&mut out), ImageFormat::Png);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(w: u32, h: u32, format: ImageFormat) -> Vec<u8> {
        // Left half red, right half blue: the crop keeps the middle.
        let image = image::RgbImage::from_fn(w, h, |x, _| {
            if x < w / 2 {
                image::Rgb([255, 0, 0])
            } else {
                image::Rgb([0, 0, 255])
            }
        });
        let mut out = Vec::new();
        image.write_to(&mut Cursor::new(&mut out), format).unwrap();
        out
    }

    #[test]
    fn wide_images_are_cropped_square_from_the_middle() {
        let image = prepare(&encoded(300, 100, ImageFormat::Png)).unwrap();
        assert_eq!(image.dimensions(), (100, 100));
        // The middle of the wide image: red on the left, blue on the right.
        assert_eq!(image.get_pixel(30, 50).0[..3], [255, 0, 0]);
        assert_eq!(image.get_pixel(70, 50).0[..3], [0, 0, 255]);
        // Rounded corners: clear in the corner, solid inside.
        assert_eq!(image.get_pixel(0, 0).0[3], 0);
        assert_eq!(image.get_pixel(50, 50).0[3], 255);
    }

    #[test]
    fn big_images_are_scaled_down_and_jpeg_works() {
        let image = prepare(&encoded(900, 1200, ImageFormat::Jpeg)).unwrap();
        assert_eq!(image.dimensions(), (STORED, STORED));
    }

    #[test]
    fn other_files_are_refused() {
        assert_eq!(prepare(b"GIF89a....").unwrap_err(), Rejected::NotPngOrJpeg);
        assert_eq!(
            prepare(b"#!/bin/sh\nrm -rf /\n").unwrap_err(),
            Rejected::NotPngOrJpeg
        );
        // A PNG signature with nothing behind it.
        assert!(matches!(
            prepare(b"\x89PNG\r\n\x1a\nbroken"),
            Err(Rejected::Unreadable(_))
        ));
        assert_eq!(
            prepare(&vec![0x89; MAX_BYTES as usize + 1]).unwrap_err(),
            Rejected::TooLarge(MAX_BYTES + 1)
        );
    }

    #[test]
    fn images_over_4096_px_are_refused_before_decoding() {
        let wide = encoded(4097, 2, ImageFormat::Png);
        assert_eq!(prepare(&wide).unwrap_err(), Rejected::TooBig(4097, 2));
        assert!(prepare(&encoded(4096, 2, ImageFormat::Png)).is_ok());
    }

    #[test]
    fn the_copy_is_a_png_in_the_folder_with_the_original_name() {
        let dir = std::env::temp_dir().join(format!("kiln-custom-icon-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("my photo.jpg");
        std::fs::write(&source, encoded(64, 48, ImageFormat::Jpeg)).unwrap();
        let folder = dir.join("icons");
        let copy = import(&source, &folder).unwrap();
        assert_eq!(copy.parent(), Some(folder.as_path()));
        assert_eq!(display_name(&copy), "my_photo.jpg");
        std::fs::remove_file(&source).unwrap();
        assert_eq!(load(&copy).unwrap().dimensions(), (48, 48));
        // Kiln's copies only.
        remove_copy(&source, &folder);
        remove_copy(&copy, &folder);
        assert!(!copy.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn icon_pngs_keep_the_grid_margin_on_macos() {
        let image = prepare(&encoded(64, 64, ImageFormat::Png)).unwrap();
        let decode = |png: Vec<u8>| image::load_from_memory(&png).unwrap().into_rgba8();
        let dock = decode(png(&image, 256, true));
        assert_eq!(dock.dimensions(), (256, 256));
        assert_eq!(dock.get_pixel(10, 128).0[3], 0, "the margin is clear");
        assert_eq!(dock.get_pixel(128, 128).0[3], 255);
        let tile = decode(png(&image, 32, false));
        assert_eq!(tile.get_pixel(1, 16).0[3], 255, "the tile fills it");
    }
}
