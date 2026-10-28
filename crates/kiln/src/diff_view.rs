//! The diff viewer: one changed file's diff in a large pop-up over the window (from the rail's
//! Changes, or "Show changes" in the palette). Unified or split; text selectable with the
//! mouse and copied without the gutter; the keys scroll, jump between hunks and files, find
//! and open the file in the editor.

use std::ops::Range;
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AppContext as _, BoxShadow, ClipboardItem, Context, Div, Entity, EventEmitter, FocusHandle,
    Focusable, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, ParentElement, Pixels, Render, ScrollStrategy, StatefulInteractiveElement,
    Styled, Subscription, Task, UniformListScrollHandle, Window, canvas, div, point, px, relative,
    uniform_list,
};

use crate::diff::{self, FileDiff, LineKind, Place, Row, SplitRow};
use crate::git::FileChange;
use crate::grid_paint::{CellMetrics, FONT_FAMILY};
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{self, palette};

/// Split is offered from this many columns.
pub const SPLIT_FROM_COLUMNS: f32 = 120.;
/// The gutter: old number, new number, sign, in columns (with their gaps).
const GUTTER_COLUMNS: f32 = 14.;
const PAD: f32 = 12.;
/// How often an open diff is read again (it follows the file).
const REFRESH: Duration = Duration::from_millis(1500);

pub enum DiffEvent {
    Dismiss,
    /// Open the file in the editor at a line.
    OpenInEditor {
        path: std::path::PathBuf,
        line: u32,
    },
}

impl EventEmitter<DiffEvent> for DiffView {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Unified,
    Split,
}

struct Find {
    input: Entity<TextInput>,
    /// (row, from, to) in shown columns, unified rows.
    matches: Vec<(usize, usize, usize)>,
    current: Option<usize>,
    _events: Subscription,
}

pub struct DiffView {
    focus: FocusHandle,
    root: String,
    files: Vec<FileChange>,
    index: usize,
    diff: Option<FileDiff>,
    rows: Vec<Row>,
    split: Vec<SplitRow>,
    mode: Mode,
    show_all: bool,
    scroll: UniformListScrollHandle,
    /// The selection in unified rows: where it began and where it reaches.
    selection: Option<(Place, Place)>,
    /// ⌘A: the whole new side.
    all_new: bool,
    dragging: bool,
    /// Where the rows' text starts (window x) and the list's top and height.
    text_x: f32,
    list_top: f32,
    list_h: f32,
    metrics: CellMetrics,
    find: Option<Find>,
    note: Option<&'static str>,
    _refresh: Task<()>,
}

