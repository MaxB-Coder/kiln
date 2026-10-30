//! Kiln's key bindings in one place: registered at startup, listed by the shortcuts pop-up (⌘/)
//! and shown next to actions in the palette. Every ⌘ shortcut on macOS is Ctrl+Shift on Windows
//! and Linux.
//!
//! The pop-up reads the live keymap, so it shows exactly what is bound; this module adds only a
//! readable name and a group per action (`describe`), and a test fails when a bound action has
//! none.

use gpui::{Action, App, KeyBinding, Keystroke};

use crate::*;

/// How keys are written: macOS glyphs (⌘⇧D) or "Ctrl+Shift+D".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyStyle {
    Mac,
    Pc,
}

impl KeyStyle {
    pub fn current() -> KeyStyle {
        if cfg!(target_os = "macos") {
            KeyStyle::Mac
        } else {
            KeyStyle::Pc
        }
    }
}

/// Every binding for a platform's style (macOS, or Windows and Linux).
pub fn bindings(style: KeyStyle) -> Vec<KeyBinding> {
    let primary = match style {
        KeyStyle::Mac => "cmd",
        KeyStyle::Pc => "ctrl-shift",
    };
    let k = |keys: &str| format!("{primary}-{keys}");
    let terminal = Some("Terminal");
    let mut bindings = vec![
        KeyBinding::new(&k("c"), Copy, terminal),
        KeyBinding::new(&k("v"), Paste, terminal),
        KeyBinding::new(&k("a"), SelectAll, terminal),
        KeyBinding::new(&k("alt-c"), CopyLastOutputRaw, terminal),
        KeyBinding::new(&k("n"), NewWindow, None),
        KeyBinding::new(&k("t"), NewSession, None),
        KeyBinding::new(&k("w"), ClosePane, None),
        KeyBinding::new(&k("d"), SplitRight, None),
        KeyBinding::new(&k("]"), FocusNextPane, None),
        KeyBinding::new(&k("["), FocusPrevPane, None),
        KeyBinding::new(&k("b"), ToggleSidebar, None),
        KeyBinding::new(&k("i"), ToggleRail, None),
        KeyBinding::new(
            match style {
                KeyStyle::Mac => "cmd-shift-i",
                KeyStyle::Pc => "ctrl-alt-shift-i",
            },
            FocusRail,
            None,
        ),
        KeyBinding::new(&k("."), ToggleFocusMode, None),
        KeyBinding::new(&k("f"), Find, None),
        KeyBinding::new(&k("k"), CommandPaletteToggle, None),
        KeyBinding::new(&k("/"), ToggleShortcuts, None),
        KeyBinding::new(&k("r"), RenameSession, None),
        // F2 too, except when a full-screen program runs (htop, mc and others use it).
        KeyBinding::new("f2", RenameSession, Some("Terminal && !full_screen")),
        KeyBinding::new(
            match style {
                KeyStyle::Mac => "cmd-shift-r",
                KeyStyle::Pc => "ctrl-alt-shift-r",
            },
            RenameProject,
            None,
        ),
        KeyBinding::new(&k(","), EditConfig, None),
        KeyBinding::new(&k("q"), Quit, None),
        KeyBinding::new(&k("="), ZoomIn, None),
        KeyBinding::new(&k("-"), ZoomOut, None),
        KeyBinding::new(&k("0"), ZoomReset, None),
        // Commands as a whole: ⌘↑/⌘↓ select the previous or next, ⇧ adds to the selection.
        KeyBinding::new(&k("up"), SelectPrevCommand, terminal),
        KeyBinding::new(&k("down"), SelectNextCommand, terminal),
        KeyBinding::new(
            match style {
                KeyStyle::Mac => "cmd-shift-up",
                KeyStyle::Pc => "ctrl-alt-shift-up",
            },
            ExtendSelectionUp,
            terminal,
        ),
        KeyBinding::new(
            match style {
                KeyStyle::Mac => "cmd-shift-down",
                KeyStyle::Pc => "ctrl-alt-shift-down",
            },
            ExtendSelectionDown,
            terminal,
        ),
        KeyBinding::new("shift-pageup", ScrollPageUp, terminal),
        KeyBinding::new("shift-pagedown", ScrollPageDown, terminal),
        KeyBinding::new("shift-home", ScrollToTop, terminal),
        KeyBinding::new("shift-end", ScrollToBottom, terminal),
    ];
    match style {
        KeyStyle::Mac => bindings.extend([
            // ⌘⇧C: last output as a fenced block. Elsewhere Ctrl+Shift+C covers it (the
            // selection, or the last output when nothing is selected).
            KeyBinding::new("cmd-shift-c", CopyLastOutput, terminal),
            KeyBinding::new("cmd-shift-]", NextSession, None),
            KeyBinding::new("cmd-shift-[", PrevSession, None),
            KeyBinding::new("cmd-1", SelectSession1, None),
            KeyBinding::new("cmd-2", SelectSession2, None),
            KeyBinding::new("cmd-3", SelectSession3, None),
            KeyBinding::new("cmd-4", SelectSession4, None),
            KeyBinding::new("cmd-5", SelectSession5, None),
            KeyBinding::new("cmd-6", SelectSession6, None),
            KeyBinding::new("cmd-7", SelectSession7, None),
            KeyBinding::new("cmd-8", SelectSession8, None),
            KeyBinding::new("cmd-9", SelectSession9, None),
        ]),
        KeyStyle::Pc => bindings.extend([
            // Windows reports Shift with a punctuation or digit key as the character it types
            // ("ctrl-shift-/" arrives as "ctrl-?"), so each Ctrl+Shift binding on one of those
            // keys has that form too (`is_layout_alias`; the pop-up lists only the first).
            KeyBinding::new("ctrl-?", ToggleShortcuts, None),
            KeyBinding::new("ctrl-<", EditConfig, None),
            KeyBinding::new("ctrl->", ToggleFocusMode, None),
            KeyBinding::new("ctrl-+", ZoomIn, None),
            KeyBinding::new("ctrl-_", ZoomOut, None),
            KeyBinding::new("ctrl-)", ZoomReset, None),
            KeyBinding::new("ctrl-}", FocusNextPane, None),
            KeyBinding::new("ctrl-{", FocusPrevPane, None),
            // F1 for the shortcuts on any layout, as F2 renames (not in full-screen programs).
            KeyBinding::new("f1", ToggleShortcuts, Some("Terminal && !full_screen")),
            KeyBinding::new("shift-insert", Paste, terminal),
            KeyBinding::new("ctrl-tab", NextSession, None),
            KeyBinding::new("ctrl-shift-tab", PrevSession, None),
            KeyBinding::new("ctrl-alt-1", SelectSession1, None),
            KeyBinding::new("ctrl-alt-2", SelectSession2, None),
            KeyBinding::new("ctrl-alt-3", SelectSession3, None),
            KeyBinding::new("ctrl-alt-4", SelectSession4, None),
            KeyBinding::new("ctrl-alt-5", SelectSession5, None),
            KeyBinding::new("ctrl-alt-6", SelectSession6, None),
            KeyBinding::new("ctrl-alt-7", SelectSession7, None),
            KeyBinding::new("ctrl-alt-8", SelectSession8, None),
            KeyBinding::new("ctrl-alt-9", SelectSession9, None),
        ]),
    }
    bindings
}

