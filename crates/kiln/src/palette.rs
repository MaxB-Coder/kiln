//! The command palette (⌘K / Ctrl+Shift+K): actions, sessions, projects and past commands in
//! one filtered list.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};

use crate::grid_paint::FONT_FAMILY;
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{self, palette};

/// What choosing an item does.
pub enum PaletteTarget {
    Action(Box<dyn Action>),
    Session(usize),
    RunCommand(String),
}

pub struct PaletteItem {
    pub label: String,
    /// Dim text on the right: a shortcut, a folder, "session", "ran 3×".
    pub detail: String,
    pub kind: &'static str,
    pub target: PaletteTarget,
}

pub enum PaletteEvent {
    Confirm(PaletteTarget),
    Dismiss,
}

impl EventEmitter<PaletteEvent> for CommandPalette {}

pub struct CommandPalette {
    input: Entity<TextInput>,
    items: Vec<PaletteItem>,
    /// Indices into `items`, best match first.
    filtered: Vec<usize>,
    selected: usize,
    _input_events: Subscription,
}

const VISIBLE: usize = 12;

impl CommandPalette {
    pub fn new(items: Vec<PaletteItem>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| TextInput::new("Search actions, sessions, commands…", cx));
        let sub = cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
            TextInputEvent::Changed => this.refilter(cx),
            TextInputEvent::Submit { .. } => this.confirm(window, cx),
            TextInputEvent::Cancel => cx.emit(PaletteEvent::Dismiss),
            TextInputEvent::Up => this.move_selection(-1, cx),
            TextInputEvent::Down => this.move_selection(1, cx),
        });
        window.focus(&input.focus_handle(cx), cx);
        let mut this = Self {
            input,
            items,
            filtered: Vec::new(),
            selected: 0,
            _input_events: sub,
        };
        this.refilter(cx);
        this
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).text().to_string();
        let mut scored: Vec<(i32, usize)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(i, item)| fuzzy_score(&query, &item.label).map(|s| (s, i)))
            .collect();
        // Stable: equal scores keep the order the workspace gave (most useful first).
        scored.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
        self.filtered = scored.into_iter().map(|(_, i)| i).collect();
        self.selected = 0;
        cx.notify();
    }

    fn move_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        if self.filtered.is_empty() {
            return;
        }
        let len = self.filtered.len() as i32;
        self.selected = (self.selected as i32 + delta).rem_euclid(len) as usize;
        cx.notify();
    }

    fn confirm(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(&index) = self.filtered.get(self.selected) else {
            return;
        };
        let item = self.items.swap_remove(index);
        cx.emit(PaletteEvent::Confirm(item.target));
    }

    fn click(&mut self, position: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = position;
        self.confirm(window, cx);
    }
}

/// Subsequence match, case-insensitive. Higher is better; `None` means no match. Rewards
/// matches at the start of words and runs of consecutive characters.
pub fn fuzzy_score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let text_chars: Vec<char> = text.chars().flat_map(|c| c.to_lowercase()).collect();
    let mut score = 0;
    let mut position = 0;
    let mut previous: Option<usize> = None;
    for q in query.chars().flat_map(|c| c.to_lowercase()) {
        if q == ' ' {
            continue;
        }
        let found = (position..text_chars.len()).find(|&i| text_chars[i] == q)?;
        let word_start = found == 0 || !text_chars[found - 1].is_alphanumeric();
        score += 1;
        if word_start {
            score += 6;
        }
        if previous == Some(found.wrapping_sub(1)) {
            score += 8;
        }
        score -= (found - position).min(10) as i32 / 3;
        previous = Some(found);
        position = found + 1;
    }
    Some(score)
}

impl Focusable for CommandPalette {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let first = self.selected.saturating_sub(VISIBLE - 1);
        let rows = self
            .filtered
            .iter()
            .enumerate()
            .skip(first)
            .take(VISIBLE)
            .map(|(position, &index)| {
                let item = &self.items[index];
                let selected = position == self.selected;
                div()
                    .id(("palette-item", position))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .h(px(30.))
                    .px(px(12.))
                    .rounded(px(6.))
                    .when(selected, |d| d.bg(theme::hsla(p.bg1)))
                    .hover(|s| s.bg(theme::hsla(p.raised)))
                    .cursor_pointer()
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.click(position, window, cx)),
                    )
                    .child(
                        div()
                            .w(px(64.))
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::hsla(p.muted))
                            .child(item.kind),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .text_color(theme::hsla(if selected { p.fg } else { p.fg_dim }))
                            .child(SharedString::from(item.label.clone())),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::hsla(p.muted))
                            .child(SharedString::from(item.detail.clone())),
                    )
            });
        div()
            .id("palette")
            .w(px(620.))
            .max_w_full()
            .flex()
            .flex_col()
            .p(px(8.))
            .rounded(px(10.))
            .bg(theme::hsla(p.bar))
            .shadow_lg()
            .font_family(FONT_FAMILY)
            .text_size(px(13.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(div().px(px(12.)).py(px(10.)).child(self.input.clone()))
            .children(rows)
            .when(self.filtered.is_empty(), |d| {
                d.child(
                    div()
                        .px(px(12.))
                        .py(px(8.))
                        .text_color(theme::hsla(p.muted))
                        .child("Nothing matches"),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_prefers_word_starts_and_runs() {
        assert!(
            fuzzy_score("split", "Split Right").unwrap()
                > fuzzy_score("split", "s p l i t").unwrap()
        );
        assert!(
            fuzzy_score("sr", "Split Right").unwrap() > fuzzy_score("sr", "Show order").unwrap()
        );
        assert_eq!(fuzzy_score("xyz", "Split Right"), None);
        assert_eq!(fuzzy_score("", "anything"), Some(0));
    }
}
