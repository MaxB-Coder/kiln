//! Keyboard, mouse and paste encoding (xterm conventions).
//!
//! Printable text is not encoded here: it reaches the shell through the platform input method,
//! so dead keys, compose and IMEs work. This module handles the keys an input method does not:
//! arrows, function keys, Ctrl and Alt combinations, and so on.

use std::borrow::Cow;

use alacritty_terminal::term::TermMode;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        shift: false,
        alt: false,
        ctrl: false,
    };

    /// The xterm modifier parameter: 1 + Shift(1) + Alt(2) + Ctrl(4).
    fn param(self) -> u8 {
        1 + self.shift as u8 + 2 * self.alt as u8 + 4 * self.ctrl as u8
    }

    fn any(self) -> bool {
        self.shift || self.alt || self.ctrl
    }
}

/// How Alt behaves on this platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltMode {
    /// Alt+key sends ESC then the key (Linux, Windows).
    Escape,
    /// Alt composes characters, like macOS Option; the input method sends the result.
    Compose,
}

/// Encodes a key press. `key` uses lowercase names ("up", "f5", "enter", or the character
/// itself). Returns `None` when the key should go to the input method as text instead.
pub fn encode_key(
    key: &str,
    mods: Mods,
    mode: TermMode,
    alt: AltMode,
) -> Option<Cow<'static, [u8]>> {
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    // Cursor keys: SS3 in application mode, CSI otherwise, CSI 1;m when modified.
    let cursor = |c: u8| -> Cow<'static, [u8]> {
        if mods.any() {
            format!("\x1b[1;{}{}", mods.param(), c as char)
                .into_bytes()
                .into()
        } else if app_cursor {
            vec![0x1b, b'O', c].into()
        } else {
            vec![0x1b, b'[', c].into()
        }
    };
    // Editing and function keys: CSI n ~ or CSI n;m ~.
    let tilde = |n: u8| -> Cow<'static, [u8]> {
        if mods.any() {
            format!("\x1b[{n};{}~", mods.param()).into_bytes().into()
        } else {
            format!("\x1b[{n}~").into_bytes().into()
        }
    };
    // F1–F4: SS3 P–S, or CSI 1;m P–S when modified.
    let ss3 = |c: u8| -> Cow<'static, [u8]> {
        if mods.any() {
            format!("\x1b[1;{}{}", mods.param(), c as char)
                .into_bytes()
                .into()
        } else {
            vec![0x1b, b'O', c].into()
        }
    };
    let alt_prefixed = |bytes: &'static [u8]| -> Cow<'static, [u8]> {
        if mods.alt && alt == AltMode::Escape {
            [&[0x1b], bytes].concat().into()
        } else {
            bytes.into()
        }
    };

    let bytes = match key {
        "up" => cursor(b'A'),
        "down" => cursor(b'B'),
        "right" => cursor(b'C'),
        "left" => cursor(b'D'),
        "home" => cursor(b'H'),
        "end" => cursor(b'F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" => ss3(b'P'),
        "f2" => ss3(b'Q'),
        "f3" => ss3(b'R'),
        "f4" => ss3(b'S'),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        "enter" => alt_prefixed(b"\r"),
        "escape" => alt_prefixed(b"\x1b"),
        "tab" if mods.shift => b"\x1b[Z".as_slice().into(),
        "tab" => alt_prefixed(b"\t"),
        "backspace" if mods.ctrl => alt_prefixed(b"\x08"),
        "backspace" => alt_prefixed(b"\x7f"),
        "space" if mods.ctrl => alt_prefixed(b"\0"),
        "space" if mods.alt && alt == AltMode::Escape => b"\x1b ".as_slice().into(),
        _ => return encode_char_key(key, mods, alt),
    };
    Some(bytes)
}

/// Ctrl and Alt combinations of single-character keys.
fn encode_char_key(key: &str, mods: Mods, alt: AltMode) -> Option<Cow<'static, [u8]>> {
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };

    let mut out = Vec::with_capacity(2);
    if mods.alt && alt == AltMode::Escape {
        out.push(0x1b);
    }

    if mods.ctrl {
        let control = match c.to_ascii_lowercase() {
            c @ 'a'..='z' => c as u8 - b'a' + 1,
            '@' | '2' | ' ' => 0x00,
            '[' | '3' => 0x1b,
            '\\' | '4' => 0x1c,
            ']' | '5' => 0x1d,
            '^' | '6' => 0x1e,
            '_' | '-' | '7' | '/' => 0x1f,
            '8' | '?' => 0x7f,
            _ => return None,
        };
        out.push(control);
        return Some(out.into());
    }

    if mods.alt && alt == AltMode::Escape {
        let c = if mods.shift {
            c.to_uppercase().next().unwrap_or(c)
        } else {
            c
        };
        let mut utf8 = [0; 4];
        out.extend_from_slice(c.encode_utf8(&mut utf8).as_bytes());
        return Some(out.into());
    }

    None
}

