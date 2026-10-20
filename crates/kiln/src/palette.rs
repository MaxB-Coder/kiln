//! The command palette (⌘K / Ctrl+Shift+K): past commands, projects, sessions and actions in
//! one filtered list, under quiet group headings.

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, App, AppContext as _, BoxShadow, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, point, px, relative,
};

use crate::grid_paint::FONT_FAMILY;
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{self, palette};

/// What choosing an item does.
pub enum PaletteTarget {
    Action(Box<dyn Action>),
    Session(usize),
    Project(std::path::PathBuf),
    RunCommand(String),
}

/// The headings results sit under, in the order they are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteGroup {
    Commands,
    Projects,
    Sessions,
    Actions,
}

impl PaletteGroup {
    pub const ALL: [PaletteGroup; 4] = [
        PaletteGroup::Commands,
        PaletteGroup::Projects,
        PaletteGroup::Sessions,
        PaletteGroup::Actions,
    ];

    pub fn title(self) -> &'static str {
        match self {
            PaletteGroup::Commands => "Commands",
            PaletteGroup::Projects => "Projects",
            PaletteGroup::Sessions => "Sessions",
            PaletteGroup::Actions => "Actions",
        }
    }
}

pub struct PaletteItem {
    pub label: String,
    /// Dim text on the right: a shortcut, a folder, the project a command ran in.
    pub detail: String,
    pub group: PaletteGroup,
    /// A project's colour, as a dot before the label.
    pub dot: Option<u32>,
    pub target: PaletteTarget,
}

/// Rows per group; a group that has the results to itself shows more.
const PER_GROUP: usize = 4;
const ALONE: usize = 12;

/// The matches for `query` by group, best first within each, in `PaletteGroup::ALL` order.
/// Equal scores keep the order the workspace gave (most useful first).
pub fn grouped(items: &[PaletteItem], query: &str) -> Vec<(PaletteGroup, Vec<usize>)> {
    let mut groups: Vec<(PaletteGroup, Vec<usize>)> = PaletteGroup::ALL
        .into_iter()
        .filter_map(|group| {
            let mut scored: Vec<(i32, usize)> = items
                .iter()
                .enumerate()
                .filter(|(_, item)| item.group == group)
                .filter_map(|(i, item)| fuzzy_score(query, &item.label).map(|s| (s, i)))
                .collect();
            scored.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
            let rows: Vec<usize> = scored.into_iter().map(|(_, i)| i).collect();
            (!rows.is_empty()).then_some((group, rows))
        })
        .collect();
    let cap = if groups.len() == 1 { ALONE } else { PER_GROUP };
    for (_, rows) in &mut groups {
        rows.truncate(cap);
    }
    groups
}

pub enum PaletteEvent {
    Confirm(PaletteTarget),
    Dismiss,
}

impl EventEmitter<PaletteEvent> for CommandPalette {}