/// Sections of the shortcuts pop-up, in order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Group {
    Sessions,
    Panes,
    View,
    CopyEdit,
    FindPalette,
    Scroll,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Group::Sessions,
        Group::Panes,
        Group::View,
        Group::CopyEdit,
        Group::FindPalette,
        Group::Scroll,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Sessions => "Sessions and windows",
            Group::Panes => "Panes",
            Group::View => "View",
            Group::CopyEdit => "Commands and copy",
            Group::FindPalette => "Find and palette",
            Group::Scroll => "Scroll",
        }
    }
}

/// Every action with a key binding: its name in the pop-up and its section, in the pop-up's
/// order. (The numbered sessions share one row.)
const DESCRIPTIONS: &[(&str, &str, Group)] = &[
    ("NewSession", "New session", Group::Sessions),
    ("NextSession", "Next session", Group::Sessions),
    ("PrevSession", "Previous session", Group::Sessions),
    ("SelectSession", "Go to session 1–9", Group::Sessions),
    ("NewWindow", "New window", Group::Sessions),
    ("RenameSession", "Rename session", Group::Sessions),
    ("RenameProject", "Rename project", Group::Sessions),
    ("EditConfig", "Settings", Group::Sessions),
    ("Quit", "Quit Kiln", Group::Sessions),
    ("SplitRight", "Split right", Group::Panes),
    ("ClosePane", "Close pane", Group::Panes),
    ("FocusNextPane", "Next pane", Group::Panes),
    ("FocusPrevPane", "Previous pane", Group::Panes),
    ("ToggleSidebar", "Sidebar", Group::View),
    ("ToggleRail", "Context rail", Group::View),
    ("FocusRail", "To the rail's changes and back", Group::View),
    ("ToggleFocusMode", "Focus mode", Group::View),
    ("ZoomIn", "Bigger text", Group::View),
    ("ZoomOut", "Smaller text", Group::View),
    ("ZoomReset", "Actual size", Group::View),
    (
        "SelectPrevCommand",
        "Select previous / next command",
        Group::CopyEdit,
    ),
    (
        "SelectNextCommand",
        "Select previous / next command",
        Group::CopyEdit,
    ),
    (
        "ExtendSelectionUp",
        "Add commands to the selection",
        Group::CopyEdit,
    ),
    (
        "ExtendSelectionDown",
        "Add commands to the selection",
        Group::CopyEdit,
    ),
    ("Copy", "Copy", Group::CopyEdit),
    ("Paste", "Paste", Group::CopyEdit),
    ("SelectAll", "Select all", Group::CopyEdit),
    ("CopyLastOutput", "Copy last output", Group::CopyEdit),
    (
        "CopyLastOutputRaw",
        "Copy last output, raw",
        Group::CopyEdit,
    ),
    ("Find", "Find", Group::FindPalette),
    (
        "CommandPaletteToggle",
        "Command palette",
        Group::FindPalette,
    ),
    ("ToggleShortcuts", "Keyboard shortcuts", Group::FindPalette),
    ("ScrollPageUp", "Page up", Group::Scroll),
    ("ScrollPageDown", "Page down", Group::Scroll),
    ("ScrollToTop", "To the top", Group::Scroll),
    ("ScrollToBottom", "To the bottom", Group::Scroll),
];

