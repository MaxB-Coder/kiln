//! A changed binary file for the diff viewer: its size before (HEAD) and after (the worktree),
//! and for images (PNG, JPEG, GIF, WebP, ICO) both pictures, decoded with the custom icon's
//! limits (`custom_icon::MAX_BYTES`, `MAX_SIDE`) and re-encoded as PNG for the view. The
//! format comes from the bytes, never the name; the name only says whether to try.

use std::io::Cursor;
use std::process::{Command, Stdio};

use image::{ImageFormat, ImageReader, Limits};

use crate::custom_icon::{MAX_BYTES, MAX_SIDE};
use crate::rail::bytes_label;

/// One side of the change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    pub bytes: u64,
    /// The picture, when it is an image within the limits.
    pub picture: Option<Picture>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// The image as PNG (a GIF's first frame).
    pub png: Vec<u8>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BinaryChange {
    /// HEAD's version; `None` for a new file.
    pub before: Option<Side>,
    /// The worktree's; `None` for a deleted file.
    pub after: Option<Side>,
    /// The name says image (the viewer shows pictures, or their sizes).
    pub image: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Modified,
    New,
    Deleted,
}

impl BinaryChange {
    pub fn status(&self) -> Status {
        match (&self.before, &self.after) {
            (None, _) => Status::New,
            (_, None) => Status::Deleted,
            _ => Status::Modified,
        }
    }

    /// "Binary file · 12 KB → 14 KB", "Binary file · new · 14 KB", "… · deleted · 12 KB".
    pub fn summary(&self) -> String {
        let size = |s: &Option<Side>| s.as_ref().map_or(0, |s| s.bytes);
        match self.status() {
            Status::Modified => format!(
                "Binary file · {} → {}",
                bytes_label(size(&self.before)),
                bytes_label(size(&self.after))
            ),
            Status::New => format!("Binary file · new · {}", bytes_label(size(&self.after))),
            Status::Deleted => {
                format!(
                    "Binary file · deleted · {}",
                    bytes_label(size(&self.before))
                )
            }
        }
    }
}

/// A side's caption: "640 × 480 · 12 KB", or the size alone when there is no picture.
pub fn caption(side: &Side) -> String {
    match &side.picture {
        Some(p) => format!("{} × {} · {}", p.width, p.height, bytes_label(side.bytes)),
        None => bytes_label(side.bytes),
    }
}

/// Whether the name is one of the images shown (SVG is text: a normal diff).
pub fn is_image(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    path.contains('.')
        && matches!(
            ext.as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico"
        )
}

/// The format, from the first bytes.
fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(ImageFormat::Png)
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(ImageFormat::Gif)
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::WebP)
    } else if bytes.starts_with(&[0, 0, 1, 0]) {
        Some(ImageFormat::Ico)
    } else {
        None
    }
}

/// Decodes an image within the limits; `None` when it is too large, not an image Kiln reads,
/// or broken.
pub fn decode(bytes: &[u8]) -> Option<Picture> {
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let format = sniff(bytes)?;
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .ok()?;
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(256 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader.decode().ok()?.into_rgba8();
    let mut png = Vec::new();
    rgba.write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
        .ok()?;
    Some(Picture { width, height, png })
}

/// The change of binary file `path` (relative to the repository at `root`): HEAD's version
/// through `git cat-file`, the worktree's from the file. Blocking.
pub fn load(root: &str, path: &str) -> BinaryChange {
    let image = is_image(path);
    let spec = format!("HEAD:{path}");
    let git = |args: &[&str]| -> Option<Vec<u8>> {
        let output = Command::new("git")
            .args(["-c", "core.fsmonitor=false", "-C", root, "--no-pager"])
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        output.status.success().then_some(output.stdout)
    };
    let before = git(&["cat-file", "-s", &spec])
        .and_then(|out| String::from_utf8_lossy(&out).trim().parse::<u64>().ok())
        .map(|bytes| Side {
            bytes,
            picture: (image && bytes <= MAX_BYTES)
                .then(|| git(&["cat-file", "blob", &spec]))
                .flatten()
                .and_then(|b| decode(&b)),
        });
    let file = std::path::Path::new(root).join(path);
    let after = std::fs::metadata(&file)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| Side {
            bytes: m.len(),
            picture: (image && m.len() <= MAX_BYTES)
                .then(|| std::fs::read(&file).ok())
                .flatten()
                .and_then(|b| decode(&b)),
        });
    BinaryChange {
        before,
        after,
        image,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(w, h, image::Rgba([200, 80, 40, 128]));
        let mut out = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
            .unwrap();
        out
    }

    fn side(bytes: u64) -> Option<Side> {
        Some(Side {
            bytes,
            picture: None,
        })
    }

    #[test]
    fn status_and_sizes() {
        let change = |before, after| BinaryChange {
            before,
            after,
            image: false,
        };
        let modified = change(side(12 * 1024), side(14 * 1024));
        assert_eq!(modified.status(), Status::Modified);
        assert_eq!(modified.summary(), "Binary file · 12 KB → 14 KB");
        let new = change(None, side(2048));
        assert_eq!(new.status(), Status::New);
        assert_eq!(new.summary(), "Binary file · new · 2 KB");
        let deleted = change(side(3 * 1024 * 1024), None);
        assert_eq!(deleted.status(), Status::Deleted);
        assert_eq!(deleted.summary(), "Binary file · deleted · 3 MB");
    }

    #[test]
    fn images_decode_within_the_limits_only() {
        let picture = decode(&png(40, 20)).expect("a small PNG");
        assert_eq!((picture.width, picture.height), (40, 20));
        assert!(picture.png.starts_with(b"\x89PNG"));
        assert_eq!(
            caption(&Side {
                bytes: 2048,
                picture: Some(picture)
            }),
            "40 × 20 · 2 KB"
        );
        // Wider than MAX_SIDE: sizes only.
        assert_eq!(decode(&png(MAX_SIDE + 1, 1)), None);
        // Not an image, whatever the name says.
        assert_eq!(decode(b"GIF? no, plain text"), None);
        assert_eq!(caption(&side(5000).unwrap()), "5 KB");
    }

    #[test]
    fn which_names_are_images() {
        for name in [
            "a.png",
            "b/c.JPG",
            "x.jpeg",
            "y.gif",
            "z.webp",
            "favicon.ico",
        ] {
            assert!(is_image(name), "{name}");
        }
        for name in ["logo.svg", "png", "notes.txt", "font.woff2"] {
            assert!(!is_image(name), "{name}");
        }
    }
}