impl DiffView {
    pub fn new(
        root: String,
        files: Vec<FileChange>,
        index: usize,
        metrics: CellMetrics,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let refresh = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(REFRESH).await;
                let Ok(Some((root, file))) = this.read_with(cx, |this, _| this.current_file())
                else {
                    break;
                };
                let fresh = cx
                    .background_spawn(async move { diff::load(&root, &file) })
                    .await;
                let alive = this.update(cx, |this, cx| {
                    // The same file still, and it changed: show it again where it was.
                    if this.current_file().map(|(_, f)| f.path) == Some(fresh.path.clone())
                        && this.diff.as_ref().map(|d| &d.patch) != Some(&fresh.patch)
                    {
                        let top = top_row(&this.scroll);
                        this.set_diff(fresh, false, cx);
                        this.scroll.scroll_to_item(top, ScrollStrategy::Top);
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        });
        let mut this = Self {
            focus,
            root,
            files,
            index,
            diff: None,
            rows: Vec::new(),
            split: Vec::new(),
            mode: Mode::Unified,
            show_all: false,
            scroll: UniformListScrollHandle::new(),
            selection: None,
            all_new: false,
            dragging: false,
            text_x: 0.,
            list_top: 0.,
            list_h: 0.,
            metrics,
            find: None,
            note: None,
            _refresh: refresh,
        };
        this.load(cx);
        this
    }

    fn current_file(&self) -> Option<(String, FileChange)> {
        Some((self.root.clone(), self.files.get(self.index)?.clone()))
    }

    /// Reads the current file's diff in the background.
    fn load(&mut self, cx: &mut Context<Self>) {
        let Some((root, file)) = self.current_file() else {
            return;
        };
        let task = cx.background_spawn(async move { diff::load(&root, &file) });
        cx.spawn(async move |this, cx| {
            let fresh = task.await;
            let _ = this.update(cx, |this, cx| {
                this.set_diff(fresh, true, cx);
                cx.notify();
            });
        })
        .detach();
    }

    fn set_diff(&mut self, fresh: FileDiff, reset: bool, cx: &gpui::App) {
        self.rows = diff::unified_rows(&fresh);
        self.split = diff::split_rows(&fresh);
        self.diff = Some(fresh);
        if reset {
            self.selection = None;
            self.all_new = false;
            self.show_all = false;
            self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        self.refind(cx);
    }

    /// The file `step` away in the changes (wrapping).
    fn go_to_file(&mut self, step: i32, cx: &mut Context<Self>) {
        if self.files.is_empty() {
            return;
        }
        let n = self.files.len() as i32;
        self.index = (self.index as i32 + step).rem_euclid(n) as usize;
        self.load(cx);
        cx.notify();
    }

    /// The file being shown (the snapshots check it).
    pub fn file(&self) -> Option<&FileDiff> {
        self.diff.as_ref()
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The rows the view lays out (capped until Show all).
    fn shown(&self) -> usize {
        let total = match self.mode {
            Mode::Unified => self.rows.len(),
            Mode::Split => self.split.len(),
        };
        diff::shown_rows(total, self.show_all)
    }

    fn total(&self) -> usize {
        match self.mode {
            Mode::Unified => self.rows.len(),
            Mode::Split => self.split.len(),
        }
    }

    /// The selected text, as it is copied.
    pub fn selected_text(&self) -> Option<String> {
        if self.all_new {
            return Some(diff::new_side(&self.rows));
        }
        let (a, b) = self.selection?;
        (a != b).then(|| diff::copy_text(&self.rows, a, b))
    }

    fn copy(&mut self, text: String, note: &'static str, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.note = Some(note);
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(1500))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.note = None;
                cx.notify();
            });
        })
        .detach();
    }

    pub fn copy_selection(&mut self, cx: &mut Context<Self>) {
        if let Some(text) = self.selected_text() {
            self.copy(text, "Copied", cx);
        }
    }

    pub fn copy_diff(&mut self, cx: &mut Context<Self>) {
        if let Some(patch) = self.diff.as_ref().map(|d| d.patch.clone()) {
            self.copy(patch, "Copied the diff", cx);
        }
    }

    fn copy_path(&mut self, cx: &mut Context<Self>) {
        if let Some((_, file)) = self.current_file() {
            self.copy(file.path, "Copied the path", cx);
        }
    }

    pub fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.mode = mode;
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    fn open_in_editor(&mut self, cx: &mut Context<Self>) {
        let (Some((root, file)), Some(diff)) = (self.current_file(), self.diff.as_ref()) else {
            return;
        };
        cx.emit(DiffEvent::OpenInEditor {
            path: std::path::Path::new(&root).join(&file.path),
            line: diff.first_changed_line(),
        });
    }

    // ----- moving --------------------------------------------------------------------------

    fn visible_rows(&self) -> usize {
        ((self.list_h / f32::from(self.metrics.line_height)) as usize).max(1)
    }

    fn scroll_by(&mut self, rows: i32, cx: &mut Context<Self>) {
        let top = top_row(&self.scroll) as i32;
        let last = self.shown().saturating_sub(1) as i32;
        let to = (top + rows).clamp(0, last.max(0)) as usize;
        self.scroll.scroll_to_item(to, ScrollStrategy::Top);
        cx.notify();
    }

    /// The hunk headers' rows, in the current view.
    fn hunk_rows(&self) -> Vec<usize> {
        match self.mode {
            Mode::Unified => self
                .rows
                .iter()
                .enumerate()
                .filter(|(_, r)| matches!(r, Row::Hunk(_)))
                .map(|(i, _)| i)
                .collect(),
            Mode::Split => self
                .split
                .iter()
                .enumerate()
                .filter(|(_, r)| matches!(r, SplitRow::Hunk(_)))
                .map(|(i, _)| i)
                .collect(),
        }
    }

    pub fn next_hunk(&mut self, forward: bool, cx: &mut Context<Self>) {
        let top = top_row(&self.scroll);
        let hunks = self.hunk_rows();
        let to = if forward {
            hunks.into_iter().find(|&h| h > top)
        } else {
            hunks.into_iter().rev().find(|&h| h < top)
        };
        if let Some(to) = to {
            self.scroll.scroll_to_item(to, ScrollStrategy::Top);
            cx.notify();
        }
    }

    // ----- find ----------------------------------------------------------------------------

    fn open_find(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(find) = &self.find {
            window.focus(&find.input.focus_handle(cx), cx);
            return;
        }
        let input = cx.new(|cx| TextInput::new("Find in the diff", cx).with_size(px(12.)));
        let events = cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
            TextInputEvent::Changed => {
                this.refind(cx);
                this.show_match(cx);
            }
            TextInputEvent::Submit { shift } => {
                this.step_match(!shift, cx);
            }
            TextInputEvent::Cancel => {
                this.find = None;
                window.focus(&this.focus, cx);
                cx.notify();
            }
            _ => {}
        });
        window.focus(&input.focus_handle(cx), cx);
        self.find = Some(Find {
            input,
            matches: Vec::new(),
            current: None,
            _events: events,
        });
        cx.notify();
    }

    /// Finds the query in the unified rows' text (case-insensitive).
    fn refind(&mut self, cx: &gpui::App) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let query = find.input.read(cx).text().to_string();
        find.matches = find_in(&self.rows, &query);
        find.current = (!find.matches.is_empty()).then_some(0);
    }

    fn step_match(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(find) = self.find.as_mut() else {
            return;
        };
        let n = find.matches.len();
        if n == 0 {
            return;
        }
        find.current = Some(match find.current {
            None => 0,
            Some(i) if forward => (i + 1) % n,
            Some(i) => (i + n - 1) % n,
        });
        self.show_match(cx);
    }

    fn show_match(&mut self, cx: &mut Context<Self>) {
        if let Some(find) = &self.find
            && let Some(&(row, _, _)) = find.current.and_then(|i| find.matches.get(i))
        {
            self.mode = Mode::Unified;
            self.scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
        cx.notify();
    }

    // ----- keys and the mouse --------------------------------------------------------------

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let ks = &event.keystroke;
        let command = ks.modifiers.platform || ks.modifiers.control;
        let page = self.visible_rows().saturating_sub(2).max(1) as i32;
        match ks.key.as_str() {
            "escape" => {
                if self.find.take().is_some() {
                    window.focus(&self.focus, cx);
                    cx.notify();
                } else {
                    cx.emit(DiffEvent::Dismiss);
                }
            }
            "c" if command => self.copy_selection(cx),
            "a" if command => {
                self.all_new = true;
                self.selection = None;
                cx.notify();
            }
            "f" if command => self.open_find(window, cx),
            "o" if command => self.open_in_editor(cx),
            "up" | "k" => self.scroll_by(-1, cx),
            "down" | "j" => self.scroll_by(1, cx),
            "space" | "pagedown" => self.scroll_by(page, cx),
            "pageup" => self.scroll_by(-page, cx),
            "home" => self.scroll_by(i32::MIN / 2, cx),
            "end" => self.scroll_by(i32::MAX / 2, cx),
            "n" => self.next_hunk(true, cx),
            "p" => self.next_hunk(false, cx),
            "]" => self.go_to_file(1, cx),
            "[" => self.go_to_file(-1, cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    /// The shown column under a window x.
    fn column_at(&self, x: Pixels) -> usize {
        let cell = f32::from(self.metrics.cell_width);
        ((f32::from(x) - self.text_x) / cell).max(0.).round() as usize
    }

    fn mouse_down(&mut self, row: usize, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(Row::Line(line)) = self.rows.get(row) else {
            return;
        };
        let column = self.column_at(event.position.x);
        self.all_new = false;
        match event.click_count {
            2 => {
                let (from, to) = diff::word_at(&line.text, column);
                self.selection = Some(((row, from), (row, to)));
            }
            3.. => self.selection = Some(((row, 0), (row, usize::MAX))),
            _ if event.modifiers.shift && self.selection.is_some() => {
                if let Some((_, head)) = self.selection.as_mut() {
                    *head = (row, column);
                }
            }
            _ => {
                self.selection = Some(((row, column), (row, column)));
                self.dragging = true;
            }
        }
        cx.notify();
    }

    fn mouse_move(&mut self, row: usize, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging || event.pressed_button != Some(MouseButton::Left) {
            self.dragging = false;
            return;
        }
        let column = self.column_at(event.position.x);
        if let Some((_, head)) = self.selection.as_mut() {
            *head = (row, column);
            cx.notify();
        }
    }

    /// For the snapshots: selects in unified rows as a drag would.
    pub fn select(&mut self, from: Place, to: Place, cx: &mut Context<Self>) {
        self.selection = Some((from, to));
        self.all_new = false;
        cx.notify();
    }

    // ----- drawing -------------------------------------------------------------------------

    /// The selected shown columns of unified row `row`, if any.
    fn selected_columns(&self, row: usize) -> Option<Range<usize>> {
        if self.all_new {
            return match self.rows.get(row) {
                Some(Row::Line(line)) if line.kind != LineKind::Removed => Some(0..usize::MAX),
                _ => None,
            };
        }
        let (a, b) = self.selection?;
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        if row < start.0 || row > end.0 || start == end {
            return None;
        }
        let from = if row == start.0 { start.1 } else { 0 };
        let to = if row == end.0 { end.1 } else { usize::MAX };
        (from < to).then_some(from..to)
    }

    fn render_rows(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<gpui::AnyElement> {
        let p = palette();
        let cell = f32::from(self.metrics.cell_width);
        let line_h = self.metrics.line_height;
        let added_bg = theme::blend(p.bar, p.ok, 0.16);
        let removed_bg = theme::blend(p.bar, p.failed, 0.16);
        let number = |n: Option<u32>| {
            div()
                .flex_none()
                .w(px(cell * 5.))
                .flex()
                .justify_end()
                .text_color(theme::hsla(p.bg3))
                .child(n.map(|n| n.to_string()).unwrap_or_default())
        };
        let total = self.total();
        let lines = self
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Line(_)))
            .count();
        let mut out = Vec::with_capacity(range.len());
        for ix in range {
            if !self.show_all && ix == diff::ROW_CAP && total > diff::ROW_CAP {
                out.push(
                    div()
                        .id(("diff-show-all", ix))
                        .h(line_h)
                        .px(px(PAD))
                        .flex()
                        .items_center()
                        .text_color(theme::hsla(p.accent))
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_all = true;
                            cx.notify();
                        }))
                        .child(format!("Show all {lines} lines"))
                        .into_any_element(),
                );
                continue;
            }
            let element = match self.mode {
                Mode::Unified => self.render_unified_row(ix, &number, added_bg, removed_bg, cx),
                Mode::Split => self.render_split_row(ix, &number, added_bg, removed_bg),
            };
            out.push(element);
        }
        out
    }

    fn render_unified_row(
        &self,
        ix: usize,
        number: &dyn Fn(Option<u32>) -> Div,
        added_bg: u32,
        removed_bg: u32,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let p = palette();
        let line_h = self.metrics.line_height;
        let row = div()
            .id(("diff-row", ix))
            .h(line_h)
            .w_full()
            .px(px(PAD))
            .flex()
            .flex_row()
            .items_center()
            .whitespace_nowrap()
            .overflow_hidden();
        match &self.rows[ix] {
            Row::Hunk(h) => {
                let header = self
                    .diff
                    .as_ref()
                    .and_then(|d| d.hunks.get(*h))
                    .map(|h| h.header.clone())
                    .unwrap_or_default();
                row.bg(theme::hsla(theme::blend(p.bar, p.bg1, 0.6)))
                    .text_color(theme::hsla(p.muted))
                    .child(header)
                    .into_any_element()
            }
            Row::Line(line) => {
                let (bg, sign, sign_color) = match line.kind {
                    LineKind::Added => (Some(added_bg), "+", p.ok),
                    LineKind::Removed => (Some(removed_bg), "−", p.failed),
                    LineKind::Context => (None, " ", p.muted),
                };
                let shown = diff::display(&line.text);
                let text = self.render_text(ix, &shown);
                row.when_some(bg, |d, bg| d.bg(theme::hsla(bg)))
                    .cursor_text()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                            this.mouse_down(ix, event, cx)
                        }),
                    )
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        this.mouse_move(ix, event, cx)
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, _| this.dragging = false),
                    )
                    .child(number(line.old))
                    .child(div().flex_none().w(px(f32::from(self.metrics.cell_width))))
                    .child(number(line.new))
                    .child(div().flex_none().w(px(f32::from(self.metrics.cell_width))))
                    .child(
                        div()
                            .flex_none()
                            .w(px(f32::from(self.metrics.cell_width) * 2.))
                            .text_color(theme::hsla(sign_color))
                            .child(sign),
                    )
                    .child(text)
                    .into_any_element()
            }
        }
    }

    /// A line's text, with the selection and the find matches marked.
    fn render_text(&self, ix: usize, shown: &str) -> Div {
        let p = palette();
        let chars: Vec<char> = shown.chars().collect();
        let len = chars.len();
        let selected = self
            .selected_columns(ix)
            .map(|r| r.start.min(len)..r.end.min(len));
        let matches: Vec<(usize, usize, bool)> = self
            .find
            .as_ref()
            .map(|f| {
                f.matches
                    .iter()
                    .enumerate()
                    .filter(|(_, (row, _, _))| *row == ix)
                    .map(|(i, (_, from, to))| (*from, *to, f.current == Some(i)))
                    .collect()
            })
            .unwrap_or_default();
        // Each column's mark: 0 plain, 1 selected, 2 a match, 3 the current match.
        let mark = |c: usize| {
            if let Some(&(_, _, current)) = matches.iter().find(|(f, t, _)| c >= *f && c < *t) {
                return if current { 3 } else { 2 };
            }
            match &selected {
                Some(r) if r.contains(&c) => 1,
                _ => 0,
            }
        };
        let mut text = div().flex().flex_row().text_color(theme::hsla(p.fg));
        let mut at = 0;
        while at < len {
            let kind = mark(at);
            let mut end = at + 1;
            while end < len && mark(end) == kind {
                end += 1;
            }
            let span: String = chars[at..end].iter().collect();
            let piece = div().child(span);
            text = text.child(match kind {
                1 => piece.bg(theme::hsla(theme::blend(p.bg1, p.accent, 0.35))),
                2 => piece.bg(theme::hsla(theme::blend(p.bg1, p.modified, 0.5))),
                3 => piece
                    .bg(theme::hsla(p.modified))
                    .text_color(theme::hsla(p.bg)),
                _ => piece,
            });
            at = end;
        }
        // A selected empty line still shows it is in.
        if len == 0 && selected.is_some() {
            text = text.child(
                div()
                    .w(px(f32::from(self.metrics.cell_width)))
                    .h(self.metrics.line_height)
                    .bg(theme::hsla(theme::blend(p.bg1, p.accent, 0.35))),
            );
        }
        text
    }

    fn render_split_row(
        &self,
        ix: usize,
        number: &dyn Fn(Option<u32>) -> Div,
        added_bg: u32,
        removed_bg: u32,
    ) -> gpui::AnyElement {
        let p = palette();
        let line_h = self.metrics.line_height;
        let row = div()
            .id(("diff-split", ix))
            .h(line_h)
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .whitespace_nowrap();
        match &self.split[ix] {
            SplitRow::Hunk(h) => {
                let header = self
                    .diff
                    .as_ref()
                    .and_then(|d| d.hunks.get(*h))
                    .map(|h| h.header.clone())
                    .unwrap_or_default();
                row.px(px(PAD))
                    .bg(theme::hsla(theme::blend(p.bar, p.bg1, 0.6)))
                    .text_color(theme::hsla(p.muted))
                    .child(header)
                    .into_any_element()
            }
            SplitRow::Pair(left, right) => {
                let side = |line: &Option<diff::DiffLine>, old: bool| {
                    let bg = match line.as_ref().map(|l| l.kind) {
                        Some(LineKind::Removed) => Some(removed_bg),
                        Some(LineKind::Added) => Some(added_bg),
                        // An empty side of a pair.
                        None => Some(theme::blend(p.bar, p.bg, 0.5)),
                        Some(LineKind::Context) => None,
                    };
                    div()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .px(px(PAD))
                        .flex()
                        .flex_row()
                        .items_center()
                        .overflow_hidden()
                        .when_some(bg, |d, bg| d.bg(theme::hsla(bg)))
                        .when_some(line.as_ref(), |d, line| {
                            d.child(number(if old { line.old } else { line.new }))
                                .child(
                                    div()
                                        .flex_none()
                                        .w(px(f32::from(self.metrics.cell_width) * 2.)),
                                )
                                .child(
                                    div()
                                        .text_color(theme::hsla(p.fg))
                                        .child(diff::display(&line.text)),
                                )
                        })
                };
                row.child(side(left, true))
                    .child(div().flex_none().w(px(1.)).h_full().bg(theme::hsla(p.bg1)))
                    .child(side(right, false))
                    .into_any_element()
            }
        }
    }
}