/// The described actions that have a binding in `style`'s keymap: (action, label).
pub fn bound_descriptions(style: KeyStyle) -> Vec<(&'static str, &'static str)> {
    let bound: Vec<&str> = bindings(style).iter().map(|b| b.action().name()).collect();
    DESCRIPTIONS
        .iter()
        .filter(|(name, _, _)| {
            bound
                .iter()
                .any(|action| position(action) == position(name))
        })
        .map(|(name, label, _)| (*name, *label))
        .collect()
}

/// Where an action sits in `DESCRIPTIONS`.
fn position(action: &str) -> Option<usize> {
    let short = action.rsplit("::").next().unwrap_or(action);
    let short = match short.strip_prefix("SelectSession") {
        Some(n) if n.parse::<u8>().is_ok() => "SelectSession",
        _ => short,
    };
    DESCRIPTIONS.iter().position(|(name, _, _)| *name == short)
}

/// A bound action's name in the pop-up and its section. `None` for actions Kiln does not
/// describe (a test makes sure every bound one is).
pub fn describe(action: &str) -> Option<(&'static str, Group)> {
    position(action).map(|i| (DESCRIPTIONS[i].1, DESCRIPTIONS[i].2))
}

/// A key's name: "D", "PgUp", "⇥" (macOS) or "Tab".
fn key_name(key: &str, style: KeyStyle) -> String {
    let mac = style == KeyStyle::Mac;
    let named = match key {
        "pageup" => "PgUp",
        "pagedown" => "PgDn",
        "home" => "Home",
        "end" => "End",
        "insert" => "Ins",
        "delete" => "Del",
        "space" => "Space",
        "tab" if mac => "⇥",
        "tab" => "Tab",
        "enter" if mac => "↩",
        "enter" => "Enter",
        "escape" => "Esc",
        "backspace" if mac => "⌫",
        "backspace" => "Backspace",
        "left" => "←",
        "right" => "→",
        "up" => "↑",
        "down" => "↓",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_string();
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) => c.to_uppercase().collect(),
        // F-keys and the like: "f5" → "F5".
        _ => {
            let mut s = key.to_string();
            if let Some(first) = s.get_mut(0..1) {
                first.make_ascii_uppercase();
            }
            s
        }
    }
}