pub struct CommandPalette {
    input: Entity<TextInput>,
    items: Vec<PaletteItem>,
    /// The rows shown, by group: indices into `items`.
    groups: Vec<(PaletteGroup, Vec<usize>)>,
    /// Position among all the rows shown, top to bottom.
    selected: usize,
    _input_events: Subscription,
}

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
            groups: Vec::new(),
            selected: 0,
            _input_events: sub,
        };
        this.refilter(cx);
        this
    }

    fn refilter(&mut self, cx: &mut Context<Self>) {
        let query = self.input.read(cx).text().to_string();
        self.groups = grouped(&self.items, &query);
        self.selected = 0;
        cx.notify();
    }

    /// Every row shown, top to bottom.
    fn shown(&self) -> impl Iterator<Item = usize> + '_ {
        self.groups
            .iter()
            .flat_map(|(_, rows)| rows.iter().copied())
    }

    fn move_selection(&mut self, delta: i32, cx: &mut Context<Self>) {
        let len = self.shown().count() as i32;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as i32 + delta).rem_euclid(len) as usize;
        cx.notify();
    }

    fn confirm(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.shown().nth(self.selected) else {
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

impl CommandPalette {
    fn render_row(
        &self,
        position: usize,
        index: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let p = palette();
        let item = &self.items[index];
        let selected = position == self.selected;
        // A command reruns on Enter: the selected one says so.
        let detail = match (&item.target, selected) {
            (PaletteTarget::RunCommand(_), true) if item.detail.is_empty() => "rerun".to_string(),
            (PaletteTarget::RunCommand(_), true) => format!("rerun · {}", item.detail),
            _ => item.detail.clone(),
        };
        div()
            .id(("palette-item", position))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .h(px(26.))
            .px(px(15.))
            .when(selected, |d| d.bg(theme::hsla(p.bg)))
            .hover(|s| s.bg(theme::hsla(p.bg)))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| this.click(position, window, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(9.))
                    .text_color(theme::hsla(if selected { p.fg } else { p.ansi[7] }))
                    .when_some(item.dot, |d, color| {
                        d.child(
                            div()
                                .flex_none()
                                .size(px(6.))
                                .rounded_full()
                                .bg(theme::hsla(color)),
                        )
                    })
                    .child(
                        div()
                            .min_w_0()
                            .whitespace_nowrap()
                            .overflow_hidden()
                            .text_ellipsis()
                            .child(SharedString::from(item.label.clone())),
                    ),
            )
            .child(
                // Folders can be long: the detail gives way before the label does.
                div()
                    .flex_shrink_1()
                    .min_w_0()
                    .max_w(px(260.))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_size(px(10.5))
                    .text_color(theme::hsla(p.bg3))
                    .child(SharedString::from(detail)),
            )
    }
}

impl Render for CommandPalette {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let mut body = div().flex().flex_col();
        let mut position = 0;
        for (group, rows) in &self.groups {
            body = body.child(
                div()
                    .px(px(15.))
                    .pt(px(9.))
                    .pb(px(3.))
                    .text_size(px(10.5))
                    .text_color(theme::hsla(p.bg3))
                    .child(group.title()),
            );
            for &index in rows {
                body = body.child(self.render_row(position, index, cx));
                position += 1;
            }
        }
        div()
            .id("palette")
            .w(relative(0.58))
            .min_w(px(420.))
            .max_w(px(820.))
            .flex()
            .flex_col()
            .pt(px(6.))
            .pb(px(10.))
            .rounded(px(12.))
            .bg(theme::hsla(p.bar))
            // A ring instead of a border.
            .shadow(vec![BoxShadow {
                color: theme::hsla(p.bg1),
                offset: point(px(0.), px(0.)),
                blur_radius: px(0.),
                spread_radius: px(1.),
                inset: false,
            }])
            .font_family(FONT_FAMILY)
            .text_size(px(12.5))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(15.))
                    .pt(px(9.))
                    .pb(px(10.))
                    .child(
                        div()
                            .flex_none()
                            .text_color(theme::hsla(p.accent))
                            .child("❯"),
                    )
                    .child(div().flex_1().min_w_0().child(self.input.clone())),
            )
            .child(body)
            .when(self.groups.is_empty(), |d| {
                d.child(
                    div()
                        .px(px(15.))
                        .py(px(8.))
                        .text_color(theme::hsla(p.bg3))
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

    fn item(label: &str, group: PaletteGroup) -> PaletteItem {
        PaletteItem {
            label: label.into(),
            detail: String::new(),
            group,
            dot: None,
            target: PaletteTarget::RunCommand(label.into()),
        }
    }

    #[test]
    fn results_sit_under_their_group_in_order() {
        let items = vec![
            item("Keyboard shortcuts", PaletteGroup::Actions),
            item("receipts", PaletteGroup::Projects),
            item("npm test", PaletteGroup::Commands),
            item("api", PaletteGroup::Sessions),
            item("npm test -- --watch", PaletteGroup::Commands),
        ];
        let groups = grouped(&items, "");
        let titles: Vec<_> = groups.iter().map(|(g, _)| g.title()).collect();
        assert_eq!(titles, ["Commands", "Projects", "Sessions", "Actions"]);
        assert_eq!(groups[0].1, [2, 4]);

        // Only the groups with a match, best match first.
        let groups = grouped(&items, "te");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], (PaletteGroup::Commands, vec![2, 4]));
        assert!(grouped(&items, "zzz").is_empty());
    }

    #[test]
    fn groups_are_capped_unless_alone() {
        let mut items: Vec<_> = (0..20)
            .map(|i| item(&format!("cmd {i}"), PaletteGroup::Commands))
            .collect();
        assert_eq!(grouped(&items, "")[0].1.len(), ALONE);
        items.push(item("New session", PaletteGroup::Actions));
        let groups = grouped(&items, "");
        assert_eq!(groups[0].1.len(), PER_GROUP);
        assert_eq!(groups[1].1, [20]);
    }
}