/// Wraps pasted text for the shell: bracketed when the program asked for it, with newlines
/// turned into carriage returns otherwise.
pub fn encode_paste(text: &str, mode: TermMode) -> Vec<u8> {
    if mode.contains(TermMode::BRACKETED_PASTE) {
        // A paste must not be able to end the bracket early and run what follows.
        let text = text.replace("\x1b[201~", "");
        let mut out = Vec::with_capacity(text.len() + 12);
        out.extend_from_slice(b"\x1b[200~");
        out.extend_from_slice(text.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseAction {
    Press,
    Release,
    /// Movement; `Some(button)` while a button is held.
    Move(Option<MouseButton>),
}

/// Whether the program wants this kind of mouse event at all.
pub fn wants_mouse(action: MouseAction, mode: TermMode) -> bool {
    match action {
        MouseAction::Press | MouseAction::Release => mode.intersects(TermMode::MOUSE_MODE),
        MouseAction::Move(Some(_)) => {
            mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION)
        }
        MouseAction::Move(None) => mode.contains(TermMode::MOUSE_MOTION),
    }
}

/// Encodes a mouse event at a 0-based cell. Returns `None` if the program did not ask for it.
pub fn encode_mouse(
    button: MouseButton,
    action: MouseAction,
    column: usize,
    line: usize,
    mods: Mods,
    mode: TermMode,
) -> Option<Vec<u8>> {
    if !wants_mouse(action, mode) {
        return None;
    }

    let base = match (action, button) {
        (MouseAction::Move(None), _) => 3 + 32,
        (MouseAction::Move(Some(held)), _) => button_code(held) + 32,
        (_, button) => button_code(button),
    };
    let code = base + 4 * mods.shift as u32 + 8 * mods.alt as u32 + 16 * mods.ctrl as u32;

    if mode.contains(TermMode::SGR_MOUSE) {
        let suffix = if action == MouseAction::Release {
            'm'
        } else {
            'M'
        };
        return Some(format!("\x1b[<{code};{};{}{suffix}", column + 1, line + 1).into_bytes());
    }

    // Legacy X10 encoding: release is button 3, wheel releases are not reported,
    // positions are limited to 223.
    let code = match (action, button) {
        (MouseAction::Release, MouseButton::WheelUp | MouseButton::WheelDown) => return None,
        (MouseAction::Release, _) => 3 + code - button_code(button),
        _ => code,
    };
    if column > 222 || line > 222 {
        return None;
    }
    Some(vec![
        0x1b,
        b'[',
        b'M',
        32 + code as u8,
        33 + column as u8,
        33 + line as u8,
    ])
}

fn button_code(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str, mods: Mods) -> Option<Vec<u8>> {
        encode_key(k, mods, TermMode::empty(), AltMode::Escape).map(|b| b.into_owned())
    }

    const CTRL: Mods = Mods {
        shift: false,
        alt: false,
        ctrl: true,
    };
    const ALT: Mods = Mods {
        shift: false,
        alt: true,
        ctrl: false,
    };
    const SHIFT: Mods = Mods {
        shift: true,
        alt: false,
        ctrl: false,
    };

    #[test]
    fn plain_text_goes_to_the_input_method() {
        assert_eq!(key("a", Mods::NONE), None);
        assert_eq!(key("space", Mods::NONE), None);
        assert_eq!(key("a", SHIFT), None);
    }

    #[test]
    fn control_keys() {
        assert_eq!(key("c", CTRL), Some(vec![0x03]));
        assert_eq!(key("[", CTRL), Some(vec![0x1b]));
        assert_eq!(key("space", CTRL), Some(vec![0x00]));
        assert_eq!(key("enter", Mods::NONE), Some(b"\r".to_vec()));
        assert_eq!(key("backspace", Mods::NONE), Some(vec![0x7f]));
        assert_eq!(key("tab", SHIFT), Some(b"\x1b[Z".to_vec()));
    }

    #[test]
    fn alt_sends_escape_unless_composing() {
        assert_eq!(key("b", ALT), Some(b"\x1bb".to_vec()));
        assert_eq!(
            encode_key("b", ALT, TermMode::empty(), AltMode::Compose),
            None
        );
    }

    #[test]
    fn cursor_keys_follow_application_mode() {
        assert_eq!(key("up", Mods::NONE), Some(b"\x1b[A".to_vec()));
        let app = encode_key("up", Mods::NONE, TermMode::APP_CURSOR, AltMode::Escape).unwrap();
        assert_eq!(&*app, b"\x1bOA");
        assert_eq!(key("left", CTRL), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(key("f5", SHIFT), Some(b"\x1b[15;2~".to_vec()));
        assert_eq!(key("f1", Mods::NONE), Some(b"\x1bOP".to_vec()));
    }

    #[test]
    fn bracketed_paste_cannot_be_escaped() {
        let out = encode_paste("a\x1b[201~rm -rf ~\n", TermMode::BRACKETED_PASTE);
        assert_eq!(out, b"\x1b[200~arm -rf ~\n\x1b[201~");
        assert_eq!(encode_paste("a\nb", TermMode::empty()), b"a\rb");
    }

    #[test]
    fn sgr_mouse() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let press = encode_mouse(
            MouseButton::Left,
            MouseAction::Press,
            4,
            2,
            Mods::NONE,
            mode,
        );
        assert_eq!(press.unwrap(), b"\x1b[<0;5;3M");
        let release = encode_mouse(
            MouseButton::Left,
            MouseAction::Release,
            4,
            2,
            Mods::NONE,
            mode,
        );
        assert_eq!(release.unwrap(), b"\x1b[<0;5;3m");
        assert_eq!(
            encode_mouse(
                MouseButton::Left,
                MouseAction::Move(None),
                4,
                2,
                Mods::NONE,
                mode
            ),
            None
        );
        let wheel = encode_mouse(
            MouseButton::WheelDown,
            MouseAction::Press,
            0,
            0,
            Mods::NONE,
            mode,
        );
        assert_eq!(wheel.unwrap(), b"\x1b[<65;1;1M");
    }

    #[test]
    fn no_mouse_reports_unless_asked() {
        let out = encode_mouse(
            MouseButton::Left,
            MouseAction::Press,
            0,
            0,
            Mods::NONE,
            TermMode::empty(),
        );
        assert_eq!(out, None);
    }
}