/// One keystroke as written on its platform: "⌃⌥⇧⌘D" or "Ctrl+Alt+Shift+D".
pub fn format_keystroke(keystroke: &Keystroke, style: KeyStyle) -> String {
    let m = &keystroke.modifiers;
    let key = key_name(&keystroke.key, style);
    match style {
        // Apple's order: Control, Option, Shift, Command.
        KeyStyle::Mac => {
            let mut s = String::new();
            if m.control {
                s.push('⌃');
            }
            if m.alt {
                s.push('⌥');
            }
            if m.shift {
                s.push('⇧');
            }
            if m.platform {
                s.push('⌘');
            }
            s + &key
        }
        KeyStyle::Pc => {
            let mut parts = Vec::new();
            if m.control {
                parts.push("Ctrl");
            }
            if m.alt {
                parts.push("Alt");
            }
            if m.shift {
                parts.push("Shift");
            }
            if m.platform {
                parts.push("Super");
            }
            parts.push(&key);
            parts.join("+")
        }
    }
}

/// A binding's keys (several keystrokes in a row are separated by spaces).
pub fn format_chord<'a>(
    keystrokes: impl IntoIterator<Item = &'a Keystroke>,
    style: KeyStyle,
) -> String {
    keystrokes
        .into_iter()
        .map(|k| format_keystroke(k, style))
        .collect::<Vec<_>>()
        .join(" ")
}

/// One row of the pop-up: an action (or a run of numbered ones) and its keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShortcutRow {
    pub label: String,
    pub group: Group,
    /// One entry per binding, as written on the platform ("⌘V", "Shift+Ins").
    pub keys: Vec<String>,
    /// The actions behind the row ("kiln::SplitRight").
    pub actions: Vec<String>,
}

/// Rows from `(action name, keystrokes)` pairs, grouped and in the pop-up's order. Actions that
/// share a label (the numbered sessions) share a row, their keys written as a range ("⌘1–9").
pub fn rows<'a>(
    bindings: impl IntoIterator<Item = (&'a str, Vec<Keystroke>)>,
    style: KeyStyle,
) -> Vec<ShortcutRow> {
    let mut rows: Vec<ShortcutRow> = Vec::new();
    // Keystrokes of numbered actions, collected to write them as a range at the end.
    let mut ranges: Vec<(usize, Vec<Keystroke>)> = Vec::new();
    for (action, keystrokes) in bindings {
        let Some((label, group)) = describe(action) else {
            continue;
        };
        let index = match rows.iter().position(|r| r.label == label) {
            Some(index) => index,
            None => {
                rows.push(ShortcutRow {
                    label: label.to_string(),
                    group,
                    keys: Vec::new(),
                    actions: Vec::new(),
                });
                rows.len() - 1
            }
        };
        let row = &mut rows[index];
        if !row.actions.iter().any(|a| a == action) {
            row.actions.push(action.to_string());
        }
        if label.contains('–') && keystrokes.len() == 1 {
            match ranges.iter_mut().find(|(i, _)| *i == index) {
                Some((_, list)) => list.extend(keystrokes),
                None => ranges.push((index, keystrokes)),
            }
        } else {
            let chord = format_chord(&keystrokes, style);
            if !row.keys.contains(&chord) {
                row.keys.push(chord);
            }
        }
    }
    for (index, mut keystrokes) in ranges {
        keystrokes.sort_by(|a, b| a.key.cmp(&b.key));
        let (first, last) = (keystrokes.first().unwrap(), keystrokes.last().unwrap());
        let same_modifiers = keystrokes.iter().all(|k| k.modifiers == first.modifiers);
        let range = if same_modifiers && keystrokes.len() > 1 {
            let mut start = format_keystroke(first, style);
            start.push('–');
            start + &key_name(&last.key, style)
        } else {
            format_chord(&keystrokes, style)
        };
        rows[index].keys.push(range);
    }
    // The pop-up's order (sections, then the order in `DESCRIPTIONS`).
    rows.sort_by_key(|r| r.actions.first().and_then(|a| position(a)));
    rows
}

/// The shifted characters of the Ctrl+Shift bindings on punctuation and digits, keyed by the
/// key: what Windows reports instead (on US and UK layouts).
const SHIFTED: [(&str, &str); 8] = [
    ("/", "?"),
    (",", "<"),
    (".", ">"),
    ("=", "+"),
    ("-", "_"),
    ("0", ")"),
    ("]", "}"),
    ("[", "{"),
];

/// A binding's second form for Windows ("ctrl-?" for "ctrl-shift-/"), left out of what Kiln
/// shows.
pub fn is_layout_alias(keystrokes: &[Keystroke]) -> bool {
    keystrokes
        .iter()
        .any(|k| !k.modifiers.shift && SHIFTED.iter().any(|(_, shifted)| k.key == *shifted))
}

