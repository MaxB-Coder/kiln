//! One terminal pane: a shell session shown as a timeline of command blocks, the running
//! command, and the prompt pinned at the bottom. Full-screen programs, and shells without
//! Kiln's integration, get the classic full grid instead.

use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use futures::StreamExt as _;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, ElementId, Entity, EntityInputHandler,
    EventEmitter, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent,
    ListAlignment, ListOffset, ListState, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ParentElement, Pixels, Point, Render, ScrollDelta, ScrollWheelEvent,
    SharedString, StatefulInteractiveElement, Styled, Task, UTF16Selection, Window, canvas, div,
    list, px,
};
use kiln_term::engine::event::Event as TermEvent;
use kiln_term::engine::grid::{Dimensions, Scroll};
use kiln_term::engine::index::{Column, Line, Point as GridPoint, Side};
use kiln_term::engine::selection::{Selection, SelectionType};
use kiln_term::engine::term::TermMode;
use kiln_term::engine::term::cell::Flags;
use kiln_term::engine::vte::ansi::{Color, NamedColor};
use kiln_term::input::{self, AltMode, Mods, MouseAction};
use kiln_term::{EngineTerm, GridSize, Mark, Phase, Session, SessionEvent, SessionOptions};

use crate::block_element::{BlockBody, FrozenBlock, TimelinePoint, TimelineSelection, WrappedRow};
use crate::copy;
use crate::grid_paint::{CellMetrics, FONT_FAMILY};
use crate::live_element::LiveElement;
use crate::search::{Match, SearchHighlights, find};
use crate::theme::{self, palette};
use crate::{
    Copy, CopyLastOutput, CopyLastOutputRaw, Paste, ScrollPageDown, ScrollPageUp, ScrollToBottom,
    ScrollToTop, SelectAll,
};

/// Option composes characters on macOS unless the config says it is Alt; Alt sends Esc+key
/// everywhere else.
fn alt_mode() -> AltMode {
    if cfg!(target_os = "macos") && !crate::config::current().option_as_alt {
        AltMode::Compose
    } else {
        AltMode::Escape
    }
}

const CURSOR_BLINK: Duration = Duration::from_millis(600);

/// Space around the grid, in points (same as the WezTerm config).
pub const PADDING_X: f32 = 14.;
pub const PADDING_TOP: f32 = 10.;
pub const PADDING_BOTTOM: f32 = 8.;

/// Which part of the live grid an element shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveRole {
    /// The whole screen: full-screen programs, or a shell without integration.
    Classic,
    /// The running command's output.
    Running,
    /// The prompt, in the input bar.
    Prompt,
}

/// Where something was drawn this frame, for mapping the mouse back to cells.
#[derive(Clone, Debug)]
pub enum HitRegion {
    Live {
        role: LiveRole,
        bounds: Bounds<Pixels>,
        top: usize,
        rows: usize,
        columns: usize,
    },
    Block {
        item: usize,
        bounds: Bounds<Pixels>,
        rows: Rc<Vec<WrappedRow>>,
    },
}

enum Hit {
    Live {
        role: LiveRole,
        point: GridPoint,
        side: Side,
    },
    Block {
        point: TimelinePoint,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Selecting {
    Live,
    Timeline,
}

/// How a pane looks from the sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneStatus {
    Idle,
    Running,
    Failed,
}

pub enum PaneEvent {
    /// Title, folder or status changed.
    Changed,
    /// The shell exited.
    Exited,
    /// "Open log" on a block: the workspace shows the file in a split.
    OpenLog(PathBuf),
    /// The program rang the terminal bell.
    Bell,
    /// A command that ran at least `notify_after_secs` finished.
    LongCommandFinished {
        command: String,
        duration: Duration,
        exit_code: Option<i32>,
        cwd: Option<String>,
    },
}

impl EventEmitter<PaneEvent> for TerminalView {}

/// The folder and the runtimes on PATH that the input bar's context was computed for.
type ContextKey = (Option<String>, Vec<(String, String)>);

/// Facts about a pane's session, for the rail.
#[derive(Clone, Debug)]
pub struct SessionInfo {
    pub shell: String,
    pub pid: Option<u32>,
    pub columns: usize,
    pub lines: usize,
    pub started: Instant,
    pub commands: usize,
    pub last_exit: Option<i32>,
    /// The host of a running ssh or mosh command.
    pub remote_host: Option<String>,
}

pub struct TerminalView {
    pub(crate) session: Session,
    focus: FocusHandle,
    pub(crate) metrics: CellMetrics,
    /// Text the input method is composing (dead keys, IME); drawn at the cursor.
    pub(crate) marked_text: Option<String>,
    pub(crate) cursor_blink_on: bool,
    blink_epoch: usize,
    pub(crate) hit_regions: Vec<HitRegion>,
    selecting: Option<Selecting>,
    pub(crate) timeline_selection: Option<TimelineSelection>,
    scroll_remainder: f32,
    /// Last mouse cell reported to a program, to avoid repeating motion reports.
    last_mouse_cell: Option<(usize, i32)>,
    pub(crate) blocks: Vec<Rc<FrozenBlock>>,
    /// The block under the mouse: it shows at full strength even when old.
    hovered_block: Option<u64>,
    list_state: ListState,
    list_blocks: usize,
    list_running: bool,
    search_highlights: SearchHighlights,
    title: Option<String>,
    cwd: Option<String>,
    /// Dim context in the input bar ("node 22.3.0", "WSL · Ubuntu").
    context: Option<String>,
    context_for: Option<ContextKey>,
    runtimes: Vec<(String, String)>,
    _context_task: Option<Task<()>>,
    started: Instant,
    pub(crate) first_frame_logged: bool,
    first_output_logged: bool,
    bench_startup: bool,
    /// `KILN_TRACE_LATENCY=1`: logs keystroke → first frame painted after the shell answered.
    trace_latency: bool,
    input_at: Option<Instant>,
    pub(crate) output_since_input: bool,
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
        cx.on_focus_in(&focus, window, |this, _, cx| this.restart_blink(cx))
            .detach();
        cx.on_focus_out(&focus, window, |this, _, _, cx| {
            this._blink = None;
            this.cursor_blink_on = true;
            cx.notify();
        })
        .detach();

