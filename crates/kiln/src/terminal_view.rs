//! One terminal session in a window: input, selection, clipboard and session events.
//!
//! Phase 1 puts blocks, the input bar and splits around this view; the view itself stays the
//! "live grid" for the running command and full-screen programs.

use std::ops::Range;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{
    App, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, Render, ScrollDelta,
    ScrollWheelEvent, Styled, Task, UTF16Selection, Window, div,
};
use kiln_term::engine::event::Event as TermEvent;
use kiln_term::engine::grid::{Dimensions, Scroll};
use kiln_term::engine::index::{Column, Line, Point as GridPoint, Side};
use kiln_term::engine::selection::{Selection, SelectionType};
use kiln_term::engine::term::TermMode;
use kiln_term::input::{self, AltMode, Mods, MouseAction};
use kiln_term::{GridSize, Mark, MarkRecord, Session, SessionEvent, SessionOptions};

use crate::terminal_element::{CellMetrics, TerminalElement};
use crate::theme::{self, PALETTE};
use crate::{Copy, Paste, ScrollPageDown, ScrollPageUp, ScrollToBottom, ScrollToTop, SelectAll};

#[cfg(target_os = "macos")]
const ALT_MODE: AltMode = AltMode::Compose;
#[cfg(not(target_os = "macos"))]
const ALT_MODE: AltMode = AltMode::Escape;

const CURSOR_BLINK: Duration = Duration::from_millis(600);

/// Where the grid sits in the window, written by the element each frame.
#[derive(Clone, Copy, Debug)]
pub struct GridLayout {
    pub origin: Point<Pixels>,
    pub metrics: CellMetrics,
    pub columns: usize,
    pub lines: usize,
}

pub struct TerminalView {
    pub(crate) session: Session,
    focus: FocusHandle,
    pub(crate) layout: Option<GridLayout>,
    pub(crate) metrics: CellMetrics,
    /// Text the input method is composing (dead keys, IME); drawn at the cursor.
    pub(crate) marked_text: Option<String>,
    pub(crate) cursor_blink_on: bool,
    blink_epoch: usize,
    selecting: bool,
    scroll_remainder: f32,
    /// Last mouse cell reported to a program, to avoid repeating motion reports.
    last_mouse_cell: Option<(usize, usize)>,
    pub(crate) marks: Vec<MarkRecord>,
    cwd: Option<String>,
    title: Option<String>,
    started: Instant,
    pub(crate) first_frame_logged: bool,
    first_output_logged: bool,
    bench_startup: bool,
    _events: Task<()>,
    _blink: Option<Task<()>>,
}

impl TerminalView {
    /// Starts the shell. Events queue in the receiver until the view exists.
    pub fn spawn_session(
        options: SessionOptions,
        initial: GridSize,
    ) -> anyhow::Result<(Session, UnboundedReceiver<SessionEvent>)> {
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let session = Session::spawn(options, initial, move |event| {
            let _ = tx.unbounded_send(event);
        })?;
        Ok((session, rx))
    }

    pub fn new(
        session: Session,
        mut rx: UnboundedReceiver<SessionEvent>,
        metrics: CellMetrics,
        bench_startup: bool,
        started: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        // Drain everything the PTY thread queued, then handle it in one update, so a burst of
        // output costs one redraw.
        let events = cx.spawn_in(window, async move |this, cx| {
            while let Some(first) = rx.next().await {
                let mut batch = vec![first];
                while let Ok(event) = rx.try_recv() {
                    batch.push(event);
                }
                let updated =
                    this.update_in(cx, |this, window, cx| this.handle_events(batch, window, cx));
                if updated.is_err() {
                    break;
                }
            }
        });

        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        cx.on_focus_in(&focus, window, |this, _, cx| this.restart_blink(cx))
            .detach();
        cx.on_focus_out(&focus, window, |this, _, _, cx| {
            this._blink = None;
            this.cursor_blink_on = true;
            cx.notify();
        })
        .detach();

        let mut this = Self {
            session,
            focus,
            layout: None,
            metrics,
            marked_text: None,
            cursor_blink_on: true,
            blink_epoch: 0,
            selecting: false,
            scroll_remainder: 0.,
            last_mouse_cell: None,
            marks: Vec::new(),
            cwd: None,
            title: None,
            started,
            first_frame_logged: false,
            first_output_logged: false,
            bench_startup,
            _events: events,
            _blink: None,
        };
        this.restart_blink(cx);
        this
    }

