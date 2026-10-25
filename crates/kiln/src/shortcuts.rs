//! The keyboard shortcuts pop-up (⌘/ or Ctrl+Shift+/): every key binding in effect, grouped,
//! with a filter. Built from the live keymap each time it opens (see `keymap`).

use gpui::{
    App, AppContext as _, BoxShadow, Context, Div, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, point, px,
};

use crate::grid_paint::FONT_FAMILY;
use crate::keymap::{self, Group, KeyStyle, ShortcutRow};
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{self, palette};

/// Windows at least this wide get two columns.
const TWO_COLUMNS_FROM: f32 = 900.;

pub enum ShortcutsEvent {
    Dismiss,
}

impl EventEmitter<ShortcutsEvent> for ShortcutsView {}

pub struct ShortcutsView {
    input: Entity<TextInput>,
    rows: Vec<ShortcutRow>,
    style: KeyStyle,
    _input_events: Subscription,
}

impl ShortcutsView {
    pub fn new(
        rows: Vec<ShortcutRow>,
        style: KeyStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| TextInput::new("Filter by name or keys", cx));
        let sub = cx.subscribe_in(&input, window, |_, _, event, _, cx| match event {
            TextInputEvent::Changed => cx.notify(),
            TextInputEvent::Cancel => cx.emit(ShortcutsEvent::Dismiss),
            _ => {}
        });
        window.focus(&input.focus_handle(cx), cx);
        Self {
            input,
            rows,
            style,
            _input_events: sub,
        }
    }

    /// The rows the filter lets through, by section.
    fn sections(&self, cx: &App) -> Vec<(Group, Vec<&ShortcutRow>)> {
        let query = self.input.read(cx).text().to_string();
        Group::ALL
            .into_iter()
            .filter_map(|group| {
                let rows: Vec<_> = self
                    .rows
                    .iter()
                    .filter(|r| r.group == group && keymap::matches(r, &query, self.style))
                    .collect();
                (!rows.is_empty()).then_some((group, rows))
            })
            .collect()
    }
}

impl Focusable for ShortcutsView {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

fn key_cap(keys: &str) -> Div {
    let p = palette();
    div()
        .flex_none()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(4.))
        .bg(theme::hsla(p.raised))
        .text_size(px(10.5))
        .text_color(theme::hsla(p.fg_dim))
        .child(SharedString::from(keys.to_string()))
}

fn row(row: &ShortcutRow) -> Div {
    let p = palette();
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .h(px(24.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .text_color(theme::hsla(p.ansi[7]))
                .child(SharedString::from(row.label.clone())),
        )
        .child(
            div()
                .flex_none()
                .flex()
                .flex_row()
                .gap(px(4.))
                .children(row.keys.iter().map(|k| key_cap(k))),
        )
}

fn section(group: Group, rows: &[&ShortcutRow]) -> Div {
    let p = palette();
    div()
        .flex()
        .flex_col()
        .pb(px(10.))
        .child(
            div()
                .pb(px(4.))
                .text_size(px(10.5))
                .text_color(theme::hsla(p.bg3))
                .child(group.title()),
        )
        .children(rows.iter().map(|r| row(r)))
}

impl Render for ShortcutsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let sections = self.sections(cx);
        let two = f32::from(window.viewport_size().width) >= TWO_COLUMNS_FROM;
        // It sits 52 px from the top (see the workspace's scrim): leave room below too.
        let max_h = f32::from(window.viewport_size().height) - 52. - 24.;

        // Two columns: sections in order, the left one filled to about half of the rows.
        let total: usize = sections.iter().map(|(_, rows)| rows.len() + 2).sum();
        let mut columns: Vec<Vec<Div>> = vec![Vec::new(), Vec::new()];
        let mut left = 0;
        for (group, rows) in &sections {
            let column = if !two || left * 2 < total { 0 } else { 1 };
            if column == 0 {
                left += rows.len() + 2;
            }
            columns[column].push(section(*group, rows));
        }

        let body = if sections.is_empty() {
            div()
                .px(px(12.))
                .pb(px(14.))
                .text_color(theme::hsla(p.muted))
                .child("No shortcut matches")
        } else {
            div().flex().flex_row().gap(px(32.)).px(px(12.)).children(
                columns
                    .into_iter()
                    .filter(|c| !c.is_empty())
                    .map(|sections| {
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .children(sections)
                    }),
            )
        };

        div()
            .id("shortcuts")
            .w(px(if two { 760. } else { 420. }))
            .max_w_full()
            .max_h(px(max_h))
            .flex()
            .flex_col()
            .p(px(8.))
            .rounded(px(12.))
            .bg(theme::hsla(p.bar))
            // A ring and a soft drop shadow instead of a border.
            .shadow(vec![
                BoxShadow {
                    color: theme::hsla(p.bg1),
                    offset: point(px(0.), px(0.)),
                    blur_radius: px(0.),
                    spread_radius: px(1.),
                    inset: false,
                },
                BoxShadow {
                    color: p.shadow(),
                    offset: point(px(0.), px(12.)),
                    blur_radius: px(32.),
                    spread_radius: px(0.),
                    inset: false,
                },
            ])
            .font_family(FONT_FAMILY)
            .text_size(px(13.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(10.))
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme::hsla(p.accent))
                            .child("❯"),
                    )
                    .child(div().flex_1().min_w_0().child(self.input.clone())),
            )
            .child(
                div()
                    .id("shortcuts-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .pt(px(4.))
                    .child(body),
            )
    }
}
