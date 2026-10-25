//! The Settings pop-up (⌘, or Ctrl+Shift+,): the app icon and the common settings, each one a
//! key in kiln.toml. A change applies at once and is written into the file with
//! `config_write`, which keeps the rest of the file as the user wrote it. Theme colours, the
//! shell and projects stay in the file ("Open config file").

use std::sync::Arc;

use gpui::AppContext as _;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Bounds, Context, Div, Entity, EventEmitter, FocusHandle, Focusable, Image,
    ImageFormat, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, Pixels, Render, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Window, canvas, div, img, linear_color_stop, linear_gradient, px,
    relative,
};
use toml_edit::Value;

use crate::config::{self, Config, IconColour, IconDesign, PromptStyle};
use crate::config_write::{self, WriteError};
use crate::grid_paint::FONT_FAMILY;
use crate::settings_theme::{self as themes, CARDS, COLOURS};
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{self, palette};

pub enum SettingsEvent {
    /// A setting was written; the config the file now holds.
    Changed(Box<Config>),
    OpenFile,
    Dismiss,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

const PANEL_W: f32 = 470.;
const FONT_SIZES: (f32, f32) = (9., 24.);
const NOTIFY_STEP: u64 = 5;
const NOTIFY_MAX: u64 = 120;

/// One change, and the key it writes.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// The icon's design.
    Icon(IconDesign),
    /// One design's colour (each keeps its own).
    IconColour(IconDesign, IconColour),
    /// A custom image, already copied into Kiln's folder (`custom_icon::import`).
    CustomIcon(std::path::PathBuf),
    /// Back to the Kiln design, forgetting the image.
    RemoveCustomIcon,
    FontSize(f32),
    CursorBlink(bool),
    Sidebar(bool),
    Prompt(PromptStyle),
    Restore(bool),
    NotifyAfter(u64),
    TintRemote(bool),
    OptionAsAlt(bool),
    UpdatesCheck(bool),
    /// A theme by name (presets and "system"), exactly as it is.
    Theme(&'static str),
    /// Back to the custom theme kept in `[theme.custom]`.
    SelectCustom,
    /// Custom, starting from a preset: `[theme.custom]` gets its base and every colour, so the
    /// file says it all.
    CustomFrom(&'static str),
    /// An interface colour (index into `COLOURS`) or a terminal colour of the custom theme.
    Colour(usize, u32),
    Ansi(usize, u32),
}

/// A change as key paths and values (`None` removes the key).
type Edits = Vec<(Vec<&'static str>, Option<Value>)>;

fn ansi_value(ansi: &[u32; 16]) -> Value {
    let array: toml_edit::Array = ansi.iter().map(|c| themes::hex(*c)).collect();
    array.into()
}

/// Colours right in `[theme]` (from before `[theme.custom]`) move into `[theme.custom]` when it
/// does not exist yet, and leave `[theme]`, so whichever theme is chosen is exactly itself.
fn tidy_theme(config: &Config) -> Edits {
    let t = &config.theme;
    let mut edits: Edits = Vec::new();
    if t.custom.is_none() && t.colours.any() {
        edits.push((
            vec!["theme", "custom", "base"],
            Some(t.custom_base().to_string().into()),
        ));
        let mut colours = t.colours.clone();
        for (i, (key, _)) in COLOURS.iter().enumerate() {
            if let Some(value) = themes::override_slot(&mut colours, i).clone() {
                edits.push((vec!["theme", "custom", key], Some(value.into())));
            }
        }
        if let Some(ansi) = &t.colours.ansi {
            let array: toml_edit::Array = ansi.iter().cloned().collect();
            edits.push((vec!["theme", "custom", "ansi"], Some(array.into())));
        }
    }
    for (key, _) in COLOURS {
        edits.push((vec!["theme", key], None));
    }
    edits.push((vec!["theme", "ansi"], None));
    edits.push((vec!["theme", "base"], None));
    edits
}

/// `icon = "black"` and the like (from before the designs) become the Kiln design with that
/// colour the first time Settings writes the file.
fn tidy_icon(config: &Config) -> Edits {
    let Some(colour) = config.icon.old_colour else {
        return Vec::new();
    };
    let mut edits: Edits = vec![(vec!["icon"], Some(IconDesign::Kiln.key().into()))];
    if config.icon_colours.kiln.is_none() {
        edits.push((vec!["icon_colours", "kiln"], Some(colour.key().into())));
    }
    edits
}

impl Change {
    /// What the change writes, given the config it changes.
    pub fn edits(self, config: &Config) -> Edits {
        let mut edits = tidy_icon(config);
        edits.extend(self.own_edits(config));
        edits
    }

    fn own_edits(self, config: &Config) -> Edits {
        match self {
            Change::IconColour(design, colour) => vec![(
                vec!["icon_colours", design.key()],
                Some(colour.key().into()),
            )],
            Change::CustomIcon(path) => vec![
                (vec!["icon"], Some(IconDesign::Custom.key().into())),
                (
                    vec!["icon_path"],
                    Some(path.to_string_lossy().into_owned().into()),
                ),
            ],
            Change::RemoveCustomIcon => vec![
                (vec!["icon"], Some(IconDesign::Kiln.key().into())),
                (vec!["icon_path"], None),
            ],
            Change::Theme(name) => {
                let mut edits = tidy_theme(config);
                edits.push((vec!["theme", "name"], Some(name.into())));
                edits
            }
            Change::SelectCustom => {
                let mut edits = tidy_theme(config);
                edits.push((vec!["theme", "name"], Some(theme::CUSTOM.into())));
                edits
            }
            Change::CustomFrom(base) => {
                let p = theme::preset_by_id(base).map_or(theme::DEFAULT_PALETTE, |p| p.palette);
                let mut edits: Edits = COLOURS
                    .iter()
                    .map(|(key, _)| (vec!["theme", *key], None))
                    .chain([(vec!["theme", "ansi"], None), (vec!["theme", "base"], None)])
                    .collect();
                edits.push((vec!["theme", "name"], Some(theme::CUSTOM.into())));
                edits.push((vec!["theme", "custom", "base"], Some(base.into())));
                for (i, (key, _)) in COLOURS.iter().enumerate() {
                    edits.push((
                        vec!["theme", "custom", key],
                        Some(themes::hex(themes::slot(&p, i)).into()),
                    ));
                }
                edits.push((vec!["theme", "custom", "ansi"], Some(ansi_value(&p.ansi))));
                edits
            }
            Change::Colour(i, color) => {
                vec![(
                    vec!["theme", "custom", COLOURS[i].0],
                    Some(themes::hex(color).into()),
                )]
            }
            Change::Ansi(i, color) => {
                let mut ansi = config.theme.custom_palette().ansi;
                ansi[i] = color;
                vec![(vec!["theme", "custom", "ansi"], Some(ansi_value(&ansi)))]
            }
            simple => {
                let (key, value) = simple.entry();
                vec![(key.to_vec(), Some(value))]
            }
        }
    }

    /// The key in kiln.toml and its new value, for the one-key settings.
    pub fn entry(self) -> (&'static [&'static str], Value) {
        match self {
            Change::Icon(design) => (&["icon"], design.key().into()),
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
            _ => unreachable!("theme changes write several keys (see `edits`)"),
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
    Theme,
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
    /// Every drawn design in every colour (rows by design, columns by colour).
    tiles: Vec<Vec<Arc<Image>>>,
    /// The custom image's tile: its copy's path, the picture and its size in px.
    custom_tile: Option<(std::path::PathBuf, Arc<Image>, u32)>,
    /// The colour pop-up under the selected design is open.
    icon_menu: bool,
    /// Why the chosen image cannot be used.
    icon_error: Option<String>,
    /// The custom theme's hex fields, one per interface colour.
    hex: Vec<Entity<TextInput>>,
    _hex_events: Vec<Subscription>,
    /// "Start from" is open.
    base_menu: bool,
    picker: Option<Picker>,
}

/// The colour picker under the custom theme's colours.
#[derive(Clone, Copy, Debug)]
struct Picker {
    target: Target,
    hsv: (f32, f32, f32),
    dragging: Option<Area>,
    square: Option<Bounds<Pixels>>,
    strip: Option<Bounds<Pixels>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Colour(usize),
    Ansi(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Area {
    Square,
    Strip,
}

impl SettingsView {
    pub fn new(mac: bool, shortcut: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let tiles = IconDesign::DRAWN
            .into_iter()
            .map(|design| {
                IconColour::ALL
                    .into_iter()
                    .map(|colour| {
                        let png = crate::app_icon::thumbnail(design, colour).to_vec();
                        Arc::new(Image::from_bytes(ImageFormat::Png, png))
                    })
                    .collect()
            })
            .collect();
        let mut hex = Vec::new();
        let mut hex_events = Vec::new();
        for i in 0..COLOURS.len() {
            let input = cx.new(|cx| TextInput::new("#rrggbb", cx).with_size(px(10.5)));
            hex_events.push(cx.subscribe_in(
                &input,
                window,
                move |this, input, event, window, cx| match event {
                    TextInputEvent::Changed => {
                        let text = input.read(cx).text().trim().to_string();
                        if let Some(color) = config::parse_hex(&text)
                            && color != themes::slot(&this.config.palette(), i)
                        {
                            this.set(Change::Colour(i, color), cx);
                        }
                    }
                    TextInputEvent::Cancel | TextInputEvent::Submit { .. } => {
                        window.focus(&this.focus, cx);
                    }
                    _ => {}
                },
            ));
            hex.push(input);
        }
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
            custom_tile: None,
            icon_menu: false,
            icon_error: None,
            hex,
            _hex_events: hex_events,
            base_menu: false,
            picker: None,
        };
        this.reload_and_sync(cx);
        this
    }

    /// `reload`, then the hex fields show the colours.
    pub fn reload_and_sync(&mut self, cx: &mut Context<Self>) {
        self.reload();
        self.sync_hex(cx);
    }

    /// The hex fields follow the colours (a field being typed in only changes once its text is
    /// a whole colour, which it then already shows).
    fn sync_hex(&mut self, cx: &mut Context<Self>) {
        let p = self.config.palette();
        for (i, input) in self.hex.iter().enumerate() {
            let text = themes::hex(themes::slot(&p, i));
            input.update(cx, |input, cx| input.set_text(&text, cx));
        }
    }

    /// The theme in effect.
    fn theme_name(&self) -> &str {
        self.config.theme.effective_name()
    }

    /// The preset a new custom theme starts from: the theme on screen.
    fn start_from(&self) -> &'static str {
        match theme::resolve_name(self.theme_name(), None, theme::system_dark()) {
            Some(preset) => preset.id,
            None => "kiln-dark",
        }
    }

    /// Choosing Custom: the one kept in the file, or a new one from the theme on screen.
    fn custom_change(&self) -> Change {
        if self.config.theme.has_custom() {
            Change::SelectCustom
        } else {
            Change::CustomFrom(self.start_from())
        }
    }

    pub fn open_picker(&mut self, target: Target, cx: &mut Context<Self>) {
        if self.picker.is_some_and(|p| p.target == target) {
            self.picker = None;
        } else {
            let p = self.config.palette();
            let color = match target {
                Target::Colour(i) => themes::slot(&p, i),
                Target::Ansi(i) => p.ansi[i],
            };
            self.picker = Some(Picker {
                target,
                hsv: themes::to_hsv(color),
                dragging: None,
                square: None,
                strip: None,
            });
        }
        cx.notify();
    }

    /// A press or drag in the picker: the colour under the pointer, applied (not yet saved).
    fn pick(&mut self, area: Area, at: gpui::Point<Pixels>, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        let bounds = match area {
            Area::Square => picker.square,
            Area::Strip => picker.strip,
        };
        let Some(b) = bounds else {
            return;
        };
        let fx = (f32::from(at.x - b.origin.x) / f32::from(b.size.width)).clamp(0., 1.);
        let fy = (f32::from(at.y - b.origin.y) / f32::from(b.size.height)).clamp(0., 1.);
        let (hue, sat, val) = picker.hsv;
        picker.hsv = match area {
            Area::Square => (hue, fx, 1. - fy),
            Area::Strip => (fx * 359.9, sat, val),
        };
        picker.dragging = Some(area);
        let (target, (h, s2, v)) = (picker.target, picker.hsv);
        self.preview(target, themes::from_hsv(h, s2, v), cx);
    }

    /// Shows a colour everywhere without writing it (while the picker is dragged).
    fn preview(&mut self, target: Target, color: u32, cx: &mut Context<Self>) {
        let mut config = self.config.clone();
        match target {
            Target::Colour(i) => {
                let custom = config.theme.custom.get_or_insert_with(Default::default);
                *themes::override_slot(&mut custom.colours, i) = Some(themes::hex(color))
            }
            Target::Ansi(i) => {
                let mut ansi = config.theme.custom_palette().ansi;
                ansi[i] = color;
                let custom = config.theme.custom.get_or_insert_with(Default::default);
                custom.colours.ansi = Some(ansi.iter().map(|c| themes::hex(*c)).collect());
            }
        }
        self.config = config.clone();
        self.sync_hex(cx);
        cx.emit(SettingsEvent::Changed(Box::new(config)));
        cx.notify();
    }

    /// The drag ended: the picked colour is written.
    fn end_drag(&mut self, cx: &mut Context<Self>) {
        let Some(picker) = self.picker.as_mut() else {
            return;
        };
        if picker.dragging.take().is_none() {
            return;
        }
        let (h, s2, v) = picker.hsv;
        let color = themes::from_hsv(h, s2, v);
        let change = match picker.target {
            Target::Colour(i) => Change::Colour(i, color),
            Target::Ansi(i) => Change::Ansi(i, color),
        };
        self.set(change, cx);
    }

    pub fn choose_theme(&mut self, name: &'static str, cx: &mut Context<Self>) {
        if name == self.theme_name() {
            return;
        }
        self.picker = None;
        if name == theme::CUSTOM {
            self.set(self.custom_change(), cx);
        } else {
            self.set(Change::Theme(name), cx);
        }
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
            Control::Theme,
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
        let edits = change.edits(&self.config);
        let edits: Vec<config_write::Edit> = edits
            .iter()
            .map(|(key, value)| (key.as_slice(), value.clone()))
            .collect();
        match config_write::write_edits(&path, &edits) {
            Ok(config) => {
                self.failed = None;
                self.config = config.clone();
                self.sync_hex(cx);
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
            // With the colour pop-up open, the arrows change the colour.
            Control::Icon if self.icon_menu && !flip => {
                let design = c.icon.design;
                let colour = c.icon_colour(design);
                let at = IconColour::ALL
                    .iter()
                    .position(|x| *x == colour)
                    .unwrap_or(0) as i32;
                let next = (at + step).rem_euclid(IconColour::ALL.len() as i32) as usize;
                Change::IconColour(design, IconColour::ALL[next])
            }
            // The arrows move between the designs (Custom once it has an image).
            Control::Icon if !flip => {
                let designs: Vec<IconDesign> = IconDesign::ALL
                    .into_iter()
                    .filter(|d| *d != IconDesign::Custom || c.icon_path.is_some())
                    .collect();
                let at = designs
                    .iter()
                    .position(|d| *d == c.icon.design)
                    .unwrap_or(0) as i32;
                Change::Icon(designs[(at + step).rem_euclid(designs.len() as i32) as usize])
            }
            Control::Theme => {
                let name = c.theme.effective_name();
                let at = CARDS.iter().position(|(id, _)| *id == name).unwrap_or(1) as i32;
                let next = CARDS
                    [(at + if flip { 1 } else { step }).rem_euclid(CARDS.len() as i32) as usize]
                    .0;
                if next == theme::CUSTOM {
                    self.custom_change()
                } else {
                    Change::Theme(next)
                }
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
            "escape" if self.icon_menu => {
                self.icon_menu = false;
                cx.notify();
            }
            "escape" => cx.emit(SettingsEvent::Dismiss),
            "tab" | "down" | "up" => {
                self.icon_menu = false;
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
                } else if current == Control::Icon {
                    // The selected design's colours, or a new image for Custom.
                    if self.config.icon.design == IconDesign::Custom {
                        self.choose_image(cx);
                    } else {
                        self.icon_menu = !self.icon_menu;
                    }
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
            .on_click(cx.listener(move |this, _, _, cx| this.set(change.clone(), cx)))
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
                .on_click(cx.listener(move |this, _, _, cx| this.set(change.clone(), cx)))
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

    /// Opens the system's file picker for a custom icon.
    fn choose_image(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Use as icon".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            let _ = this.update(cx, |this, cx| this.use_image(path, cx));
        })
        .detach();
    }

    /// Checks `source` and, if it can be an icon, saves Kiln's copy and uses it.
    pub fn use_image(&mut self, source: std::path::PathBuf, cx: &mut Context<Self>) {
        let Some(folder) = crate::custom_icon::folder() else {
            self.icon_error = Some("No folder for kiln.toml (HOME is not set)".into());
            cx.notify();
            return;
        };
        let task = cx.background_spawn({
            let folder = folder.clone();
            async move { crate::custom_icon::import(&source, &folder) }
        });
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(copy) => {
                    this.icon_error = None;
                    this.icon_menu = false;
                    let old = this.config.icon_path.clone();
                    this.set(Change::CustomIcon(copy.clone()), cx);
                    if let Some(old) = old.filter(|old| *old != copy) {
                        crate::custom_icon::remove_copy(&old, &folder);
                    }
                }
                Err(rejected) => {
                    this.icon_error = Some(rejected.to_string());
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Back to the Kiln design; Kiln's copy of the image goes.
    pub fn remove_image(&mut self, cx: &mut Context<Self>) {
        let old = self.config.icon_path.clone();
        self.icon_error = None;
        self.set(Change::RemoveCustomIcon, cx);
        if let (Some(old), Some(folder)) = (old, crate::custom_icon::folder()) {
            crate::custom_icon::remove_copy(&old, &folder);
        }
    }

    /// The custom image's tile, made again when the image changes.
    fn custom_tile(&mut self) -> Option<(Arc<Image>, u32)> {
        let path = self.config.icon_path.clone()?;
        if self.custom_tile.as_ref().map(|(p, _, _)| p) != Some(&path) {
            self.custom_tile = match crate::app_icon::custom_thumbnail(&path) {
                Ok((png, side)) => Some((
                    path,
                    Arc::new(Image::from_bytes(ImageFormat::Png, png)),
                    side,
                )),
                Err(err) => {
                    log::warn!("the custom icon: {err}");
                    None
                }
            };
        }
        self.custom_tile
            .as_ref()
            .map(|(_, image, side)| (image.clone(), *side))
    }

    /// Clicking a tile: another design is chosen and its colours shown; the chosen one shows
    /// or hides them. Custom asks for an image when it has none or is already chosen.
    pub fn click_icon(&mut self, design: IconDesign, cx: &mut Context<Self>) {
        let current = self.config.icon.design;
        if design == IconDesign::Custom {
            self.icon_menu = false;
            if current == IconDesign::Custom || self.config.icon_path.is_none() {
                self.choose_image(cx);
            } else {
                self.set(Change::Icon(IconDesign::Custom), cx);
            }
            return;
        }
        if design == current {
            self.icon_menu = !self.icon_menu;
            cx.notify();
        } else {
            self.icon_menu = true;
            self.set(Change::Icon(design), cx);
        }
    }

    fn icon_tiles(&mut self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = palette();
        let lit = self.lit(Control::Icon);
        let selected = match self.config.app_icon() {
            config::AppIcon::Custom(_) => IconDesign::Custom,
            config::AppIcon::Drawn(design, _) => design,
        };
        let custom = self.custom_tile();
        const TILE: f32 = 60.;
        let label = |on: bool| {
            if on {
                p.fg
            } else {
                theme::blend(p.bg3, p.muted, 0.5)
            }
        };
        // The ring sits 3 px outside the tile, as an outline would.
        let ring = |on: bool, tile: AnyElement| {
            div()
                .relative()
                .p(px(3.))
                .rounded(px(17.))
                .border_2()
                .border_color(if on {
                    theme::hsla(p.accent)
                } else {
                    gpui::transparent_black()
                })
                .child(tile)
        };
        let mut row = div()
            .flex()
            .flex_row()
            .justify_between()
            .px(px(18.))
            .py(px(4.));
        for (i, design) in IconDesign::DRAWN.into_iter().enumerate() {
            let on = selected == design;
            let colour = self.config.icon_colour(design);
            let column = IconColour::ALL
                .iter()
                .position(|c| *c == colour)
                .unwrap_or(0);
            let image = self.tiles[i][column].clone();
            let (top, bottom) = colour.swatch();
            let badge = div()
                .absolute()
                .right(px(-5.))
                .bottom(px(-5.))
                .size(px(18.))
                .rounded_full()
                .bg(theme::hsla(p.raised))
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(12.))
                        .rounded_full()
                        .border_1()
                        .border_color(gpui::white().opacity(0.25))
                        .bg(linear_gradient(
                            180.,
                            linear_color_stop(theme::hsla(top), 0.),
                            linear_color_stop(theme::hsla(bottom), 1.),
                        )),
                );
            let tile = div()
                .relative()
                .child(img(image).size(px(TILE)).rounded(px(14.)))
                .when(on, |d| d.child(badge))
                .into_any_element();
            row = row.child(
                div()
                    .id(("icon", i))
                    .w(px(76.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .text_size(px(10.5))
                    .text_color(theme::hsla(label(on)))
                    .on_click(cx.listener(move |this, _, _, cx| this.click_icon(design, cx)))
                    .child(ring(on, tile))
                    .child(if on {
                        format!("{} ▾", design.title())
                    } else {
                        design.title().to_string()
                    }),
            );
        }
        // Custom: the image, or a dashed "+" until there is one.
        let on = selected == IconDesign::Custom;
        let tile = match &custom {
            Some((image, _)) => img(image.clone())
                .size(px(TILE))
                .rounded(px(14.))
                .into_any_element(),
            None => div()
                .size(px(TILE))
                .rounded(px(14.))
                .border_2()
                .border_dashed()
                .border_color(theme::hsla(p.bg2))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(22.))
                .text_color(theme::hsla(label(false)))
                .child("+")
                .into_any_element(),
        };
        row = row.child(
            div()
                .id(("icon", IconDesign::DRAWN.len()))
                .w(px(76.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.))
                .cursor_pointer()
                .text_size(px(10.5))
                .text_color(theme::hsla(label(on)))
                .on_click(cx.listener(|this, _, _, cx| this.click_icon(IconDesign::Custom, cx)))
                .child(ring(on, tile))
                .child(IconDesign::Custom.title()),
        );

        let menu = (self.icon_menu && selected != IconDesign::Custom).then(|| {
            let at = IconDesign::DRAWN
                .iter()
                .position(|d| *d == selected)
                .unwrap_or(0);
            let current = self.config.icon_colour(selected);
            // Under its tile, kept inside the panel.
            let width = 7. * 58. + 24.;
            let left = (at as f32 * 95. - 2.).clamp(4., PANEL_W - width - 4.);
            div()
                .id("icon-colours")
                .ml(px(left))
                .mt(px(8.))
                .w(px(width))
                .px(px(12.))
                .py(px(10.))
                .rounded(px(10.))
                .bg(theme::hsla(p.raised))
                .border_1()
                .border_color(theme::hsla(p.bg1))
                .shadow(vec![gpui::BoxShadow {
                    color: p.shadow(),
                    offset: gpui::point(px(0.), px(10.)),
                    blur_radius: px(30.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.icon_menu = false;
                    cx.notify();
                }))
                .child(
                    div()
                        .mb(px(8.))
                        .text_size(px(10.))
                        .text_color(theme::hsla(p.muted))
                        .child("Colour"),
                )
                .child(div().flex().flex_row().children(
                    IconColour::ALL.into_iter().enumerate().map(|(i, colour)| {
                        let on = colour == current;
                        let (top, bottom) = colour.swatch();
                        // Equal columns, the swatch centred and its name below.
                        div()
                            .id(("icon-colour", i))
                            .w(px(58.))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(5.))
                            .cursor_pointer()
                            .text_size(px(9.5))
                            .text_color(theme::hsla(if on { p.fg } else { p.fg_dim }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set(Change::IconColour(selected, colour), cx)
                            }))
                            .child(
                                div()
                                    .p(px(2.))
                                    .rounded_full()
                                    .border_2()
                                    .border_color(if on {
                                        theme::hsla(p.accent)
                                    } else {
                                        gpui::transparent_black()
                                    })
                                    .child(div().size(px(24.)).rounded_full().bg(linear_gradient(
                                        180.,
                                        linear_color_stop(theme::hsla(top), 0.),
                                        linear_color_stop(theme::hsla(bottom), 1.),
                                    ))),
                            )
                            .child(colour.title())
                    }),
                ))
        });

        // The line under the row.
        let link = |id: &'static str, text: &'static str| {
            div()
                .id(id)
                .cursor_pointer()
                .text_color(theme::hsla(p.accent))
                .child(text)
        };
        let note = div()
            .px(px(18.))
            .pt(px(8.))
            .flex()
            .flex_row()
            .gap(px(10.))
            .text_size(px(10.5))
            .text_color(theme::hsla(p.bg3));
        let note = if let Some(error) = &self.icon_error {
            note.child(div().text_color(theme::hsla(p.failed)).child(error.clone()))
        } else if selected == IconDesign::Custom
            && let (Some((_, side)), Some(path)) = (&custom, &self.config.icon_path)
        {
            note.child(format!(
                "Using {} · {side}×{side}",
                crate::custom_icon::display_name(path)
            ))
            .child(
                link("icon-another", "Choose another…")
                    .on_click(cx.listener(|this, _, _, cx| this.choose_image(cx))),
            )
            .child(
                link("icon-remove", "Remove")
                    .on_click(cx.listener(|this, _, _, cx| this.remove_image(cx))),
            )
        } else if self.icon_menu {
            note.child(format!(
                "Colour for {}. Each icon keeps its own colour.",
                selected.title()
            ))
        } else {
            note.child("Click the selected icon for its colours.")
        };

        div()
            .flex()
            .flex_col()
            .pb(px(4.))
            .when(lit, |d| d.bg(theme::hsla(p.bg)))
            .child(row)
            .children(menu)
            .child(note)
    }
}

impl SettingsView {
    fn theme_cards(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = palette();
        let current = self.theme_name().to_string();
        // Custom's card: the custom colours, or the theme a new one would start from.
        let custom = if self.config.theme.has_custom() {
            self.config.theme.custom_palette()
        } else {
            theme::preset_by_id(self.start_from()).map_or(theme::DEFAULT_PALETTE, |p| p.palette)
        };
        let lit = self.lit(Control::Theme);
        let card = |i: usize, id: &'static str, title: &'static str, cx: &mut Context<Self>| {
            let on = current == id;
            div()
                .id(("theme", i))
                .w(px(themes::CARD_W))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.))
                .cursor_pointer()
                .text_size(px(10.))
                .text_color(theme::hsla(if on { p.fg } else { p.muted }))
                .whitespace_nowrap()
                .on_click(cx.listener(move |this, _, _, cx| this.choose_theme(id, cx)))
                .child(
                    div()
                        .relative()
                        .w(px(themes::CARD_W))
                        .h(px(themes::CARD_H))
                        .child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, _, window, _| {
                                    themes::paint_card(id, &custom, bounds, window)
                                },
                            )
                            .size_full(),
                        )
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(px(8.))
                                .border_1()
                                .border_color(theme::hsla(p.bg1)),
                        )
                        .when(on, |d| {
                            // The ring sits 3 px outside the card, as an outline would.
                            d.child(
                                div()
                                    .absolute()
                                    .top(px(-5.))
                                    .left(px(-5.))
                                    .right(px(-5.))
                                    .bottom(px(-5.))
                                    .rounded(px(13.))
                                    .border_2()
                                    .border_color(theme::hsla(p.accent)),
                            )
                        }),
                )
                .child(title)
        };
        let mut rows = div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .px(px(18.))
            .pt(px(6.))
            .pb(px(6.))
            .when(lit, |d| d.bg(theme::hsla(p.bg)));
        for (r, chunk) in CARDS.chunks(4).enumerate() {
            let mut row = div().flex().flex_row().gap(px(10.));
            for (k, (id, title)) in chunk.iter().enumerate() {
                row = row.child(card(r * 4 + k, id, title, cx));
            }
            rows = rows.child(row);
        }
        rows
    }

    fn swatch(
        &self,
        id: (&'static str, usize),
        color: u32,
        target: Target,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<Div> {
        let p = palette();
        let open = self.picker.is_some_and(|pk| pk.target == target);
        div()
            .id(id)
            .flex_none()
            .rounded(px(5.))
            .bg(theme::hsla(color))
            .border_1()
            .border_color(theme::hsla(if open { p.accent } else { p.bg2 }))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| this.open_picker(target, cx)))
    }

    fn custom_editor(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = palette();
        let c = self.config.palette();
        let base = self.config.theme.custom_base().to_string();
        let base_title = theme::preset_by_id(&base).map_or("Kiln Dark", |p| p.title);
        let field = |i: usize, this: &Self, cx: &mut Context<Self>| {
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(7.))
                .text_size(px(10.5))
                .text_color(theme::hsla(p.fg_dim))
                .child(
                    this.swatch(("colour", i), themes::slot(&c, i), Target::Colour(i), cx)
                        .size(px(18.)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(COLOURS[i].1),
                )
                .child(
                    div()
                        .flex_none()
                        .w(px(64.))
                        .px(px(5.))
                        .rounded(px(5.))
                        .bg(theme::hsla(p.bar))
                        .border_1()
                        .border_color(theme::hsla(p.bg1))
                        .child(this.hex[i].clone()),
                )
        };
        let mut grid = div().flex().flex_col().gap(px(6.));
        for r in 0..COLOURS.len().div_ceil(2) {
            let mut row = div()
                .flex()
                .flex_row()
                .gap(px(14.))
                .child(field(2 * r, self, cx));
            row = if 2 * r + 1 < COLOURS.len() {
                row.child(field(2 * r + 1, self, cx))
            } else {
                row.child(div().flex_1())
            };
            grid = grid.child(row);
        }
        let ansi_row = |label: &'static str, from: usize, this: &Self, cx: &mut Context<Self>| {
            let mut row = div().flex().flex_row().items_center().gap(px(6.)).child(
                div()
                    .w(px(44.))
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(theme::hsla(p.bg3))
                    .child(label),
            );
            for i in from..from + 8 {
                row = row.child(
                    this.swatch(("ansi", i), c.ansi[i], Target::Ansi(i), cx)
                        .flex_1()
                        .h(px(20.)),
                );
            }
            row
        };
        let base_menu = self.base_menu.then(|| {
            div()
                .flex()
                .flex_col()
                .mb(px(8.))
                .p(px(4.))
                .rounded(px(6.))
                .bg(theme::hsla(p.bar))
                .border_1()
                .border_color(theme::hsla(p.bg1))
                .children(theme::PRESETS.iter().enumerate().map(|(i, preset)| {
                    let id = preset.id;
                    div()
                        .id(("base", i))
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(px(4.))
                        .cursor_pointer()
                        .text_size(px(11.))
                        .text_color(theme::hsla(if id == base { p.fg } else { p.fg_dim }))
                        .hover(|s| s.bg(theme::hsla(p.bg1)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.base_menu = false;
                            this.set(Change::CustomFrom(id), cx);
                        }))
                        .child(preset.title)
                }))
        });
        let picker = self.picker.map(|picker| self.picker_view(picker, cx));
        let reset_base = theme::preset_by_id(&base).map_or("kiln-dark", |p| p.id);
        div()
            .mx(px(18.))
            .my(px(4.))
            .px(px(12.))
            .pt(px(10.))
            .pb(px(12.))
            .rounded(px(10.))
            .bg(theme::hsla(p.bg))
            .border_1()
            .border_color(theme::hsla(p.bg1))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .items_center()
                    .mb(px(8.))
                    .text_size(px(11.))
                    .text_color(theme::hsla(p.fg_dim))
                    .child("Custom colours")
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .child("Start from")
                            .child(
                                div()
                                    .id("start-from")
                                    .px(px(6.))
                                    .py(px(1.))
                                    .rounded(px(6.))
                                    .bg(theme::hsla(p.bar))
                                    .border_1()
                                    .border_color(theme::hsla(p.bg1))
                                    .text_color(theme::hsla(p.fg))
                                    .cursor_pointer()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.base_menu = !this.base_menu;
                                        cx.notify();
                                    }))
                                    .child(SharedString::from(format!("{base_title} ▾"))),
                            ),
                    ),
            )
            .children(base_menu)
            .child(grid)
            .children(picker)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .mt(px(10.))
                    .child(ansi_row("normal", 0, self, cx))
                    .child(ansi_row("bright", 8, self, cx)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_between()
                    .mt(px(10.))
                    .text_size(px(10.))
                    .text_color(theme::hsla(p.bg3))
                    .child("Saved to kiln.toml as you edit")
                    .child(
                        div()
                            .id("reset-base")
                            .cursor_pointer()
                            .text_color(theme::hsla(p.fg_dim))
                            .hover(|s| s.text_color(theme::hsla(p.accent)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set(Change::CustomFrom(reset_base), cx)
                            }))
                            .child("Reset to base"),
                    ),
            )
    }

    /// The picker: saturation and value in a square, hue in a strip under it, the colour and
    /// its hex beside them.
    fn picker_view(&self, picker: Picker, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let p = palette();
        let (hue, sat, val) = picker.hsv;
        let color = themes::from_hsv(hue, sat, val);
        let pure = themes::from_hsv(hue, 1., 1.);
        let (w, h) = (170., 90.);
        let view = cx.entity();
        let view2 = view.clone();
        let white = gpui::white();
        let black = gpui::black();
        let square = div()
            .id("picker-square")
            .relative()
            .w(px(w))
            .h(px(h))
            .rounded(px(6.))
            .overflow_hidden()
            .bg(theme::hsla(pure))
            .cursor_crosshair()
            .child(div().absolute().size_full().bg(linear_gradient(
                90.,
                linear_color_stop(white, 0.),
                linear_color_stop(white.opacity(0.), 1.),
            )))
            .child(div().absolute().size_full().bg(linear_gradient(
                180.,
                linear_color_stop(black.opacity(0.), 0.),
                linear_color_stop(black, 1.),
            )))
            .child(
                div()
                    .absolute()
                    .left(px(sat * w - 5.))
                    .top(px((1. - val) * h - 5.))
                    .size(px(10.))
                    .rounded_full()
                    .border_2()
                    .border_color(white),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, _, cx| {
                        view.update(cx, |v, _| {
                            if let Some(p) = v.picker.as_mut() {
                                p.square = Some(bounds);
                            }
                        })
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    this.pick(Area::Square, e.position, cx)
                }),
            );
        let hues = [0., 60., 120., 180., 240., 300., 360.];
        let strip = div()
            .id("picker-strip")
            .relative()
            .mt(px(8.))
            .w(px(w))
            .h(px(12.))
            .rounded(px(6.))
            .overflow_hidden()
            .flex()
            .flex_row()
            .cursor_pointer()
            .children(hues.windows(2).map(|pair| {
                div().flex_1().h_full().bg(linear_gradient(
                    90.,
                    linear_color_stop(theme::hsla(themes::from_hsv(pair[0], 1., 1.)), 0.),
                    linear_color_stop(theme::hsla(themes::from_hsv(pair[1] - 0.01, 1., 1.)), 1.),
                ))
            }))
            .child(
                div()
                    .absolute()
                    .left(px(hue / 360. * w - 2.))
                    .top_0()
                    .w(px(4.))
                    .h_full()
                    .rounded(px(2.))
                    .border_1()
                    .border_color(white),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, _, cx| {
                        view2.update(cx, |v, _| {
                            if let Some(p) = v.picker.as_mut() {
                                p.strip = Some(bounds);
                            }
                        })
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, e: &MouseDownEvent, _, cx| {
                    this.pick(Area::Strip, e.position, cx)
                }),
            );
        div()
            .mt(px(10.))
            .flex()
            .flex_row()
            .gap(px(12.))
            .child(div().flex().flex_col().child(square).child(strip))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .text_size(px(10.5))
                    .text_color(theme::hsla(p.fg_dim))
                    .child(
                        div()
                            .size(px(28.))
                            .rounded(px(6.))
                            .bg(theme::hsla(color))
                            .border_1()
                            .border_color(theme::hsla(p.bg2)),
                    )
                    .child(SharedString::from(themes::hex(color)))
                    .child(
                        div()
                            .id("picker-done")
                            .cursor_pointer()
                            .text_color(theme::hsla(p.fg_dim))
                            .hover(|s| s.text_color(theme::hsla(p.accent)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.picker = None;
                                cx.notify();
                            }))
                            .child("Done"),
                    ),
            )
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
            .child(section("Theme"))
            .child(self.theme_cards(cx))
            .when(self.theme_name() == theme::CUSTOM, |d| {
                d.child(self.custom_editor(cx))
            })
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
            .w(px(PANEL_W))
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
            // Dragging in the colour picker.
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if e.pressed_button == Some(MouseButton::Left)
                    && let Some(area) = this.picker.and_then(|p| p.dragging)
                {
                    this.pick(area, e.position, cx);
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.end_drag(cx)),
            )
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
                Change::Icon(IconDesign::Flame),
                Box::new(|c: &Config| c.icon.design == IconDesign::Flame)
                    as Box<dyn Fn(&Config) -> bool>,
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
            let (key, value) = change.clone().entry();
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

    #[test]
    fn icons_from_before_the_designs_are_rewritten_on_the_first_write() {
        let old = "icon = \"black\"\nfont_size = 14\n";
        let text = apply(old, Change::FontSize(15.));
        let config = config::parse(&text).unwrap();
        assert_eq!(config.icon.old_colour, None, "{text}");
        assert_eq!(
            config.app_icon(),
            config::AppIcon::Drawn(IconDesign::Kiln, IconColour::Black)
        );
        assert!(
            text.contains("icon = \"kiln\"") && text.contains("[icon_colours]\nkiln = \"black\"")
        );
        // Choosing another design keeps Kiln's colour for later.
        let text = apply(old, Change::Icon(IconDesign::Line));
        let config = config::parse(&text).unwrap();
        assert_eq!(
            config.app_icon(),
            config::AppIcon::Drawn(IconDesign::Line, IconColour::Terracotta)
        );
        assert_eq!(config.icon_colour(IconDesign::Kiln), IconColour::Black);
    }

    #[test]
    fn each_design_keeps_its_own_colour() {
        let text = apply(
            config::TEMPLATE,
            Change::IconColour(IconDesign::Kiln, IconColour::Sage),
        );
        let text = apply(&text, Change::Icon(IconDesign::Beehive));
        let text = apply(
            &text,
            Change::IconColour(IconDesign::Beehive, IconColour::Slate),
        );
        let config = config::parse(&text).unwrap();
        assert_eq!(
            config.app_icon(),
            config::AppIcon::Drawn(IconDesign::Beehive, IconColour::Slate)
        );
        let text = apply(&text, Change::Icon(IconDesign::Kiln));
        assert_eq!(
            config::parse(&text).unwrap().app_icon(),
            config::AppIcon::Drawn(IconDesign::Kiln, IconColour::Sage)
        );
        // A custom image, then Remove: Kiln again, with its colour.
        let text = apply(
            &text,
            Change::CustomIcon("/c/icons/1234abcd-a.png.png".into()),
        );
        assert_eq!(
            config::parse(&text).unwrap().app_icon(),
            config::AppIcon::Custom("/c/icons/1234abcd-a.png.png".into())
        );
        let text = apply(&text, Change::RemoveCustomIcon);
        let config = config::parse(&text).unwrap();
        assert_eq!(config.icon_path, None);
        assert_eq!(
            config.app_icon(),
            config::AppIcon::Drawn(IconDesign::Kiln, IconColour::Sage)
        );
    }

    /// Applies `change` to `text` as Settings would.
    fn apply(text: &str, change: Change) -> String {
        let edits = change.edits(&config::parse(text).unwrap());
        let edits: Vec<config_write::Edit> = edits
            .iter()
            .map(|(key, value)| (key.as_slice(), value.clone()))
            .collect();
        config_write::with_edits(text, &edits).unwrap()
    }

    #[test]
    fn presets_are_exact_and_custom_comes_back() {
        let nord = theme::preset_by_id("nord").unwrap().palette;
        // A custom theme from Everforest, then one colour changed.
        let text = apply(config::TEMPLATE, Change::CustomFrom("everforest"));
        let text = apply(&text, Change::Colour(0, 0x123456));
        let custom = config::parse(&text).unwrap();
        assert_eq!(custom.theme.effective_name(), theme::CUSTOM);
        assert_eq!(custom.palette_for(true).bg, 0x123456);
        // A preset is exactly the preset; the custom colours wait in `[theme.custom]`.
        let text = apply(&text, Change::Theme("nord"));
        assert_eq!(config::parse(&text).unwrap().palette_for(true), nord);
        // Custom again: the same colours.
        let text = apply(&text, Change::SelectCustom);
        assert_eq!(
            config::parse(&text).unwrap().palette_for(true),
            custom.palette_for(true)
        );
    }

    #[test]
    fn colours_from_before_the_custom_table_move_into_it() {
        let old = "[theme]\nbase = \"kiln-light\"\nbg = \"#ffffff\"\nansi = [\"#000001\"]\n";
        let before = config::parse(old).unwrap();
        assert_eq!(before.theme.effective_name(), theme::CUSTOM);
        let text = apply(old, Change::Theme("nord"));
        let after = config::parse(&text).unwrap();
        assert_eq!(
            after.palette_for(true),
            theme::preset_by_id("nord").unwrap().palette
        );
        assert!(
            !after.theme.colours.any() && after.theme.base.is_none(),
            "{text}"
        );
        let text = apply(&text, Change::SelectCustom);
        assert_eq!(
            config::parse(&text).unwrap().palette_for(true),
            before.palette_for(true)
        );
    }
}