        let list_state = ListState::new(0, ListAlignment::Bottom, px(600.));
        list_state.set_follow_mode(gpui::FollowMode::Tail);

        let mut this = Self {
            session,
            focus,
            metrics,
            marked_text: None,
            cursor_blink_on: true,
            blink_epoch: 0,
            hit_regions: Vec::new(),
            selecting: None,
            timeline_selection: None,
            scroll_remainder: 0.,
            last_mouse_cell: None,
            blocks: Vec::new(),
            hovered_block: None,
            list_state,
            list_blocks: 0,
            list_running: false,
            search_highlights: SearchHighlights::default(),
            title: None,
            cwd: None,
            context: None,
            context_for: None,
            runtimes: Vec::new(),
            _context_task: None,
            started,
            first_frame_logged: false,
            first_output_logged: false,
            bench_startup,
            trace_latency: std::env::var_os("KILN_TRACE_LATENCY").is_some(),
            input_at: None,
            output_since_input: false,
            _events: events,
            _blink: None,
        };
        this.restart_blink(cx);
        this
    }

    // ----- session events -------------------------------------------------------------------

    fn handle_events(&mut self, batch: Vec<SessionEvent>, _: &mut Window, cx: &mut Context<Self>) {
        let mut cleared = false;
        for event in batch {
            match event {
                SessionEvent::Term(TermEvent::Wakeup) => {
                    if !self.first_output_logged {
                        self.first_output_logged = true;
                        log::info!("first shell output after {:?}", self.started.elapsed());
                    }
                    if self.input_at.is_some() {
                        self.output_since_input = true;
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
                        256 | 267 => theme::rgb_from_hex(palette().fg),
                        257 => theme::rgb_from_hex(palette().bg),
                        _ => theme::rgb_from_hex(palette().accent),
                    });
                    drop(term);
                    self.session.write(format(rgb).into_bytes());
                }
                SessionEvent::Term(TermEvent::TextAreaSizeRequest(format)) => self
                    .session
                    .write(format(self.session.size().into()).into_bytes()),
                SessionEvent::Term(TermEvent::ChildExit(status)) => {
                    log::info!("shell exited: {status}")
                }
                SessionEvent::Term(TermEvent::Exit) => {
                    cx.emit(PaneEvent::Exited);
                    return;
                }
                SessionEvent::Term(TermEvent::Bell) => cx.emit(PaneEvent::Bell),
                SessionEvent::Term(
                    TermEvent::MouseCursorDirty | TermEvent::CursorBlinkingChange,
                ) => {}
                SessionEvent::Mark(record) => {
                    log::debug!("mark {record:?}");
                    if let Mark::Cwd { path, .. } = &record.mark {
                        self.cwd = Some(path.clone());
                    }
                }
                SessionEvent::Block(block) => {
                    let notify_after = crate::config::current().notify_after_secs;
                    if notify_after > 0
                        && !block.startup
                        && !block.command.is_empty()
                        && block.duration >= Duration::from_secs(notify_after)
                    {
                        cx.emit(PaneEvent::LongCommandFinished {
                            command: block.command.clone(),
                            duration: block.duration,
                            exit_code: block.exit_code,
                            cwd: block.cwd.clone(),
                        });
                    }
                    if block.clears_timeline() {
                        self.blocks.retain(|b| b.pinned.get());
                        cleared = true;
                    } else {
                        self.blocks.push(Rc::new(FrozenBlock::new(block)));
                    }
                    self.timeline_selection = None;
                }
            }
        }
        self.sync_list(cleared);
        self.refresh_context(cx);
        cx.emit(PaneEvent::Changed);
        cx.notify();
    }

    /// Recomputes the project's runtimes and the input bar's context when the folder or the
    /// tools on PATH changed. Version lookups may run a tool once, so they run in the background.
    fn refresh_context(&mut self, cx: &mut Context<Self>) {
        let live = self.session.live();
        let key = (live.cwd.clone(), live.tools.clone());
        if self.context_for.as_ref() == Some(&key) {
            return;
        }
        self.context_for = Some(key.clone());
        let wsl = crate::context::wsl_label(self.session.shell());
        self._context_task = Some(cx.spawn(async move |this, cx| {
            let runtimes = cx
                .background_executor()
                .spawn(async move {
                    let (cwd, tools) = key;
                    cwd.map(|cwd| crate::context::project_runtimes(&cwd, &tools))
                        .unwrap_or_default()
                })
                .await;
            let mut parts: Vec<String> = runtimes
                .iter()
                .take(2)
                .map(|(name, version)| format!("{name} {version}"))
                .collect();
            parts.extend(wsl);
            let _ = this.update(cx, |this, cx| {
                this.context = (!parts.is_empty()).then(|| parts.join("  ·  "));
                this.runtimes = runtimes;
                cx.emit(PaneEvent::Changed);
                cx.notify();
            });
        }));
    }

    /// Language runtimes the current project uses, with versions.
    pub fn runtimes(&self) -> &[(String, String)] {
        &self.runtimes
    }

    /// Keeps the list's items (blocks, then the running command) in step with the session.
    fn sync_list(&mut self, cleared: bool) {
        let running = self.session.live().phase == Phase::Running;
        if cleared {
            self.list_state.reset(self.blocks.len() + running as usize);
            self.list_state.set_follow_mode(gpui::FollowMode::Tail);
        } else {
            let start = self.list_blocks.min(self.blocks.len());
            let old_len = self.list_running as usize;
            let new_len = (self.blocks.len() - start) + running as usize;
            if old_len > 0 || new_len > 0 {
                self.list_state.splice(start..start + old_len, new_len);
            }
        }
        self.list_blocks = self.blocks.len();
        self.list_running = running;
    }

    pub fn title(&self) -> String {
        match (&self.title, &self.cwd) {
            (Some(title), _) => title.clone(),
            (None, Some(cwd)) => copy::short_folder(cwd),
            (None, None) => self.session.shell().label(),
        }
    }

    pub fn cwd(&self) -> Option<&str> {
        self.cwd.as_deref()
    }

    pub fn status(&self) -> PaneStatus {
        if self.session.live().phase == Phase::Running {
            PaneStatus::Running
        } else if self
            .last_command_index()
            .is_some_and(|ix| self.blocks[ix].block.failed())
        {
            PaneStatus::Failed
        } else {
            PaneStatus::Idle
        }
    }

    /// Commands run in this pane, oldest first.
    pub fn commands(&self) -> Vec<String> {
        self.blocks
            .iter()
            .filter(|b| !b.block.startup && !b.block.command.is_empty())
            .map(|b| b.block.command.clone())
            .collect()
    }

    pub fn session_info(&self) -> SessionInfo {
        let size = self.session.size();
        SessionInfo {
            shell: self.session.shell().label(),
            pid: self.session.pid(),
            columns: size.columns,
            lines: size.lines,
            started: self.started,
            commands: self.commands().len(),
            last_exit: self.session.live().last_exit,
            remote_host: self.remote_host(),
        }
    }

    /// The tint for a pane connected to another machine, with its host.
    pub fn tint(&self) -> Option<(crate::remote::Tint, String)> {
        let host = self.remote_host()?;
        let tint = crate::remote::Tint::for_host(&host, &crate::config::current().tints)?;
        Some((tint, host))
    }

    /// The machine the running command is connected to (`ssh`, `mosh`).
    pub fn remote_host(&self) -> Option<String> {
        let live = self.session.live();
        if live.phase != Phase::Running {
            return None;
        }
        live.command.as_deref().and_then(crate::remote::remote_host)
    }

    // ----- startup benchmark and latency trace ----------------------------------------------

    /// Called after a frame is painted. Logs input latency when tracing is on.
    pub(crate) fn frame_painted(&mut self) {
        if self.output_since_input
            && let Some(at) = self.input_at.take()
        {
            self.output_since_input = false;
            log::info!(
                "latency: keystroke to frame painted {:.2} ms",
                at.elapsed().as_secs_f64() * 1000.
            );
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
            let line = format!("startup_ms={:.1}", elapsed.as_secs_f64() * 1000.);
            println!("{line}");
            // Windows release builds have no console of their own: scripts read the file.
            if let Some(path) = std::env::var_os("KILN_BENCH_FILE") {
                let _ = std::fs::write(path, format!("{line}\n"));
            }
            cx.quit();
        }
    }

    // ----- cursor blink ---------------------------------------------------------------------

    fn restart_blink(&mut self, cx: &mut Context<Self>) {
        self.cursor_blink_on = true;
        self.blink_epoch += 1;
        if !crate::config::current().cursor_blink {
            self._blink = None;
            return;
        }
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

    // ----- input ----------------------------------------------------------------------------

    /// Typing scrolls back to the newest output, drops selections and keeps the cursor solid.
    fn before_input(&mut self, cx: &mut Context<Self>) {
        if self.trace_latency {
            self.input_at = Some(Instant::now());
            self.output_since_input = false;
        }
        let mut term = self.session.term().lock();
        if term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
        }
        term.selection = None;
        drop(term);
        self.timeline_selection = None;
        self.list_state.scroll_to_end();
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
        if let Some(bytes) = input::encode_key(&keystroke.key, mods, self.mode(), alt_mode()) {
            self.before_input(cx);
            self.session.write(bytes);
            cx.stop_propagation();
        }
    }

    /// Sends text to the shell as if typed (rerun, the palette).
    pub fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.before_input(cx);
        self.session.write(text.as_bytes().to_vec());
        cx.notify();
    }

    pub fn is_at_prompt(&self) -> bool {
        self.session.live().phase == Phase::Prompt
    }

    // ----- clipboard and block actions ------------------------------------------------------

    fn selection_text(&self) -> Option<String> {
        if let Some(selection) = self.timeline_selection.filter(|s| !s.is_empty()) {
            let text = selection.text(&self.blocks, self.session.size().columns);
            return (!text.is_empty()).then_some(text);
        }
        self.session
            .term()
            .lock()
            .selection_to_string()
            .filter(|t| !t.is_empty())
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.selection_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        } else if cfg!(not(target_os = "macos")) {
            // Ctrl+Shift+C with nothing selected copies the last output.
            self.copy_last(false, cx);
        }
    }

    fn last_command_index(&self) -> Option<usize> {
        self.blocks.iter().rposition(|b| !b.block.startup)
    }

    fn copy_last(&mut self, raw: bool, cx: &mut Context<Self>) {
        if let Some(ix) = self.last_command_index() {
            self.copy_block(ix, raw, cx);
        }
    }

    fn copy_last_output(&mut self, _: &CopyLastOutput, _: &mut Window, cx: &mut Context<Self>) {
        self.copy_last(false, cx);
    }

    fn copy_last_output_raw(
        &mut self,
        _: &CopyLastOutputRaw,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_last(true, cx);
    }

    pub fn copy_block(&mut self, item: usize, raw: bool, cx: &mut Context<Self>) {
        if let Some(block) = self.blocks.get(item) {
            let text = if raw {
                block.block.output_text()
            } else {
                copy::fenced(&block.block)
            };
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    pub fn rerun_block(&mut self, item: usize, cx: &mut Context<Self>) {
        let Some(command) = self.blocks.get(item).map(|b| b.block.command.clone()) else {
            return;
        };
        self.run_command(&command, cx);
    }

    /// Runs a command at the prompt, as if typed and entered. Does nothing while a command runs.
    pub fn run_command(&mut self, command: &str, cx: &mut Context<Self>) {
        if self.is_at_prompt() && !command.trim().is_empty() {
            // Ctrl+E Ctrl+U clears whatever is on the line first.
            self.type_text(&format!("\x05\x15{command}\r"), cx);
        }
    }

    pub fn toggle_pin(&mut self, item: usize, cx: &mut Context<Self>) {
        if let Some(block) = self.blocks.get(item) {
            block.pinned.set(!block.pinned.get());
            cx.notify();
        }
    }

    pub fn open_log(&mut self, item: usize, cx: &mut Context<Self>) {
        let Some(block) = self.blocks.get(item) else {
            return;
        };
        match crate::logs::write_log(&block.block) {
            Ok(path) => cx.emit(PaneEvent::OpenLog(path)),
            Err(err) => log::error!("could not write the log: {err:#}"),
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
        if self.in_classic_mode() || self.blocks.is_empty() {
            let mut term = self.session.term().lock();
            let start = GridPoint::new(Line(-(term.grid().history_size() as i32)), Column(0));
            let end = GridPoint::new(
                Line(term.screen_lines() as i32 - 1),
                Column(term.columns() - 1),
            );
            let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
            selection.update(end, Side::Right);
            term.selection = Some(selection);
        } else {
            let last = self.blocks.len() - 1;
            let rows = self.blocks[last].rows(self.session.size().columns).len();
            self.timeline_selection = Some(TimelineSelection {
                anchor: TimelinePoint {
                    item: 0,
                    row: 0,
                    column: 0,
                },
                head: TimelinePoint {
                    item: last,
                    row: rows.saturating_sub(1),
                    column: usize::MAX / 2,
                },
            });
        }
        cx.notify();
    }

    // ----- scrolling ------------------------------------------------------------------------

    fn in_classic_mode(&self) -> bool {
        let term = self.session.term().lock();
        let live = self.session.live();
        term.mode().contains(TermMode::ALT_SCREEN)
            || !live.integrated
            || live.phase == Phase::Unknown
    }

    fn scroll_lines(&mut self, lines: i32, cx: &mut Context<Self>) {
        if self.in_classic_mode() {
            self.session
                .term()
                .lock()
                .scroll_display(Scroll::Delta(lines));
        } else {
            self.list_state
                .scroll_by(-self.metrics.line_height * lines as f32);
        }
        cx.notify();
    }

    fn scroll_page(&mut self, up: bool, cx: &mut Context<Self>) {
        let lines = self.session.size().lines.max(2) as i32 - 1;
        self.scroll_lines(if up { lines } else { -lines }, cx);
    }

    fn scroll_to_top(&mut self, cx: &mut Context<Self>) {
        if self.in_classic_mode() {
            self.session.term().lock().scroll_display(Scroll::Top);
        } else {
            self.list_state.scroll_to(ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
        }
        cx.notify();
    }

    fn scroll_to_bottom(&mut self, cx: &mut Context<Self>) {
        if self.in_classic_mode() {
            self.session.term().lock().scroll_display(Scroll::Bottom);
        } else {
            self.list_state.scroll_to_end();
        }
        cx.notify();
    }

    // ----- mouse ----------------------------------------------------------------------------

    /// What is under a window position. With `clamp_to`, the nearest region of that kind counts
    /// (dragging a selection past the edge).
    fn hit(&self, position: Point<Pixels>, clamp_to: Option<Selecting>) -> Option<Hit> {
        let metrics = self.metrics;
        let region = self
            .hit_regions
            .iter()
            .rev()
            .find(|r| region_bounds(r).contains(&position))
            .or_else(|| {
                let kind = clamp_to?;
                self.hit_regions
                    .iter()
                    .filter(|r| {
                        matches!(
                            (kind, r),
                            (Selecting::Live, HitRegion::Live { .. })
                                | (Selecting::Timeline, HitRegion::Block { .. })
                        )
                    })
                    .min_by(|a, b| {
                        let da = vertical_distance(region_bounds(a), position);
                        let db = vertical_distance(region_bounds(b), position);
                        da.total_cmp(&db)
                    })
            })?;
        let bounds = region_bounds(region);
        let x = f32::from(position.x - bounds.origin.x).max(0.);
        let y = f32::from(position.y - bounds.origin.y);
        let cell_w = f32::from(metrics.cell_width);
        let line_h = f32::from(metrics.line_height);
        let column = (x / cell_w) as usize;
        let side = if x / cell_w - (x / cell_w).floor() < 0.5 {
            Side::Left
        } else {
            Side::Right
        };
        match region {
            HitRegion::Live {
                role,
                top,
                rows,
                columns,
                ..
            } => {
                let row = ((y / line_h).floor().max(0.) as usize).min(rows.saturating_sub(1));
                let history = self.session.term().lock().grid().history_size() as i32;
                let line = Line(*top as i32 + row as i32 - history);
                let column = column.min(columns.saturating_sub(1));
                Some(Hit::Live {
                    role: *role,
                    point: GridPoint::new(line, Column(column)),
                    side,
                })
            }
            HitRegion::Block { item, rows, .. } => {
                if rows.is_empty() {
                    return None;
                }
                let row = if y < 0. {
                    0
                } else {
                    ((y / line_h) as usize).min(rows.len() - 1)
                };
                Some(Hit::Block {
                    point: TimelinePoint {
                        item: *item,
                        row,
                        column,
                    },
                })
            }
        }
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
        let Some(Hit::Live {
            role: LiveRole::Classic | LiveRole::Running,
            point,
            ..
        }) = self.hit(position, None)
        else {
            return false;
        };
        if point.line.0 < 0 {
            return false;
        }
        let cell = (point.column.0, point.line.0);
        if matches!(action, MouseAction::Move(_)) && self.last_mouse_cell == Some(cell) {
            return true;
        }
        self.last_mouse_cell = Some(cell);
        if let Some(bytes) = input::encode_mouse(
            button,
            action,
            point.column.0,
            point.line.0 as usize,
            mods(modifiers),
            mode,
        ) {
            self.session.write(bytes);
        }
        true
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let Some(button) = mouse_button(event.button) else {
            return;
        };
        if self.report_mouse(button, MouseAction::Press, event.position, &event.modifiers) {
            return;
        }
        if event.button != MouseButton::Left {
            return;
        }
        match self.hit(event.position, None) {
            Some(Hit::Live { point, side, .. }) => {
                self.timeline_selection = None;
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
                self.selecting = Some(Selecting::Live);
            }
            Some(Hit::Block { point }) => {
                self.session.term().lock().selection = None;
                let selection = match (event.click_count, self.timeline_selection) {
                    (2, _) => self.word_at(point),
                    (3.., _) => self.line_at(point),
                    (_, Some(mut existing)) if event.modifiers.shift => {
                        existing.head = point;
                        existing
                    }
                    _ => TimelineSelection {
                        anchor: point,
                        head: point,
                    },
                };
                self.timeline_selection = Some(selection);
                self.selecting = Some(Selecting::Timeline);
            }
            None => {
                self.timeline_selection = None;
                self.session.term().lock().selection = None;
            }
        }
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let held = event.pressed_button.and_then(mouse_button);
        if self.selecting.is_none()
            && self.report_mouse(
                held.unwrap_or(input::MouseButton::Left),
                MouseAction::Move(held),
                event.position,
                &event.modifiers,
            )
        {
            return;
        }
        if held != Some(input::MouseButton::Left) {
            return;
        }
        match (self.selecting, self.hit(event.position, self.selecting)) {
            (Some(Selecting::Live), Some(Hit::Live { point, side, .. })) => {
                if let Some(selection) = self.session.term().lock().selection.as_mut() {
                    selection.update(point, side);
                }
            }
            (Some(Selecting::Timeline), Some(Hit::Block { point })) => {
                if let Some(selection) = self.timeline_selection.as_mut() {
                    selection.head = point;
                }
            }
            _ => return,
        }
        cx.notify();
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(button) = mouse_button(event.button) else {
            return;
        };
        if self.selecting.is_none()
            && self.report_mouse(
                button,
                MouseAction::Release,
                event.position,
                &event.modifiers,
            )
        {
            return;
        }
        if event.button == MouseButton::Left && self.selecting.take().is_some() {
            let mut term = self.session.term().lock();
            if term.selection.as_ref().is_some_and(|s| s.is_empty()) {
                term.selection = None;
            }
            drop(term);
            cx.notify();
        }
    }

    fn word_at(&self, point: TimelinePoint) -> TimelineSelection {
        let block = &self.blocks[point.item];
        let rows = block.rows(self.session.size().columns);
        let cells = rows
            .get(point.row)
            .map(|row| block.cells(*row))
            .unwrap_or(&[]);
        let is_word = |i: usize| {
            cells.get(i).is_some_and(|c| {
                !c.c.is_whitespace()
                    && !"│`|:\"'()[]{}<>,;".contains(c.c)
                    && !c.flags.contains(Flags::WIDE_CHAR_SPACER)
            })
        };
        let mut start = point.column.min(cells.len().saturating_sub(1));
        let mut end = start;
        if is_word(start) {
            while start > 0 && is_word(start - 1) {
                start -= 1;
            }
            while is_word(end + 1) {
                end += 1;
            }
        }
        TimelineSelection {
            anchor: TimelinePoint {
                column: start,
                ..point
            },
            head: TimelinePoint {
                column: end,
                ..point
            },
        }
    }

    fn line_at(&self, point: TimelinePoint) -> TimelineSelection {
        let block = &self.blocks[point.item];
        let rows = block.rows(self.session.size().columns);
        let line = rows.get(point.row).map_or(0, |r| r.line);
        let first = rows
            .iter()
            .position(|r| r.line == line)
            .unwrap_or(point.row);
        let last = rows
            .iter()
            .rposition(|r| r.line == line)
            .unwrap_or(point.row);
        TimelineSelection {
            anchor: TimelinePoint {
                row: first,
                column: 0,
                ..point
            },
            head: TimelinePoint {
                row: last,
                column: usize::MAX / 2,
                ..point
            },
        }
    }

    fn scroll_wheel(&mut self, event: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !self.in_classic_mode() {
            // The timeline list scrolls itself.
            return;
        }
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
                if let Some(bytes) = input::encode_key(key, Mods::NONE, mode, alt_mode()) {
                    self.session.write(bytes);
                }
            }
            return;
        }
        self.session
            .term()
            .lock()
            .scroll_display(Scroll::Delta(whole));
        cx.notify();
    }

    // ----- search ---------------------------------------------------------------------------

    pub fn search_highlights(&self) -> SearchHighlights {
        self.search_highlights.clone()
    }

    /// Runs a search, highlights the matches and returns them (oldest first).
    pub fn set_search(&mut self, query: &str, cx: &mut Context<Self>) -> Vec<Match> {
        let classic = self.in_classic_mode();
        let live_top = {
            let live = self.session.live();
            live.output_top.or(live.prompt_top).unwrap_or(0)
        };
        let matches = {
            let term = self.session.term().lock();
            find(
                query,
                &self.blocks,
                &term,
                if classic { 0 } else { live_top },
            )
        };
        self.search_highlights = SearchHighlights::from_matches(&matches);
        cx.notify();
        matches
    }

    /// Scrolls so a match is visible.
    pub fn reveal_match(&mut self, m: Match, cx: &mut Context<Self>) {
        match m {
            Match::Header { item, .. } => {
                self.list_state.scroll_to(ListOffset {
                    item_ix: item,
                    offset_in_item: px(0.),
                });
            }
            Match::Block { item, line, .. } => {
                let columns = self.session.size().columns;
                let row = self.blocks[item]
                    .rows(columns)
                    .iter()
                    .position(|r| r.line == line)
                    .unwrap_or(0);
                // Header and spacing sit above the output.
                let offset = self.metrics.line_height * (row as f32 + 1.) + px(18.);
                self.list_state.scroll_to(ListOffset {
                    item_ix: item,
                    offset_in_item: (offset - px(80.)).max(px(0.)),
                });
            }
            Match::Live { line, .. } => {
                if self.in_classic_mode() {
                    let mut term = self.session.term().lock();
                    let screen = term.screen_lines() as i32;
                    let offset = term.grid().display_offset() as i32;
                    let visible_top = -offset;
                    if line.0 < visible_top || line.0 >= visible_top + screen {
                        let target = (-line.0 + screen / 2).max(0);
                        term.scroll_display(Scroll::Delta(target - offset));
                    }
                } else {
                    self.list_state.scroll_to_end();
                }
            }
        }
        cx.notify();
    }

    // ----- layout ---------------------------------------------------------------------------

    /// Computes the grid size from the pane's bounds and resizes the session.
    fn resize_to(&mut self, bounds: Bounds<Pixels>, window: &mut Window) {
        let metrics = CellMetrics::measure(self.metrics.font_size, window);
        let usable_w = f32::from(bounds.size.width) - 2. * PADDING_X;
        let usable_h = f32::from(bounds.size.height) - PADDING_TOP - PADDING_BOTTOM;
        let columns = ((usable_w / f32::from(metrics.cell_width)) as usize).max(2);
        let lines = ((usable_h / f32::from(metrics.line_height)) as usize).max(1);
        let scale = window.scale_factor();
        if metrics != self.metrics {
            self.metrics = metrics;
            self.list_state.remeasure();
        }
        self.session.resize(GridSize {
            columns,
            lines,
            cell_width: (f32::from(metrics.cell_width) * scale) as u16,
            cell_height: (f32::from(metrics.line_height) * scale) as u16,
        });
    }

    pub fn set_font_size(&mut self, font_size: Pixels, cx: &mut Context<Self>) {
        self.metrics.font_size = font_size;
        self.list_state.remeasure();
        cx.notify();
    }

    /// The last row (absolute) at or after `from` that holds anything, or the cursor's row.
    fn last_used_row(term: &EngineTerm, from: usize, cursor_abs: usize) -> usize {
        let grid = term.grid();
        let history = grid.history_size();
        let bottom = history + grid.screen_lines() - 1;
        let columns = grid.columns();
        let floor = cursor_abs.max(from);
        let mut abs = bottom;
        while abs > floor {
            let row = &grid[Line(abs as i32 - history as i32)];
            let used = (0..columns).any(|c| {
                let cell = &row[Column(c)];
                cell.c != ' ' || cell.bg != Color::Named(NamedColor::Background)
            });
            if used {
                break;
            }
            abs -= 1;
        }
        abs.max(floor)
    }

    fn render_block(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let frozen = self.blocks[ix].clone();
        let block = &frozen.block;
        let latest = Some(ix) == self.last_command_index();
        let pinned = frozen.pinned.get();
        let columns = self.session.size().columns;
        let has_output = !frozen.rows(columns).is_empty();
        let group: SharedString = format!("block-{}", block.id).into();
        let small = px(11.);

        fn action(
            label: &'static str,
            ix: usize,
            cx: &mut Context<TerminalView>,
            f: fn(&mut TerminalView, usize, &mut Context<TerminalView>),
        ) -> impl IntoElement {
            div()
                .id(ElementId::NamedInteger(label.into(), ix as u64))
                .px(px(4.))
                .text_size(px(11.))
                .text_color(theme::hsla(palette().muted))
                .hover(|s| s.text_color(theme::hsla(palette().fg)))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(move |this, _, _, cx| f(this, ix, cx)))
                .child(label)
        }

        let title = if block.startup {
            div()
                .text_color(theme::hsla(palette().muted))
                .text_size(small)
                .child("shell started")
        } else {
            // Find highlights in the command line.
            let mut title = div()
                .flex()
                .flex_row()
                .text_color(theme::hsla(palette().fg))
                .whitespace_nowrap()
                .overflow_hidden();
            let mut at = 0;
            for &(start, end) in self.search_highlights.header_ranges(ix) {
                if start > at {
                    title = title.child(block.command[at..start].to_string());
                }
                title = title.child(
                    div()
                        .bg(theme::hsla(palette().modified))
                        .text_color(theme::hsla(palette().bg))
                        .child(block.command[start..end].to_string()),
                );
                at = end;
            }
            title.child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(block.command[at..].to_string()),
            )
        };
        let meta = if block.startup {
            String::new()
        } else {
            copy::duration_label(block.duration)
        };

        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .h(self.metrics.line_height)
            .font_family(FONT_FAMILY)
            .text_size(self.metrics.font_size)
            .when(block.failed(), |d| {
                d.child(
                    div()
                        .flex_none()
                        .size(px(6.))
                        .rounded_full()
                        .bg(theme::hsla(palette().failed)),
                )
            })
            .when(pinned, |d| {
                d.child(
                    div()
                        .flex_none()
                        .size(px(6.))
                        .rounded_full()
                        .bg(theme::hsla(palette().accent)),
                )
            })
            .child(div().flex_1().min_w_0().child(title))
            .when(!block.startup, |d| {
                d.child(
                    div()
                        .flex()
                        .flex_row()
                        .gap(px(6.))
                        .invisible()
                        .group_hover(group.clone(), |s| s.visible())
                        .child(action("Copy", ix, cx, |this, ix, cx| {
                            this.copy_block(ix, false, cx)
                        }))
                        .child(action("Rerun", ix, cx, |this, ix, cx| {
                            this.rerun_block(ix, cx)
                        }))
                        .child(action(
                            if pinned { "Unpin" } else { "Pin" },
                            ix,
                            cx,
                            |this, ix, cx| this.toggle_pin(ix, cx),
                        ))
                        .child(action("Open log", ix, cx, |this, ix, cx| {
                            this.open_log(ix, cx)
                        })),
                )
            })
            .child(
                div()
                    .flex_none()
                    .text_size(small)
                    .text_color(theme::hsla(palette().muted))
                    .child(meta),
            );

        let id = block.id;
        let dimmed = !latest && !pinned && self.hovered_block != Some(id);
        div()
            .id(ElementId::NamedInteger("block".into(), block.id))
            .group(group)
            .w_full()
            .pt(px(14.))
            .flex()
            .flex_col()
            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                if *hovered {
                    this.hovered_block = Some(id);
                } else if this.hovered_block == Some(id) {
                    this.hovered_block = None;
                }
                cx.notify();
            }))
            .when(dimmed, |d| d.opacity(0.6))
            .child(header)
            .when(has_output, |d| {
                d.child(div().pt(px(4.)).child(BlockBody::new(
                    entity,
                    ix,
                    frozen.clone(),
                    columns,
                    self.metrics,
                )))
            })
            .when(block.truncated, |d| {
                d.child(
                    div()
                        .pt(px(2.))
                        .text_size(small)
                        .text_color(theme::hsla(palette().muted))
                        .child("earlier output did not fit in the scrollback"),
                )
            })
            .into_any_element()
    }

    fn render_running(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let live = self.session.live();
        let (top, rows) = {
            let term = self.session.term().lock();
            let grid = term.grid();
            let cursor_abs = grid.history_size() + grid.cursor.point.line.0.max(0) as usize;
            let top = live.output_top.unwrap_or(cursor_abs).min(cursor_abs);
            let last = Self::last_used_row(&term, top, cursor_abs);
            (top, last - top + 1)
        };
        let elapsed = live
            .command_started
            .and_then(|t| t.elapsed().ok())
            .map(copy::duration_label)
            .unwrap_or_default();
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .h(self.metrics.line_height)
            .font_family(FONT_FAMILY)
            .text_size(self.metrics.font_size)
            .child(
                div()
                    .flex_none()
                    .size(px(6.))
                    .rounded_full()
                    .bg(theme::hsla(palette().ok)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_color(theme::hsla(palette().fg))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(live.command.clone().unwrap_or_default()),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(11.))
                    .text_color(theme::hsla(palette().muted))
                    .child(elapsed),
            );
        div()
            .w_full()
            .pt(px(14.))
            .pb(px(PADDING_BOTTOM))
            .flex()
            .flex_col()
            .child(header)
            .child(div().pt(px(4.)).child(LiveElement::new(
                entity,
                self.focus.clone(),
                LiveRole::Running,
                top,
                rows,
            )))
            .into_any_element()
    }

    fn render_item(&mut self, ix: usize, _: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        if ix < self.blocks.len() {
            self.render_block(ix, cx)
        } else {
            self.render_running(cx)
        }
    }
}

