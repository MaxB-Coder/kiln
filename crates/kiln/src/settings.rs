//! The Settings pop-up (⌘, or Ctrl+Shift+,): the app icon and the common settings, each one a
//! key in kiln.toml. A change applies at once and is written into the file with
//! `config_write`, which keeps the rest of the file as the user wrote it. Theme colours, the
//! shell and projects stay in the file ("Open config file").

use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, Context, Div, EventEmitter, FocusHandle, Focusable, Image, ImageFormat,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, div, img, px, relative,
};
use toml_edit::Value;

use crate::config::{self, AppIcon, Config, PromptStyle};
use crate::config_write::{self, WriteError};
use crate::grid_paint::FONT_FAMILY;
use crate::theme::{self, palette};

pub enum SettingsEvent {
    /// A setting was written; the config the file now holds.
    Changed(Box<Config>),
    OpenFile,
    Dismiss,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

const FONT_SIZES: (f32, f32) = (9., 24.);
const NOTIFY_STEP: u64 = 5;
const NOTIFY_MAX: u64 = 120;

/// One change, and the key it writes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Change {
    Icon(AppIcon),
    FontSize(f32),
    CursorBlink(bool),
    Sidebar(bool),
    Prompt(PromptStyle),
    Restore(bool),
    NotifyAfter(u64),
    TintRemote(bool),
    OptionAsAlt(bool),
    UpdatesCheck(bool),
}

impl Change {
    /// The key in kiln.toml and its new value.
    pub fn entry(self) -> (&'static [&'static str], Value) {
        match self {
            Change::Icon(icon) => (&["icon"], icon.key().into()),
            Change::FontSize(size) if size.fract() == 0. => (&["font_size"], (size as i64).into()),
            Change::FontSize(size) => (&["font_size"], f64::from(size).into()),
            Change::CursorBlink(on) => (&["cursor_blink"], on.into()),
            Change::Sidebar(on) => (&["window", "sidebar"], on.into()),
            Change::Prompt(PromptStyle::Kiln) => (&["shell", "prompt"], "kiln".into()),
            Change::Prompt(PromptStyle::Shell) => (&["shell", "prompt"], "shell".into()),
            Change::Restore(on) => (&["window", "restore"], on.into()),
            Change::NotifyAfter(secs) => (&["notify_after_secs"], (secs as i64).into()),
            Change::TintRemote(on) => (&["tints", "remote"], on.into()),
            Change::OptionAsAlt(on) => (&["option_as_alt"], on.into()),
            Change::UpdatesCheck(on) => (&["updates", "check"], on.into()),
        }
    }
}

/// The font size one step up or down: whole points, 9 to 24.
pub fn step_font_size(size: f32, up: bool) -> f32 {
    let next = if up {
        size.floor() + 1.
    } else {
        size.ceil() - 1.
    };
    next.clamp(FONT_SIZES.0, FONT_SIZES.1)
}

/// The notification delay one step up or down: multiples of 5 s, 0 is off.
pub fn step_notify(secs: u64, up: bool) -> u64 {
    if up {
        (secs / NOTIFY_STEP + 1) * NOTIFY_STEP
    } else {
        secs.saturating_sub(1) / NOTIFY_STEP * NOTIFY_STEP
    }
    .min(NOTIFY_MAX)
}

pub fn notify_label(secs: u64) -> String {
    if secs == 0 {
        "off".into()
    } else {
        format!("{secs} s")
    }
}

fn font_label(size: f32) -> String {
    if size.fract() == 0. {
        format!("{size:.0} pt")
    } else {
        format!("{size:.1} pt")
    }
}

/// The controls, in keyboard order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Icon,
    FontSize,
    CursorBlink,
    Sidebar,
    Prompt,
    Restore,
    NotifyAfter,
    TintRemote,
    OptionAsAlt,
    UpdatesCheck,
    OpenFile,
}

pub struct SettingsView {
    focus: FocusHandle,
    config: Config,
    /// The file has an error: shown, and nothing is written.
    broken: Option<String>,
    /// The last write failed (the file could not be saved).
    failed: Option<String>,
    /// Keyboard focus, shown once the keyboard is used.
    focused: usize,
    keyboard: bool,
    /// Option as Alt is a macOS setting.
    mac: bool,
    shortcut: String,
    tiles: Vec<(AppIcon, Arc<Image>)>,
}