impl Focusable for DiffView {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for DiffView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let cell = f32::from(self.metrics.cell_width);
        let width = f32::from(window.viewport_size().width) * 0.9;
        let can_split = width / cell >= SPLIT_FROM_COLUMNS;
        if !can_split && self.mode == Mode::Split {
            self.mode = Mode::Unified;
        }
        let (path, kind, added, removed, binary, loaded) = match &self.diff {
            Some(d) => (d.path.clone(), d.kind, d.added, d.removed, d.binary, true),
            None => (
                self.files
                    .get(self.index)
                    .map(|f| f.path.clone())
                    .unwrap_or_default(),
                self.files.get(self.index).map_or(' ', |f| f.kind),
                0,
                0,
                false,
                false,
            ),
        };
        let kind_color = match kind {
            'A' | '?' => p.ok,
            'D' | 'U' => p.failed,
            'M' => p.modified,
            _ => p.fg_dim,
        };
        let toggle = |label: &'static str, mode: Mode, cx: &mut Context<Self>| {
            let on = self.mode == mode;
            div()
                .id(label)
                .px(px(8.))
                .py(px(2.))
                .rounded(px(5.))
                .cursor_pointer()
                .when(on, |d| d.bg(theme::hsla(p.bg1)))
                .text_color(theme::hsla(if on { p.fg } else { p.muted }))
                .on_click(cx.listener(move |this, _, _, cx| this.set_mode(mode, cx)))
                .child(label)
        };
        let header =
            div()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .px(px(PAD + 4.))
                .py(px(10.))
                .border_b_1()
                .border_color(theme::hsla(p.bg1))
                .child(div().flex_none().text_color(theme::hsla(kind_color)).child(
                    if kind == '?' {
                        "A".into()
                    } else {
                        kind.to_string()
                    },
                ))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .text_ellipsis()
                        .whitespace_nowrap()
                        .text_color(theme::hsla(p.fg))
                        .child(path),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .flex_row()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_color(theme::hsla(p.ok))
                                .child(format!("+{added}")),
                        )
                        .child(
                            div()
                                .text_color(theme::hsla(p.failed))
                                .child(format!("−{removed}")),
                        ),
                )
                .when(can_split, |d| {
                    d.child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_row()
                            .p(px(2.))
                            .rounded(px(7.))
                            .bg(theme::hsla(p.bg))
                            .child(toggle("Unified", Mode::Unified, cx))
                            .child(toggle("Split", Mode::Split, cx)),
                    )
                })
                .child(
                    div()
                        .flex_none()
                        .text_color(theme::hsla(p.bg3))
                        .child("esc"),
                );

        let find_bar = self.find.as_ref().map(|find| {
            let n = find.matches.len();
            div()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .px(px(PAD + 4.))
                .py(px(6.))
                .border_b_1()
                .border_color(theme::hsla(p.bg1))
                .child(div().text_color(theme::hsla(p.accent)).child("❯"))
                .child(div().flex_1().min_w_0().child(find.input.clone()))
                .child(
                    div()
                        .text_color(theme::hsla(p.muted))
                        .text_size(px(11.))
                        .child(crate::workspace::find_status(find.current, n)),
                )
        });

        let entity = cx.entity();
        let body = if !loaded {
            div()
                .flex_1()
                .p(px(PAD + 4.))
                .text_color(theme::hsla(p.muted))
                .child("Reading the diff…")
                .into_any_element()
        } else if binary {
            div()
                .flex_1()
                .p(px(PAD + 4.))
                .text_color(theme::hsla(p.muted))
                .child("Binary file changed")
                .into_any_element()
        } else if self.rows.is_empty() {
            div()
                .flex_1()
                .p(px(PAD + 4.))
                .text_color(theme::hsla(p.muted))
                .child("No changes against HEAD")
                .into_any_element()
        } else {
            let count = self.shown() + usize::from(self.shown() < self.total());
            let gutter = PAD + GUTTER_COLUMNS * cell;
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    canvas(
                        move |bounds, _, cx| {
                            entity.update(cx, |this, _| {
                                this.text_x = f32::from(bounds.origin.x) + gutter;
                                this.list_top = f32::from(bounds.origin.y);
                                this.list_h = f32::from(bounds.size.height);
                            })
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .child(
                    uniform_list("diff-rows", count, cx.processor(Self::render_rows))
                        .track_scroll(&self.scroll)
                        .size_full(),
                )
                .into_any_element()
        };

        let action = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .cursor_pointer()
                .text_color(theme::hsla(p.fg_dim))
                .hover(|s| s.text_color(theme::hsla(p.fg)))
                .child(label)
        };
        let footer = div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(14.))
            .px(px(PAD + 4.))
            .py(px(8.))
            .border_t_1()
            .border_color(theme::hsla(p.bg1))
            .text_size(px(11.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(theme::hsla(p.bg3))
                    .child("↑↓ scroll · n/p hunk · [ ] file · ⌘C copy · ⌘O open"),
            )
            .when_some(self.note, |d, note| {
                d.child(div().text_color(theme::hsla(p.ok)).child(note))
            })
            .child(
                action("diff-copy", "Copy diff")
                    .on_click(cx.listener(|this, _, _, cx| this.copy_diff(cx))),
            )
            .child(
                action("diff-path", "Copy path")
                    .on_click(cx.listener(|this, _, _, cx| this.copy_path(cx))),
            )
            .child(
                action("diff-open", "Open")
                    .on_click(cx.listener(|this, _, _, cx| this.open_in_editor(cx))),
            );

        div()
            .id("diff-view")
            .key_context("DiffView")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(|this, _: &crate::Find, window, cx| this.open_find(window, cx)))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .w(relative(0.9))
            .h(relative(0.9))
            .flex()
            .flex_col()
            .rounded(px(12.))
            .bg(theme::hsla(p.bar))
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
            .overflow_hidden()
            .font_family(FONT_FAMILY)
            .text_size(self.metrics.font_size)
            .child(header)
            .children(find_bar)
            .child(body)
            .child(footer)
    }
}