/// The rows for the keymap in effect.
pub fn live_rows(cx: &App, style: KeyStyle) -> Vec<ShortcutRow> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    rows(
        keymap
            .bindings()
            .filter(|b| {
                let keys: Vec<Keystroke> =
                    b.keystrokes().iter().map(|k| k.inner().clone()).collect();
                !is_layout_alias(&keys)
            })
            .map(|binding| {
                (
                    binding.action().name(),
                    binding
                        .keystrokes()
                        .iter()
                        .map(|k| k.inner().clone())
                        .collect(),
                )
            }),
        style,
    )
}

/// The keys for one action in the keymap in effect ("⌘K"), for the palette.
pub fn keys_for(action: &dyn Action, cx: &App, style: KeyStyle) -> String {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    keymap
        .bindings_for_action(action)
        .find(|b| {
            let keys: Vec<Keystroke> = b.keystrokes().iter().map(|k| k.inner().clone()).collect();
            !is_layout_alias(&keys)
        })
        .map(|binding| format_chord(binding.keystrokes().iter().map(|k| k.inner()), style))
        .unwrap_or_default()
}

/// Lowercase without spaces or "+", and with key names written the macOS way when that is the
/// style, so "cmd+d" finds "⌘D".
fn normalize(text: &str, style: KeyStyle) -> String {
    let mut s = text.to_lowercase().replace([' ', '+'], "");
    if style == KeyStyle::Mac {
        for (word, glyph) in [
            ("command", "⌘"),
            ("cmd", "⌘"),
            ("shift", "⇧"),
            ("option", "⌥"),
            ("opt", "⌥"),
            ("alt", "⌥"),
            ("control", "⌃"),
            ("ctrl", "⌃"),
        ] {
            s = s.replace(word, glyph);
        }
    }
    s
}