impl SettingsView {
    pub fn new(mac: bool, shortcut: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let tiles = AppIcon::ALL
            .into_iter()
            .map(|icon| {
                let png = crate::app_icon::thumbnail(icon).to_vec();
                (icon, Arc::new(Image::from_bytes(ImageFormat::Png, png)))
            })
            .collect();
        let mut this = Self {
            focus,
            config: config::current(),
            broken: None,
            failed: None,
            focused: 0,
            keyboard: false,
            mac,
            shortcut,
            tiles,
        };
        this.reload();
        this
    }

    /// Reads the file again (it changed, or the pop-up opened): its values, or its error.
    pub fn reload(&mut self) {
        let text = config::config_path().and_then(|path| std::fs::read_to_string(path).ok());
        self.broken = text.as_deref().and_then(|t| config_write::check(t).err());
        if self.broken.is_none() {
            self.config = text
                .as_deref()
                .and_then(|t| config::parse(t).ok())
                .unwrap_or_else(config::current);
        }
    }

    fn controls(&self) -> Vec<Control> {
        let mut controls = vec![
            Control::Icon,
            Control::FontSize,
            Control::CursorBlink,
            Control::Sidebar,
            Control::Prompt,
            Control::Restore,
            Control::NotifyAfter,
            Control::TintRemote,
        ];
        if self.mac {
            controls.push(Control::OptionAsAlt);
        }
        controls.push(Control::UpdatesCheck);
        controls.push(Control::OpenFile);
        if self.broken.is_some() {
            controls.retain(|c| *c == Control::OpenFile);
        }
        controls
    }

    /// Writes one change into kiln.toml and applies it.
    pub fn set(&mut self, change: Change, cx: &mut Context<Self>) {
        if self.broken.is_some() {
            return;
        }
        let Some(path) = config::config_path() else {
            self.failed = Some("No folder for kiln.toml (HOME is not set)".into());
            cx.notify();
            return;
        };
        let (key, value) = change.entry();
        match config_write::write_setting(&path, key, value) {
            Ok(config) => {
                self.failed = None;
                self.config = config.clone();
                cx.emit(SettingsEvent::Changed(Box::new(config)));
            }
            Err(WriteError::Broken(message)) => self.broken = Some(message),
            Err(WriteError::Io(message)) => self.failed = Some(message),
        }
        cx.notify();
    }