/// The first row in view (or the one a scroll is about to bring there).
fn top_row(scroll: &UniformListScrollHandle) -> usize {
    let state = scroll.0.borrow();
    state
        .deferred_scroll_to_item
        .as_ref()
        .map(|deferred| deferred.item_index)
        .unwrap_or_else(|| state.base_handle.logical_scroll_top().0)
}

/// The rows (unified) whose text holds `query`, case-insensitive: (row, from, to) in shown
/// columns.
pub fn find_in(rows: &[Row], query: &str) -> Vec<(usize, usize, usize)> {
    let query: Vec<char> = query.to_lowercase().chars().collect();
    if query.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let Row::Line(line) = row else {
            continue;
        };
        let shown: Vec<char> = diff::display(&line.text).to_lowercase().chars().collect();
        let mut at = 0;
        while at + query.len() <= shown.len() {
            if shown[at..at + query.len()] == query[..] {
                out.push((i, at, at + query.len()));
                at += query.len();
            } else {
                at += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_marks_each_match_in_shown_columns() {
        let rows = diff::unified_rows(&diff::parse(
            "a.rs",
            'M',
            "@@ -1,2 +1,2 @@\n-let Foo = foo;\n+\tlet foo = 2;\n",
        ));
        assert_eq!(
            find_in(&rows, "foo"),
            vec![(1, 4, 7), (1, 10, 13), (2, 8, 11)]
        );
        assert!(find_in(&rows, "").is_empty());
    }
}