    fn handle_events(
        &mut self,
        batch: Vec<SessionEvent>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for event in batch {
            match event {
                SessionEvent::Term(TermEvent::Wakeup) => {
                    if !self.first_output_logged {
                        self.first_output_logged = true;
                        log::info!("first shell output after {:?}", self.started.elapsed());
                    }
                }
                SessionEvent::Term(TermEvent::Title(title)) => self.title = Some(title),
                SessionEvent::Term(TermEvent::ResetTitle) => self.title = None,
                SessionEvent::Term(TermEvent::PtyWrite(text)) => {
                    self.session.write(text.into_bytes())
                }
                SessionEvent::Term(TermEvent::ClipboardStore(_, text)) => {
                    cx.write_to_clipboard(ClipboardItem::new_string(text))
                }
                // OSC 52 reads are disabled in the engine config; never answer one.
                SessionEvent::Term(TermEvent::ClipboardLoad(..)) => {}
                SessionEvent::Term(TermEvent::ColorRequest(index, format)) => {
                    let term = self.session.term().lock();
                    let rgb = term.colors()[index].unwrap_or_else(|| match index {
                        0..=255 => theme::indexed(index as u8),
                        256 | 267 => theme::rgb_from_hex(PALETTE.fg),
                        257 => theme::rgb_from_hex(PALETTE.bg),
                        _ => theme::rgb_from_hex(PALETTE.accent),
                    });
                    drop(term);
                    self.session.write(format(rgb).into_bytes());
                }
                SessionEvent::Term(TermEvent::TextAreaSizeRequest(format)) => self
                    .session
                    .write(format(self.session.size().into()).into_bytes()),
                SessionEvent::Term(TermEvent::ChildExit(status)) => {
                    log::info!("shell exited: {status}");
                }
                SessionEvent::Term(TermEvent::Exit) => {
                    window.remove_window();
                    return;
                }
                SessionEvent::Term(
                    TermEvent::Bell | TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange,
                ) => {}
                SessionEvent::Mark(record) => {
                    log::debug!("mark {record:?}");
                    if let Mark::Cwd { path, .. } = &record.mark {
                        self.cwd = Some(path.clone());
                    }
                    self.marks.push(record);
                }
            }
        }
        window.set_window_title(&self.window_title());
        cx.notify();
    }

    fn window_title(&self) -> String {
        match (&self.title, &self.cwd) {
            (Some(title), _) => title.clone(),
            (None, Some(cwd)) => cwd.clone(),
            (None, None) => self.session.shell().label(),
        }
    }