    /// The change a control makes: Space or Enter (`step` 0), or an arrow (-1, +1).
    fn change_for(&self, control: Control, step: i32) -> Option<Change> {
        let c = &self.config;
        let flip = step == 0;
        Some(match control {
            Control::Icon => {
                let at = AppIcon::ALL.iter().position(|i| *i == c.icon).unwrap_or(0) as i32;
                let next = (at + if flip { 1 } else { step }).rem_euclid(3) as usize;
                Change::Icon(AppIcon::ALL[next])
            }
            Control::FontSize if !flip => Change::FontSize(step_font_size(c.font_size, step > 0)),
            Control::NotifyAfter if !flip => {
                Change::NotifyAfter(step_notify(c.notify_after_secs, step > 0))
            }
            Control::Prompt => Change::Prompt(match c.shell.prompt {
                PromptStyle::Kiln => PromptStyle::Shell,
                PromptStyle::Shell => PromptStyle::Kiln,
            }),
            Control::CursorBlink if flip => Change::CursorBlink(!c.cursor_blink),
            Control::Sidebar if flip => Change::Sidebar(!c.window.sidebar),
            Control::Restore if flip => Change::Restore(!c.window.restore),
            Control::TintRemote if flip => Change::TintRemote(!c.tints.remote),
            Control::OptionAsAlt if flip => Change::OptionAsAlt(!c.option_as_alt),
            Control::UpdatesCheck if flip => Change::UpdatesCheck(!c.updates.check),
            _ => return None,
        })
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let controls = self.controls();
        let count = controls.len();
        let current = controls[self.focused.min(count - 1)];
        let shift = event.keystroke.modifiers.shift;
        match event.keystroke.key.as_str() {
            "escape" => cx.emit(SettingsEvent::Dismiss),
            "tab" | "down" | "up" => {
                let back = shift || event.keystroke.key == "up";
                let at = self.focused.min(count - 1);
                self.focused = if !self.keyboard {
                    at
                } else if back {
                    (at + count - 1) % count
                } else {
                    (at + 1) % count
                };
                self.keyboard = true;
                cx.notify();
            }
            "left" | "right" => {
                self.keyboard = true;
                let step = if event.keystroke.key == "left" { -1 } else { 1 };
                if let Some(change) = self.change_for(current, step) {
                    self.set(change, cx);
                }
                cx.notify();
            }
            "space" | "enter" => {
                self.keyboard = true;
                if current == Control::OpenFile {
                    cx.emit(SettingsEvent::OpenFile);
                } else if let Some(change) = self.change_for(current, 0) {
                    self.set(change, cx);
                }
                cx.notify();
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    /// Whether the keyboard is on this control (its row is lit).
    fn lit(&self, control: Control) -> bool {
        let controls = self.controls();
        self.keyboard && controls.get(self.focused) == Some(&control)
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn section(title: &'static str) -> Div {
    let p = palette();
    div()
        .px(px(18.))
        .pt(px(11.))
        .pb(px(4.))
        .text_size(px(10.5))
        .text_color(theme::hsla(p.bg3))
        .child(title)
}

fn toggle(on: bool) -> Div {
    let p = palette();
    div()
        .relative()
        .flex_none()
        .w(px(30.))
        .h(px(17.))
        .rounded_full()
        .bg(theme::hsla(if on { p.accent } else { p.bg1 }))
        .child(
            div()
                .absolute()
                .top(px(2.))
                .left(px(if on { 15. } else { 2. }))
                .size(px(13.))
                .rounded_full()
                .bg(theme::hsla(if on { p.bar } else { p.ansi[7] })),
        )
}

impl SettingsView {
    /// A row: the label (and a quiet line under it) on the left, the control on the right.
    fn row(
        &self,
        control: Control,
        label: &'static str,
        sub: Option<&'static str>,
        right: impl IntoElement,
    ) -> Div {
        let p = palette();
        div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .px(px(18.))
            .py(px(5.))
            .text_color(theme::hsla(p.fg_dim))
            .when(self.lit(control), |d| d.bg(theme::hsla(p.bg)))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .child(label)
                    .when_some(sub, |d, sub| {
                        d.child(
                            div()
                                .mt(px(2.))
                                .text_size(px(10.5))
                                .text_color(theme::hsla(p.bg3))
                                .child(sub),
                        )
                    }),
            )
            .child(right)
    }

    fn switch(
        &self,
        id: &'static str,
        on: bool,
        change: Change,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        div()
            .id(id)
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.set(change, cx)))
            .child(toggle(on))
    }

    fn stepper(
        &self,
        id: &'static str,
        value: String,
        down: Change,
        up: Change,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = palette();
        let button = |id: (&'static str, usize), glyph: &'static str, change: Change| {
            div()
                .id(id)
                .size(px(20.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.))
                .bg(theme::hsla(p.bg))
                .text_color(theme::hsla(p.ansi[7]))
                .cursor_pointer()
                .hover(|s| s.text_color(theme::hsla(p.fg)))
                .on_click(cx.listener(move |this, _, _, cx| this.set(change, cx)))
                .child(glyph)
        };
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .text_color(theme::hsla(p.fg))
            .child(button((id, 0), "−", down))
            .child(
                div()
                    .min_w(px(40.))
                    .flex()
                    .justify_center()
                    .child(SharedString::from(value)),
            )
            .child(button((id, 1), "+", up))
    }

    fn segmented(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = palette();
        let current = self.config.shell.prompt;
        let option = |i: usize, label: &'static str, style: PromptStyle| {
            let on = current == style;
            div()
                .id(("prompt", i))
                .px(px(9.))
                .py(px(2.))
                .rounded(px(5.))
                .text_size(px(11.))
                .cursor_pointer()
                .text_color(theme::hsla(if on {
                    p.fg
                } else {
                    theme::blend(p.bg3, p.muted, 0.5)
                }))
                .when(on, |d| d.bg(theme::hsla(p.bg1)))
                .on_click(cx.listener(move |this, _, _, cx| this.set(Change::Prompt(style), cx)))
                .child(label)
        };
        div()
            .flex()
            .flex_row()
            .gap(px(2.))
            .p(px(2.))
            .rounded(px(7.))
            .bg(theme::hsla(p.bg))
            .child(option(0, "Kiln flame", PromptStyle::Kiln))
            .child(option(1, "Shell's own", PromptStyle::Shell))
    }

    fn icon_tiles(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = palette();
        let lit = self.lit(Control::Icon);
        div()
            .flex()
            .flex_row()
            .gap(px(14.))
            .px(px(18.))
            .py(px(4.))
            .when(lit, |d| d.bg(theme::hsla(p.bg)))
            .children(self.tiles.iter().enumerate().map(|(i, (icon, image))| {
                let icon = *icon;
                let on = self.config.icon == icon;
                div()
                    .id(("icon", i))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .text_size(px(10.5))
                    .text_color(theme::hsla(if on {
                        p.fg
                    } else {
                        theme::blend(p.bg3, p.muted, 0.5)
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| this.set(Change::Icon(icon), cx)))
                    // The ring sits 3 px outside the tile, as an outline would.
                    .child(
                        div()
                            .p(px(3.))
                            .rounded(px(17.))
                            .border_2()
                            .border_color(if on {
                                theme::hsla(p.accent)
                            } else {
                                gpui::transparent_black()
                            })
                            .child(img(image.clone()).size(px(60.)).rounded(px(14.))),
                    )
                    .child(icon.title())
            }))
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let c = self.config.clone();
        let max_h = f32::from(window.viewport_size().height) - 44.;
        let editable = self.broken.is_none();

        let mut body = div().flex().flex_col();
        body = body
            .child(section("App icon"))
            .child(self.icon_tiles(cx))
            .child(section("Appearance"))
            .child(self.row(
                Control::FontSize,
                "Font size",
                None,
                self.stepper(
                    "font-size",
                    font_label(c.font_size),
                    Change::FontSize(step_font_size(c.font_size, false)),
                    Change::FontSize(step_font_size(c.font_size, true)),
                    cx,
                ),
            ))
            .child(self.row(
                Control::CursorBlink,
                "Blinking cursor",
                None,
                self.switch(
                    "cursor-blink",
                    c.cursor_blink,
                    Change::CursorBlink(!c.cursor_blink),
                    cx,
                ),
            ))
            .child(self.row(
                Control::Sidebar,
                "Sidebar",
                None,
                self.switch(
                    "sidebar",
                    c.window.sidebar,
                    Change::Sidebar(!c.window.sidebar),
                    cx,
                ),
            ))
            .child(self.row(Control::Prompt, "Prompt", None, self.segmented(cx)))
            .child(section("Behaviour"))
            .child(self.row(
                Control::Restore,
                "Reopen sessions at launch",
                None,
                self.switch(
                    "restore",
                    c.window.restore,
                    Change::Restore(!c.window.restore),
                    cx,
                ),
            ))
            .child(self.row(
                Control::NotifyAfter,
                "Notify when a command finishes",
                Some("If it ran this long, out of sight"),
                self.stepper(
                    "notify",
                    notify_label(c.notify_after_secs),
                    Change::NotifyAfter(step_notify(c.notify_after_secs, false)),
                    Change::NotifyAfter(step_notify(c.notify_after_secs, true)),
                    cx,
                ),
            ))
            .child(self.row(
                Control::TintRemote,
                "Tint remote sessions",
                Some("Production hosts tint red"),
                self.switch(
                    "tint-remote",
                    c.tints.remote,
                    Change::TintRemote(!c.tints.remote),
                    cx,
                ),
            ))
            .when(self.mac, |d| {
                d.child(self.row(
                    Control::OptionAsAlt,
                    "Option key as Alt",
                    Some("macOS only"),
                    self.switch(
                        "option-as-alt",
                        c.option_as_alt,
                        Change::OptionAsAlt(!c.option_as_alt),
                        cx,
                    ),
                ))
            })
            .child(section("Updates"))
            .child(self.row(
                Control::UpdatesCheck,
                "Check for updates daily",
                Some("Kiln's only network use"),
                self.switch(
                    "updates",
                    c.updates.check,
                    Change::UpdatesCheck(!c.updates.check),
                    cx,
                ),
            ));

        let open_lit = self.lit(Control::OpenFile);
        div()
            .id("settings")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .w(px(470.))
            .max_w_full()
            .max_h(px(max_h))
            .flex()
            .flex_col()
            .pt(px(14.))
            .pb(px(10.))
            .rounded(px(12.))
            .bg(theme::hsla(p.bar))
            // A ring instead of a border.
            .shadow(vec![gpui::BoxShadow {
                color: theme::hsla(p.bg1),
                offset: gpui::point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            }])
            .font_family(FONT_FAMILY)
            .text_size(px(12.))
            // About a browser's normal line height, as in the mockup.
            .line_height(relative(1.3))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px(px(18.))
                    .pb(px(6.))
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::hsla(p.fg))
                            .child("Settings"),
                    )
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme::hsla(p.bg3))
                            .child("esc"),
                    ),
            )
            .when_some(self.broken.clone(), |d, error| {
                d.child(
                    div()
                        .mx(px(18.))
                        .my(px(6.))
                        .px(px(10.))
                        .py(px(8.))
                        .rounded(px(8.))
                        .bg(theme::hsla(p.bg))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .text_color(theme::hsla(p.failed))
                                .child("kiln.toml has an error, so Settings leaves it alone:"),
                        )
                        .child(
                            div()
                                .text_size(px(10.5))
                                .text_color(theme::hsla(p.fg_dim))
                                .child(SharedString::from(error)),
                        ),
                )
            })
            .child(
                div()
                    .id("settings-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(!editable, |d| d.opacity(0.4).occlude())
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .px(px(18.))
                    .pt(px(10.))
                    .pb(px(2.))
                    .mt(px(4.))
                    .text_size(px(10.5))
                    .text_color(theme::hsla(p.bg3))
                    .child(
                        div()
                            .id("open-config")
                            .text_color(theme::hsla(if open_lit { p.accent } else { p.ansi[7] }))
                            .hover(|s| s.text_color(theme::hsla(p.accent)))
                            .cursor_pointer()
                            .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile)))
                            .child("Open config file"),
                    )
                    .child(SharedString::from(self.shortcut.clone())),
            )
            .when_some(self.failed.clone(), |d, error| {
                d.child(
                    div()
                        .px(px(18.))
                        .pt(px(4.))
                        .text_size(px(10.5))
                        .text_color(theme::hsla(p.failed))
                        .child(SharedString::from(format!("Not saved: {error}"))),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steppers_move_in_whole_steps_within_their_range() {
        assert_eq!(step_font_size(13., true), 14.);
        assert_eq!(step_font_size(13.5, true), 14.);
        assert_eq!(step_font_size(13.5, false), 13.);
        assert_eq!(step_font_size(24., true), 24.);
        assert_eq!(step_font_size(9., false), 9.);
        assert_eq!(step_notify(10, true), 15);
        assert_eq!(step_notify(7, true), 10);
        assert_eq!(step_notify(7, false), 5);
        assert_eq!(step_notify(5, false), 0);
        assert_eq!(step_notify(0, false), 0);
        assert_eq!(step_notify(120, true), 120);
        assert_eq!(notify_label(0), "off");
        assert_eq!(notify_label(10), "10 s");
    }

    #[test]
    fn each_change_writes_its_own_key() {
        let text = config::TEMPLATE;
        for (change, check) in [
            (
                Change::Icon(AppIcon::White),
                Box::new(|c: &Config| c.icon == AppIcon::White) as Box<dyn Fn(&Config) -> bool>,
            ),
            (
                Change::FontSize(15.),
                Box::new(|c: &Config| c.font_size == 15.),
            ),
            (
                Change::CursorBlink(false),
                Box::new(|c: &Config| !c.cursor_blink),
            ),
            (
                Change::Sidebar(false),
                Box::new(|c: &Config| !c.window.sidebar),
            ),
            (
                Change::Prompt(PromptStyle::Shell),
                Box::new(|c: &Config| c.shell.prompt == PromptStyle::Shell),
            ),
            (
                Change::Restore(false),
                Box::new(|c: &Config| !c.window.restore),
            ),
            (
                Change::NotifyAfter(0),
                Box::new(|c: &Config| c.notify_after_secs == 0),
            ),
            (
                Change::TintRemote(false),
                Box::new(|c: &Config| !c.tints.remote),
            ),
            (
                Change::OptionAsAlt(true),
                Box::new(|c: &Config| c.option_as_alt),
            ),
            (
                Change::UpdatesCheck(false),
                Box::new(|c: &Config| !c.updates.check),
            ),
        ] {
            let (key, value) = change.entry();
            let new = config_write::with_setting(text, key, value).unwrap();
            let config = config::parse(&new).unwrap();
            assert!(check(&config), "{change:?}");
            // Nothing else changed.
            let lines = |t: &str| t.lines().map(str::to_string).collect::<Vec<_>>();
            let changed = lines(text)
                .iter()
                .zip(lines(&new))
                .filter(|(a, b)| *a != b)
                .count();
            assert_eq!(changed, 1, "{change:?}");
        }
    }
}
