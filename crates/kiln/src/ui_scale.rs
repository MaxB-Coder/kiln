//! The interface size: one factor for the sizes, gaps, radii and icons outside the terminal's
//! grid (the sidebar, the rail, the header's pills, the pop-ups and the diff viewer's frame).
//! The terminal's text has its own size (`font_size`, the zoom keys). Views write `ui(12.)`
//! where they would write `px(12.)`.

use std::sync::atomic::{AtomicU32, Ordering};

use gpui::{Pixels, px};
use serde::Deserialize;

/// `interface_size` in kiln.toml.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InterfaceSize {
    Smaller,
    #[default]
    Default,
    Larger,
    Largest,
}

impl InterfaceSize {
    pub const ALL: [InterfaceSize; 4] = [
        InterfaceSize::Smaller,
        InterfaceSize::Default,
        InterfaceSize::Larger,
        InterfaceSize::Largest,
    ];

    pub fn factor(self) -> f32 {
        match self {
            InterfaceSize::Smaller => 0.9,
            InterfaceSize::Default => 1.0,
            InterfaceSize::Larger => 1.15,
            InterfaceSize::Largest => 1.3,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            InterfaceSize::Smaller => "smaller",
            InterfaceSize::Default => "default",
            InterfaceSize::Larger => "larger",
            InterfaceSize::Largest => "largest",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            InterfaceSize::Smaller => "Smaller",
            InterfaceSize::Default => "Default",
            InterfaceSize::Larger => "Larger",
            InterfaceSize::Largest => "Largest",
        }
    }

    /// One step bigger or smaller, stopping at the ends ("Bigger interface" in the palette).
    pub fn step(self, bigger: bool) -> Self {
        let at = Self::ALL.iter().position(|s| *s == self).unwrap_or(1);
        let next = if bigger {
            (at + 1).min(Self::ALL.len() - 1)
        } else {
            at.saturating_sub(1)
        };
        Self::ALL[next]
    }
}

/// The factor in effect, as f32 bits (1.0 until the config is read).
static SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000);

pub fn scale() -> f32 {
    f32::from_bits(SCALE.load(Ordering::Relaxed))
}

pub fn set(size: InterfaceSize) {
    SCALE.store(size.factor().to_bits(), Ordering::Relaxed);
}

/// `v` logical pixels at the interface size, kept to whole quarter pixels so borders and text
/// stay crisp.
pub fn ui(v: f32) -> Pixels {
    px(scaled(v, scale()))
}

fn scaled(v: f32, scale: f32) -> f32 {
    (v * scale * 4.).round() / 4.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_scale_by_the_factor() {
        assert_eq!(scaled(12., 1.0), 12.);
        assert_eq!(scaled(12., 1.3), 15.5);
        assert_eq!(scaled(11., 0.9), 10.);
        assert_eq!(scaled(1., 1.15), 1.25);
        assert_eq!(scaled(0., 1.3), 0.);
    }

    #[test]
    fn steps_stop_at_the_ends() {
        use InterfaceSize::*;
        assert_eq!(Default.step(true), Larger);
        assert_eq!(Largest.step(true), Largest);
        assert_eq!(Default.step(false), Smaller);
        assert_eq!(Smaller.step(false), Smaller);
    }

    #[test]
    fn the_setting_reads_its_words() {
        #[derive(Deserialize)]
        struct C {
            interface_size: InterfaceSize,
        }
        let read = |s: &str| toml::from_str::<C>(s).unwrap().interface_size;
        assert_eq!(read("interface_size = \"smaller\""), InterfaceSize::Smaller);
        assert_eq!(read("interface_size = \"largest\""), InterfaceSize::Largest);
        assert!(toml::from_str::<C>("interface_size = \"huge\"").is_err());
    }
}