    pub(crate) fn on_first_frame(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.first_frame_logged {
            return;
        }
        self.first_frame_logged = true;
        let elapsed = self.started.elapsed();
        log::info!("first frame with shell output after {elapsed:?}");
        if self.bench_startup {
            println!("startup_ms={:.1}", elapsed.as_secs_f64() * 1000.);
            cx.quit();
        }
    }

    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_blink_on = true;
        self.blink_epoch += 1;
        let epoch = self.blink_epoch;
        self._blink = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(CURSOR_BLINK).await;
                let alive = this.update(cx, |this, cx| {
                    if this.blink_epoch != epoch {
                        return false;
                    }
                    this.cursor_blink_on = !this.cursor_blink_on;
                    cx.notify();
                    true
                });
                if !matches!(alive, Ok(true)) {
                    break;
                }
            }
        }));
    }

    /// Typing scrolls back to the live screen and keeps the cursor solid.
    fn before_input(&mut self, cx: &mut Context<Self>) {
        let mut term = self.session.term().lock();
        if term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
        }
        drop(term);
        self.restart_blink(cx);
    }

    fn mode(&self) -> TermMode {
        *self.session.term().lock().mode()
    }

    fn key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.platform || self.marked_text.is_some() {
            return;
        }
        let mods = mods(&keystroke.modifiers);
        if let Some(bytes) = input::encode_key(&keystroke.key, mods, self.mode(), ALT_MODE) {
            self.before_input(cx);
            self.session.term().lock().selection = None;
            self.session.write(bytes);
            cx.stop_propagation();
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        let text = self.session.term().lock().selection_to_string();
        if let Some(text) = text.filter(|t| !t.is_empty()) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        self.before_input(cx);
        let bytes = input::encode_paste(&text, self.mode());
        self.session.write(bytes);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        let mut term = self.session.term().lock();
        let start = GridPoint::new(Line(-(term.grid().history_size() as i32)), Column(0));
        let end = GridPoint::new(
            Line(term.screen_lines() as i32 - 1),
            Column(term.columns() - 1),
        );
        let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
        selection.update(end, Side::Right);
        term.selection = Some(selection);
        drop(term);
        cx.notify();
    }

    fn scroll(&mut self, scroll: Scroll, cx: &mut Context<Self>) {
        self.session.term().lock().scroll_display(scroll);
        cx.notify();
    }

    /// Grid cell under a window position, clamped to the grid, plus which half of the cell.
    fn cell_at(&self, position: Point<Pixels>) -> Option<(usize, usize, Side)> {
        let layout = self.layout?;
        let x = f32::from(position.x - layout.origin.x).max(0.);
        let y = f32::from(position.y - layout.origin.y).max(0.);
        let cell_w = f32::from(layout.metrics.cell_width);
        let cell_h = f32::from(layout.metrics.line_height);
        let column = ((x / cell_w) as usize).min(layout.columns.saturating_sub(1));
        let line = ((y / cell_h) as usize).min(layout.lines.saturating_sub(1));
        let side = if x / cell_w - (x / cell_w).floor() < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        Some((column, line, side))
    }

    fn grid_point(&self, column: usize, viewport_line: usize) -> GridPoint {
        let offset = self.session.term().lock().grid().display_offset() as i32;
        GridPoint::new(Line(viewport_line as i32 - offset), Column(column))
    }

    /// Sends a mouse event to the program if it asked for mouse reports. Shift always selects.
    fn report_mouse(
        &mut self,
        button: input::MouseButton,
        action: MouseAction,
        position: Point<Pixels>,
        modifiers: &Modifiers,
    ) -> bool {
        if modifiers.shift {
            return false;
        }
        let mode = self.mode();
        if !input::wants_mouse(action, mode) {
            return false;
        }
        let Some((column, line, _)) = self.cell_at(position) else {
            return false;
        };
        if matches!(action, MouseAction::Move(_)) && self.last_mouse_cell == Some((column, line)) {
            return true;
        }
        self.last_mouse_cell = Some((column, line));
        if let Some(bytes) =
            input::encode_mouse(button, action, column, line, mods(modifiers), mode)
        {
            self.session.write(bytes);
        }
        true
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let button = match event.button {
            MouseButton::Left => input::MouseButton::Left,
            MouseButton::Middle => input::MouseButton::Middle,
            MouseButton::Right => input::MouseButton::Right,
            _ => return,
        };
        if self.report_mouse(button, MouseAction::Press, event.position, &event.modifiers) {
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        let Some((column, line, side)) = self.cell_at(event.position) else {
            return;
        };
        let point = self.grid_point(column, line);
        let kind = match event.click_count {
            2 => SelectionType::Semantic,
            3.. => SelectionType::Lines,
            _ => SelectionType::Simple,
        };
        let mut term = self.session.term().lock();
        if event.modifiers.shift
            && let Some(selection) = term.selection.as_mut()
        {
            selection.update(point, side);
        } else {
            term.selection = Some(Selection::new(kind, point, side));
        }
        drop(term);
        self.selecting = true;
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let held = match event.pressed_button {
            Some(MouseButton::Left) => Some(input::MouseButton::Left),
            Some(MouseButton::Middle) => Some(input::MouseButton::Middle),
            Some(MouseButton::Right) => Some(input::MouseButton::Right),
            _ => None,
        };
        if !self.selecting
            && self.report_mouse(
                held.unwrap_or(input::MouseButton::Left),
                MouseAction::Move(held),
                event.position,
                &event.modifiers,
            )
        {
            return;
        }
        if !self.selecting || held != Some(input::MouseButton::Left) {
            return;
        }
        let Some((column, line, side)) = self.cell_at(event.position) else {
            return;
        };
        let point = self.grid_point(column, line);
        if let Some(selection) = self.session.term().lock().selection.as_mut() {
            selection.update(point, side);
        }
        cx.notify();
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let button = match event.button {
            MouseButton::Left => input::MouseButton::Left,
            MouseButton::Middle => input::MouseButton::Middle,
            MouseButton::Right => input::MouseButton::Right,
            _ => return,
        };
        if !self.selecting
            && self.report_mouse(
                button,
                MouseAction::Release,
                event.position,
                &event.modifiers,
            )
        {
            return;
        }
        if event.button == MouseButton::Left && self.selecting {
            self.selecting = false;
            let mut term = self.session.term().lock();
            if term.selection.as_ref().is_some_and(|s| s.is_empty()) {
                term.selection = None;
            }
            drop(term);
            cx.notify();
        }
    }

    fn scroll_wheel(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let line_height = f32::from(self.metrics.line_height);
        let lines = match event.delta {
            ScrollDelta::Lines(delta) => delta.y,
            ScrollDelta::Pixels(delta) => f32::from(delta.y) / line_height,
        } + self.scroll_remainder;
        let whole = lines.trunc() as i32;
        self.scroll_remainder = lines - whole as f32;
        if whole == 0 {
            return;
        }

        let mode = self.mode();
        let button = if whole > 0 {
            input::MouseButton::WheelUp
        } else {
            input::MouseButton::WheelDown
        };
        if !event.modifiers.shift && mode.intersects(TermMode::MOUSE_MODE) {
            for _ in 0..whole.unsigned_abs() {
                self.report_mouse(button, MouseAction::Press, event.position, &event.modifiers);
            }
            return;
        }
        if mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
            && !event.modifiers.shift
        {
            let key = if whole > 0 { "up" } else { "down" };
            for _ in 0..whole.unsigned_abs() {
                if let Some(bytes) = input::encode_key(key, Mods::NONE, mode, ALT_MODE) {
                    self.session.write(bytes);
                }
            }
            return;
        }
        self.scroll(Scroll::Delta(whole), cx);
    }

    /// The cursor cell in window coordinates, for the IME candidate window.
    fn cursor_bounds(&self) -> Option<Bounds<Pixels>> {
        let layout = self.layout?;
        let term = self.session.term().lock();
        let cursor = term.grid().cursor.point;
        let line = cursor.line.0 + term.grid().display_offset() as i32;
        let origin = layout.origin
            + Point::new(
                layout.metrics.cell_width * cursor.column.0 as f32,
                layout.metrics.line_height * line as f32,
            );
        Some(Bounds::new(
            origin,
            gpui::size(layout.metrics.cell_width, layout.metrics.line_height),
        ))
    }
}