/// Whether a row matches the filter: by its name ("split") or its keys ("⌘D", "ctrl+shift+d").
pub fn matches(row: &ShortcutRow, query: &str, style: KeyStyle) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let label = row.label.to_lowercase();
    if label.contains(&query.to_lowercase()) {
        return true;
    }
    let wanted = normalize(query, style);
    !wanted.is_empty()
        && row
            .keys
            .iter()
            .any(|keys| normalize(keys, style).contains(&wanted))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows_for(style: KeyStyle) -> (Vec<KeyBinding>, Vec<ShortcutRow>) {
        let bindings: Vec<KeyBinding> = bindings(style)
            .into_iter()
            .filter(|b| !is_layout_alias(&keys_of(b)))
            .collect();
        let rows = rows(
            bindings.iter().map(|b| {
                (
                    b.action().name(),
                    b.keystrokes().iter().map(|k| k.inner().clone()).collect(),
                )
            }),
            style,
        );
        (bindings, rows)
    }

    #[test]
    fn every_bound_action_appears_once_with_its_keys() {
        for style in [KeyStyle::Mac, KeyStyle::Pc] {
            let (bindings, rows) = rows_for(style);
            for binding in &bindings {
                let name = binding.action().name();
                assert!(describe(name).is_some(), "{name} has no name in the pop-up");
                let holding: Vec<_> = rows
                    .iter()
                    .filter(|r| r.actions.iter().any(|a| a == name))
                    .collect();
                assert_eq!(
                    holding.len(),
                    1,
                    "{name} ({style:?}) is in {} rows",
                    holding.len()
                );
                let chord = format_chord(binding.keystrokes().iter().map(|k| k.inner()), style);
                let row = holding[0];
                let listed = row.keys.contains(&chord)
                    // A numbered action is inside its row's range ("⌘1–9").
                    || row.label.contains('–');
                assert!(listed, "{name}: {chord} not in {:?}", row.keys);
            }
        }
    }

    #[test]
    fn platform_only_bindings_stay_on_their_platform() {
        let (_, mac) = rows_for(KeyStyle::Mac);
        let (_, pc) = rows_for(KeyStyle::Pc);
        let keys = |rows: &[ShortcutRow], label: &str| {
            rows.iter()
                .find(|r| r.label == label)
                .map(|r| r.keys.clone())
        };
        assert_eq!(keys(&mac, "Go to session 1–9").unwrap(), ["⌘1–9"]);
        assert_eq!(keys(&pc, "Go to session 1–9").unwrap(), ["Ctrl+Alt+1–9"]);
        assert_eq!(keys(&mac, "Paste").unwrap(), ["⌘V"]);
        assert_eq!(keys(&pc, "Paste").unwrap(), ["Ctrl+Shift+V", "Shift+Ins"]);
        assert_eq!(keys(&mac, "Copy last output").unwrap(), ["⇧⌘C"]);
        assert_eq!(keys(&pc, "Copy last output"), None);
        assert_eq!(keys(&mac, "Command palette").unwrap(), ["⌘K"]);
        assert_eq!(keys(&pc, "Command palette").unwrap(), ["Ctrl+Shift+K"]);
        // ⌘P (Ctrl+Shift+P) is left unbound.
        for style in [KeyStyle::Mac, KeyStyle::Pc] {
            let p = bindings(style).into_iter().any(|b| {
                b.keystrokes().iter().any(|k| {
                    let k = k.inner();
                    k.key == "p" && (k.modifiers.platform || k.modifiers.control)
                })
            });
            assert!(!p, "{style:?}: something is bound to P");
        }
        assert_eq!(keys(&pc, "Next session").unwrap(), ["Ctrl+Tab"]);
        assert_eq!(
            keys(&pc, "Keyboard shortcuts").unwrap(),
            ["Ctrl+Shift+/", "F1"]
        );
    }

    fn keys_of(binding: &KeyBinding) -> Vec<Keystroke> {
        binding
            .keystrokes()
            .iter()
            .map(|k| k.inner().clone())
            .collect()
    }

    #[test]
    fn shifted_punctuation_also_binds_the_character_windows_reports() {
        let all = bindings(KeyStyle::Pc);
        for binding in &all {
            let keys = keys_of(binding);
            let [k] = keys.as_slice() else { continue };
            let Some((_, shifted)) = SHIFTED.iter().find(|(key, _)| k.key == *key) else {
                continue;
            };
            if !(k.modifiers.control && k.modifiers.shift) || k.modifiers.alt {
                continue;
            }
            let alias = all.iter().any(|other| {
                let other_keys = keys_of(other);
                other.action().name() == binding.action().name()
                    && other_keys.len() == 1
                    && other_keys[0].key == *shifted
                    && other_keys[0].modifiers.control
                    && !other_keys[0].modifiers.shift
            });
            assert!(alias, "{} has no ctrl-{shifted}", binding.action().name());
        }
    }

    #[test]
    fn keys_are_written_the_platform_way() {
        let k = |s: &str| Keystroke::parse(s).unwrap();
        assert_eq!(format_keystroke(&k("cmd-shift-c"), KeyStyle::Mac), "⇧⌘C");
        assert_eq!(
            format_keystroke(&k("ctrl-alt-shift-cmd-d"), KeyStyle::Mac),
            "⌃⌥⇧⌘D"
        );
        assert_eq!(
            format_keystroke(&k("ctrl-shift-d"), KeyStyle::Pc),
            "Ctrl+Shift+D"
        );
        assert_eq!(format_keystroke(&k("shift-pageup"), KeyStyle::Mac), "⇧PgUp");
        assert_eq!(
            format_keystroke(&k("shift-pageup"), KeyStyle::Pc),
            "Shift+PgUp"
        );
        assert_eq!(
            format_keystroke(&k("ctrl-shift-tab"), KeyStyle::Pc),
            "Ctrl+Shift+Tab"
        );
        assert_eq!(format_keystroke(&k("cmd-/"), KeyStyle::Mac), "⌘/");
        assert_eq!(
            format_chord([&k("ctrl-k"), &k("ctrl-s")], KeyStyle::Pc),
            "Ctrl+K Ctrl+S"
        );
    }

    #[test]
    fn the_filter_finds_names_and_keys() {
        let (_, mac) = rows_for(KeyStyle::Mac);
        let (_, pc) = rows_for(KeyStyle::Pc);
        let found = |rows: &[ShortcutRow], query: &str, style| -> Vec<String> {
            rows.iter()
                .filter(|r| matches(r, query, style))
                .map(|r| r.label.clone())
                .collect()
        };
        assert_eq!(found(&mac, "split", KeyStyle::Mac), ["Split right"]);
        assert_eq!(found(&mac, "⌘D", KeyStyle::Mac), ["Split right"]);
        assert_eq!(found(&mac, "cmd+d", KeyStyle::Mac), ["Split right"]);
        assert_eq!(found(&pc, "ctrl+shift+d", KeyStyle::Pc), ["Split right"]);
        assert_eq!(found(&pc, "Ctrl+Shift+D", KeyStyle::Pc), ["Split right"]);
        assert!(found(&mac, "", KeyStyle::Mac).len() == mac.len());
        assert!(found(&mac, "zzz", KeyStyle::Mac).is_empty());
    }
}