fn region_bounds(region: &HitRegion) -> Bounds<Pixels> {
    match region {
        HitRegion::Live { bounds, .. } | HitRegion::Block { bounds, .. } => *bounds,
    }
}

fn vertical_distance(bounds: Bounds<Pixels>, position: Point<Pixels>) -> f32 {
    if position.y < bounds.top() {
        f32::from(bounds.top() - position.y)
    } else if position.y > bounds.bottom() {
        f32::from(position.y - bounds.bottom())
    } else {
        0.
    }
}

fn mouse_button(button: MouseButton) -> Option<input::MouseButton> {
    match button {
        MouseButton::Left => Some(input::MouseButton::Left),
        MouseButton::Middle => Some(input::MouseButton::Middle),
        MouseButton::Right => Some(input::MouseButton::Right),
        _ => None,
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
        self.hit_regions.clear();
        let entity: Entity<Self> = cx.entity();

        enum Layout {
            Classic { top: usize, rows: usize },
            Blocks { prompt: Option<(usize, usize)> },
        }
        let layout = {
            let term = self.session.term().lock();
            let live = self.session.live();
            let grid = term.grid();
            let history = grid.history_size();
            let screen = grid.screen_lines();
            if term.mode().contains(TermMode::ALT_SCREEN)
                || !live.integrated
                || live.phase == Phase::Unknown
            {
                Layout::Classic {
                    top: history - grid.display_offset(),
                    rows: screen,
                }
            } else {
                let cursor_abs = history + grid.cursor.point.line.0.max(0) as usize;
                let prompt = (live.phase == Phase::Prompt).then(|| {
                    let top = live.prompt_top.unwrap_or(cursor_abs).min(cursor_abs);
                    let last = Self::last_used_row(&term, top, cursor_abs);
                    (top, last - top + 1)
                });
                Layout::Blocks { prompt }
            }
        };

        let sizer_entity = entity.clone();
        let sizer = canvas(
            move |bounds, window, cx| {
                sizer_entity.update(cx, |view, _| view.resize_to(bounds, window));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let content = match layout {
            Layout::Classic { top, rows } => div()
                .size_full()
                .px(px(PADDING_X))
                .pt(px(PADDING_TOP))
                .pb(px(PADDING_BOTTOM))
                .child(
                    LiveElement::new(entity, self.focus.clone(), LiveRole::Classic, top, rows)
                        .fill(),
                )
                .into_any_element(),
            Layout::Blocks { prompt } => div()
                .size_full()
                .flex()
                .flex_col()
                .child(
                    // The list ignores its own padding; pad a wrapper instead.
                    div().flex_1().min_h_0().w_full().px(px(PADDING_X)).child(
                        list(self.list_state.clone(), cx.processor(Self::render_item)).size_full(),
                    ),
                )
                .when_some(prompt, |d, (top, rows)| {
                    d.child(
                        div()
                            .flex_none()
                            .w_full()
                            .bg(theme::hsla(palette().bar))
                            .px(px(PADDING_X))
                            .pt(px(10.))
                            .pb(px(10.))
                            .relative()
                            .child(LiveElement::new(
                                entity,
                                self.focus.clone(),
                                LiveRole::Prompt,
                                top,
                                rows,
                            ))
                            .when_some(self.context.clone(), |d, context| {
                                d.child(
                                    div()
                                        .absolute()
                                        .top(px(10.))
                                        .right(px(PADDING_X))
                                        .h(self.metrics.line_height)
                                        .flex()
                                        .items_center()
                                        .font_family(FONT_FAMILY)
                                        .text_size(px(11.))
                                        .text_color(theme::hsla(palette().muted))
                                        .child(context),
                                )
                            }),
                    )
                })
                .into_any_element(),
        };

        let p = palette();
        let tint = self.tint();
        let tint_label = tint.as_ref().map(|(tint, host)| {
            div()
                .absolute()
                .top(px(6.))
                .right(px(PADDING_X))
                .px(px(8.))
                .py(px(2.))
                .rounded(px(5.))
                .bg(theme::hsla(theme::blend(tint.background(&p), p.bar, 0.5)))
                .font_family(FONT_FAMILY)
                .text_size(px(11.))
                .text_color(theme::hsla(tint.label_color(&p)))
                .child(tint.label(host))
        });

        div()
            .id("terminal")
            .key_context("Terminal")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .bg(theme::hsla(
                tint.as_ref().map_or(p.bg, |(tint, _)| tint.background(&p)),
            ))
            .on_key_down(cx.listener(Self::key_down))
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::copy_last_output))
            .on_action(cx.listener(Self::copy_last_output_raw))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(|this, _: &ScrollPageUp, _, cx| this.scroll_page(true, cx)))
            .on_action(cx.listener(|this, _: &ScrollPageDown, _, cx| this.scroll_page(false, cx)))
            .on_action(cx.listener(|this, _: &ScrollToTop, _, cx| this.scroll_to_top(cx)))
            .on_action(cx.listener(|this, _: &ScrollToBottom, _, cx| this.scroll_to_bottom(cx)))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(Self::mouse_move))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::mouse_up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::mouse_up))
            .on_scroll_wheel(cx.listener(Self::scroll_wheel))
            .child(sizer)
            .child(content)
            .when_some(tint_label, |d, label| d.child(label))
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
        // The cursor cell, for the IME candidate window.
        let term = self.session.term().lock();
        let cursor = term.grid().cursor.point;
        let cursor_abs = term.grid().history_size() as i32 + cursor.line.0;
        drop(term);
        self.hit_regions.iter().find_map(|region| match region {
            HitRegion::Live {
                bounds, top, rows, ..
            } if cursor_abs >= *top as i32 && cursor_abs < (*top + *rows) as i32 => {
                let origin = bounds.origin
                    + Point::new(
                        self.metrics.cell_width * cursor.column.0 as f32,
                        self.metrics.line_height * (cursor_abs - *top as i32) as f32,
                    );
                Some(Bounds::new(
                    origin,
                    gpui::size(self.metrics.cell_width, self.metrics.line_height),
                ))
            }
            _ => None,
        })
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