fn mods(modifiers: &Modifiers) -> Mods {
    Mods {
        shift: modifiers.shift,
        alt: modifiers.alt,
        ctrl: modifiers.control,
    }
}

impl Focusable for TerminalView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity: Entity<Self> = cx.entity();
        div()
            .id("terminal")
            .key_context("Terminal")
            .track_focus(&self.focus)
            .size_full()
            .bg(theme::hsla(PALETTE.bg))
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(|this, _: &ScrollPageUp, _, cx| {
                let lines = this.layout.map_or(10, |l| l.lines as i32);
                this.scroll(Scroll::Delta(lines), cx)
            }))
            .on_action(cx.listener(|this, _: &ScrollPageDown, _, cx| {
                let lines = this.layout.map_or(10, |l| l.lines as i32);
                this.scroll(Scroll::Delta(-lines), cx)
            }))
            .on_action(cx.listener(|this, _: &ScrollToTop, _, cx| this.scroll(Scroll::Top, cx)))
            .on_action(
                cx.listener(|this, _: &ScrollToBottom, _, cx| this.scroll(Scroll::Bottom, cx)),
            )
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_scroll_wheel(cx.listener(Self::scroll_wheel))
            .child(TerminalElement::new(entity, self.focus.clone()))
    }
}

impl EntityInputHandler for TerminalView {
    fn text_for_range(
        &mut self,
        _: Range<usize>,
        _: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        None
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: 0..0,
            reversed: false,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_text
            .as_ref()
            .map(|text| 0..text.encode_utf16().count())
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_text = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = None;
        if text.is_empty() {
            return;
        }
        self.before_input(cx);
        self.session.term().lock().selection = None;
        self.session.write(text.as_bytes().to_vec());
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        _: Option<Range<usize>>,
        new_text: &str,
        _: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.marked_text = (!new_text.is_empty()).then(|| new_text.to_string());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        _: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        self.cursor_bounds()
    }

    fn character_index_for_point(
        &mut self,
        _: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        None
    }
}
