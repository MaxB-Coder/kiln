//! The window's root view: the sidebar of sessions, the panes of the active session side by
//! side, find, the command palette, and the title strip with the window controls.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use gpui::prelude::FluentBuilder as _;
use gpui::{
    Action, AnyElement, App, AppContext as _, Context, Decorations, Entity, Focusable,
    InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels, Render, ResizeEdge,
    SharedString, StatefulInteractiveElement, Styled, Subscription, Task, Window,
    WindowControlArea, canvas, div, px,
};
use kiln_term::{GridSize, SessionOptions, ShellCommand};

use crate::config::{self, Config};
use crate::copy;
use crate::flame;
use crate::grid_paint::{CellMetrics, FONT_FAMILY};
use crate::keymap::{self, KeyStyle};
use crate::logs;
use crate::naming;
use crate::palette::{CommandPalette, PaletteEvent, PaletteGroup, PaletteItem, PaletteTarget};
use crate::rail::Rail;
use crate::search::Match;
use crate::settings::{SettingsEvent, SettingsView};
use crate::shortcuts::{ShortcutsEvent, ShortcutsView};
use crate::state::{self, TabState, WindowState};
use crate::terminal_view::{PaneEvent, PaneStatus, TerminalView};
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{self, palette};
use crate::{
    ClosePane, CloseWindow, CommandPaletteToggle, CopyLastOutput, CopyLastOutputRaw, EditConfig,
    Find, FocusNextPane, FocusPrevPane, InstallUpdate, NewSession, NewWindow, NextSession,
    OpenConfigFile, PrevSession, ProjectColour, Quit, RenameProject, RenameSession, SelectSession1,
    SelectSession2, SelectSession3, SelectSession4, SelectSession5, SelectSession6, SelectSession7,
    SelectSession8, SelectSession9, SplitRight, ToggleFocusMode, ToggleRail, ToggleShortcuts,
    ToggleSidebar, ZoomIn, ZoomOut, ZoomReset,
};

/// Height of the strip at the top that holds the window controls.
pub const TITLEBAR_HEIGHT: Pixels = px(32.);
const SIDEBAR_WIDTH: Pixels = px(220.);
/// Room for the macOS traffic lights when the sidebar is hidden.
const TRAFFIC_LIGHTS_WIDTH: Pixels = px(78.);
const PANE_GAP: Pixels = px(10.);
/// Focus mode's reading width: this share of the window, at most this many columns.
const READING_SHARE: f32 = 0.72;
const READING_COLUMNS: f32 = 100.;
/// A pane header's text starts where the timeline's does, and its pills end where the blocks'
/// backgrounds do.
const HEADER_LEAD: f32 = crate::terminal_view::PADDING_X + crate::terminal_view::BLOCK_INSET;
const HEADER_TRAIL: f32 = crate::terminal_view::PADDING_X;
/// The `>_` mark and the room after it; minimize, maximize and close.
const MARK_WIDTH: f32 = 44.;
/// While the flame holds still, how often to look whether it may move again.
const FLAME_HELD_CHECK: Duration = Duration::from_millis(500);
const WINDOW_CONTROLS_WIDTH: f32 = 3. * 46.;

struct Pane {
    view: Entity<TerminalView>,
    /// The folder it started in, until the shell reports its own.
    start_cwd: Option<String>,
    /// Runs a one-off program (a diff, a log, the config editor): not saved with the layout.
    ephemeral: bool,
    _events: Subscription,
}

/// A session in the sidebar: one or more panes side by side.
struct Tab {
    id: u64,
    panes: Vec<Pane>,
    focused: usize,
    /// A bell, or a long command that finished out of sight; cleared when the session is shown.
    attention: bool,
    /// The name the user gave it; it then no longer follows the folder or the program.
    name: Option<String>,
}

/// What the sidebar's inline editor renames.
#[derive(Clone, Debug, PartialEq)]
pub enum RenameTarget {
    /// A session, by its id.
    Session(u64),
    /// A project, by its folder.
    Project(String),
}

struct Renaming {
    target: RenameTarget,
    input: Entity<TextInput>,
    _events: Subscription,
}

/// The right-click menu on a sidebar row, where it was opened. It takes the keys: ↑↓ move,
/// Enter picks, Esc closes.
struct SidebarMenu {
    target: RenameTarget,
    position: gpui::Point<Pixels>,
    selected: usize,
    focus: gpui::FocusHandle,
}

/// The project colour pop-up: Auto (0), then `state::PROJECT_COLOURS`.
struct ColourMenu {
    root: String,
    selected: usize,
    focus: gpui::FocusHandle,
}

/// The colour pop-up's columns, equal widths.
const COLOUR_COLUMNS: usize = 3;
const COLOUR_COLUMN_W: f32 = 58.;

/// Where a sidebar row is drawn, recorded as it paints (see `Workspace::sidebar_rows`).
#[derive(Clone, Copy)]
enum RowAt {
    Header(usize),
    Session(usize),
    Hint,
}

fn record_row(workspace: Entity<Workspace>, at: RowAt) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |bounds, _, _, cx| {
            let y = f32::from(bounds.center().y);
            workspace.update(cx, |w, _| {
                let rows = &mut w.sidebar_ys.0;
                let grow = |rows: &mut Vec<(f32, f32)>, i: usize| {
                    if rows.len() <= i {
                        rows.resize(i + 1, (0., 0.));
                    }
                };
                match at {
                    RowAt::Header(i) => {
                        grow(rows, i);
                        rows[i].0 = y;
                    }
                    RowAt::Session(i) => {
                        grow(rows, i);
                        rows[i].1 = y;
                    }
                    RowAt::Hint => w.sidebar_ys.1 = y,
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// "blue" → "Blue".
fn capitalised(name: &str) -> String {
    let mut chars = name.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

struct SearchBar {
    input: Entity<TextInput>,
    pane: Entity<TerminalView>,
    matches: Vec<Match>,
    current: Option<usize>,
    _events: Subscription,
}

pub struct Workspace {
    tabs: Vec<Tab>,
    active: usize,
    next_tab_id: u64,
    sidebar_open: bool,
    pub(crate) rail: Option<Rail>,
    /// ⌘.: only the focused pane, no sidebar or rail.
    focus_mode: bool,
    /// The focused pane's flame: its frame (0–3), and whether a task moves it on. Other panes'
    /// flames hold frame 1.
    flame_frame: usize,
    flame_ticking: bool,
    search: Option<SearchBar>,
    palette: Option<(Entity<CommandPalette>, Subscription)>,
    shortcuts: Option<(Entity<ShortcutsView>, Subscription)>,
    settings: Option<(Entity<SettingsView>, Subscription)>,
    renaming: Option<Renaming>,
    sidebar_menu: Option<SidebarMenu>,
    colour_menu: Option<ColourMenu>,
    /// Where the sidebar's rows were drawn (window y of their middles): each project's header
    /// and first session, and the shortcuts hint. For the snapshots' clicks.
    sidebar_ys: (Vec<(f32, f32)>, f32),
    /// The config this window last applied (its own writes, then the file watcher's reloads).
    applied: Config,
    metrics: CellMetrics,
    started: Instant,
    bench_startup: bool,
    shell_override: Option<ShellCommand>,
    /// Per working directory as reported: the folder with links resolved, and its project (git
    /// root, else the folder itself).
    projects: HashMap<String, (PathBuf, PathBuf)>,
    start_error: Option<String>,
    _config_watch: Task<()>,
    _state_watch: Task<()>,
}

impl Workspace {
    /// `restore`: reopen the sessions saved from the last window (else one new session).
    pub fn new(
        shell_override: Option<ShellCommand>,
        metrics: CellMetrics,
        bench_startup: bool,
        restore: bool,
        started: Instant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let config = config::current();
        let mut this = Self {
            tabs: Vec::new(),
            active: 0,
            next_tab_id: 1,
            sidebar_open: config.window.sidebar,
            rail: None,
            focus_mode: false,
            flame_frame: 0,
            flame_ticking: false,
            search: None,
            palette: None,
            shortcuts: None,
            renaming: None,
            sidebar_menu: None,
            colour_menu: None,
            sidebar_ys: (Vec::new(), 0.),
            settings: None,
            applied: config.clone(),
            metrics,
            started,
            bench_startup,
            shell_override,
            projects: HashMap::new(),
            start_error: None,
            _config_watch: Task::ready(()),
            _state_watch: Task::ready(()),
        };
        this._config_watch = this.watch_config(window, cx);
        crate::app_icon::apply(&config.app_icon(), window, true);
        // The "system" theme follows the OS appearance, as it changes.
        follow_appearance(window);
        cx.observe_window_appearance(window, |_, window, _| follow_appearance(window))
            .detach();
        cx.observe_window_activation(window, |this, window, cx| {
            if window.is_window_active()
                && let Some(tab) = this.tabs.get_mut(this.active)
                && std::mem::take(&mut tab.attention)
            {
                cx.notify();
            }
        })
        .detach();
        let saved = restore.then(|| state::current().window).flatten();
        if !saved.is_some_and(|saved| this.restore(saved, window, cx)) {
            this.new_tab(None, None, window, cx);
        }
        // A benchmark run leaves the saved sessions alone.
        if !bench_startup {
            this._state_watch = this.watch_state(window, cx);
            cx.on_app_quit(|this, cx| {
                this.save_state(cx);
                async {}
            })
            .detach();
        }
        this
    }

    // ----- panes and sessions ---------------------------------------------------------------

    fn session_options(&self, cwd: Option<PathBuf>, shell: Option<ShellCommand>) -> SessionOptions {
        let config = config::current();
        let mut options = SessionOptions::default();
        if let Some(shell) = shell.or_else(|| self.shell_override.clone()) {
            options.shell = shell;
        } else if let Some(program) = config.shell.program.clone() {
            options.shell =
                ShellCommand::new(program, config.shell.args.clone().unwrap_or_default());
        }
        if let Some(cwd) = cwd.filter(|c| c.is_dir()) {
            options.working_directory = Some(cwd);
        }
        options.scrollback_lines = config.scrollback;
        options.shell_integration = config.shell.integration.unwrap_or(true);
        if config.shell.prompt == config::PromptStyle::Kiln {
            options.prompt = Some(kiln_term::integration::MinimalPrompt);
        }
        options
    }

    fn spawn_pane(
        &mut self,
        cwd: Option<PathBuf>,
        shell: Option<ShellCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Pane> {
        let start_cwd = cwd.as_ref().map(|c| c.to_string_lossy().into_owned());
        let ephemeral = shell.is_some();
        let options = self.session_options(cwd, shell);
        // A first guess; the pane resizes the grid to its real size on its first frame.
        let grid = GridSize {
            columns: 100,
            lines: 30,
            cell_width: 16,
            cell_height: 34,
        };
        let bench = self.bench_startup && self.tabs.is_empty();
        match TerminalView::spawn_session(options, grid) {
            Ok((session, events)) => {
                let metrics = self.metrics;
                let started = if self.tabs.is_empty() {
                    self.started
                } else {
                    Instant::now()
                };
                let view = cx.new(|cx| {
                    TerminalView::new(session, events, metrics, bench, started, window, cx)
                });
                let events = cx.subscribe_in(&view, window, Self::on_pane_event);
                Some(Pane {
                    view,
                    start_cwd,
                    ephemeral,
                    _events: events,
                })
            }
            Err(err) => {
                log::error!("could not start the shell: {err:#}");
                self.start_error = Some(format!("{err:#}"));
                cx.notify();
                None
            }
        }
    }

    pub fn new_tab(
        &mut self,
        cwd: Option<PathBuf>,
        shell: Option<ShellCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(pane) = self.spawn_pane(cwd, shell, window, cx) else {
            return;
        };
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let insert_at = if self.tabs.is_empty() {
            0
        } else {
            self.active + 1
        };
        self.tabs.insert(
            insert_at,
            Tab {
                id,
                panes: vec![pane],
                focused: 0,
                attention: false,
                name: None,
            },
        );
        self.activate(insert_at, window, cx);
    }

    /// The frame a pane's flame shows.
    pub fn flame_frame_of(&self, view: &Entity<TerminalView>) -> usize {
        if self.focused_view().as_ref() == Some(view) {
            self.flame_frame
        } else {
            0
        }
    }

    /// Starts moving the flame on, unless it already is.
    fn ensure_flame(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.flame_ticking {
            return;
        }
        self.flame_ticking = true;
        cx.spawn_in(window, async move |this, cx| {
            let mut wait = flame::FRAME_EVERY;
            loop {
                cx.background_executor().timer(wait).await;
                match this.update_in(cx, |this, window, cx| this.flame_tick(window, cx)) {
                    Ok(Some(next)) => wait = next,
                    _ => break,
                }
            }
        })
        .detach();
    }

    /// One step of the focused pane's flame: the next frame while the window is active, the
    /// pane has the keyboard and the system allows motion; frame 1 otherwise. Only this view
    /// redraws (the panes are cached). Returns when to look again, or `None` once the pane
    /// has no flame (a command runs, a full-screen program is open).
    fn flame_tick(&mut self, window: &Window, cx: &mut Context<Self>) -> Option<Duration> {
        let Some(view) = self.focused_view().filter(|v| v.read(cx).flame().is_some()) else {
            self.flame_ticking = false;
            self.flame_frame = 0;
            return None;
        };
        let animate = flame::animation_forced()
            || window.is_window_active()
                && view.focus_handle(cx).is_focused(window)
                && !cx.reduce_motion()
                && !flame::reduced_motion();
        if animate {
            self.flame_frame = (self.flame_frame + 1) % flame::FRAMES;
            cx.notify();
            Some(flame::FRAME_EVERY)
        } else {
            if self.flame_frame != 0 {
                self.flame_frame = 0;
                cx.notify();
            }
            Some(FLAME_HELD_CHECK)
        }
    }

    pub fn focused_view(&self) -> Option<Entity<TerminalView>> {
        let tab = self.tabs.get(self.active)?;
        tab.panes.get(tab.focused).map(|p| p.view.clone())
    }

    fn focused_cwd(&self, cx: &App) -> Option<PathBuf> {
        self.focused_view()
            .and_then(|v| v.read(cx).cwd().map(PathBuf::from))
    }

    fn focus_active(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(view) = self.focused_view() {
            window.focus(&view.focus_handle(cx), cx);
        }
        self.refresh_rail(cx);
    }

    fn activate(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        self.active = index;
        self.tabs[index].attention = false;
        self.close_search(window, cx);
        self.focus_active(window, cx);
        self.update_title(window, cx);
        cx.notify();
    }

    fn update_title(&mut self, window: &mut Window, cx: &App) {
        let title = self.focused_breadcrumb(cx);
        if !title.is_empty() {
            window.set_window_title(&title);
        }
    }

    pub(crate) fn split_right(
        &mut self,
        cwd: Option<PathBuf>,
        shell: Option<ShellCommand>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let cwd = cwd.or_else(|| self.focused_cwd(cx));
        let Some(pane) = self.spawn_pane(cwd, shell, window, cx) else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        tab.focused += 1;
        tab.panes.insert(tab.focused, pane);
        self.focus_active(window, cx);
        cx.notify();
    }

    fn close_pane(
        &mut self,
        view: &Entity<TerminalView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((t, p)) = self.tabs.iter().enumerate().find_map(|(t, tab)| {
            tab.panes
                .iter()
                .position(|pane| &pane.view == view)
                .map(|p| (t, p))
        }) else {
            return;
        };
        if self.search.as_ref().is_some_and(|s| &s.pane == view) {
            self.search = None;
        }
        let tab = &mut self.tabs[t];
        tab.panes.remove(p);
        if tab.panes.is_empty() {
            self.tabs.remove(t);
            if self.tabs.is_empty() {
                // The last session closed: next launch starts fresh.
                if !self.bench_startup {
                    self.save_state(cx);
                }
                window.remove_window();
                return;
            }
            if self.active >= t && self.active > 0 {
                self.active -= 1;
            }
        } else {
            tab.focused = tab.focused.min(tab.panes.len() - 1);
        }
        self.focus_active(window, cx);
        self.update_title(window, cx);
        cx.notify();
    }

    fn focus_pane(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(self.active) else {
            return;
        };
        let len = tab.panes.len() as isize;
        tab.focused = (tab.focused as isize + delta).rem_euclid(len) as usize;
        self.focus_active(window, cx);
        self.update_title(window, cx);
        cx.notify();
    }

    fn on_pane_event(
        &mut self,
        view: &Entity<TerminalView>,
        event: &PaneEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            PaneEvent::Changed => {
                if Some(view) == self.focused_view().as_ref() {
                    self.update_title(window, cx);
                }
                cx.notify();
            }
            PaneEvent::Exited => self.close_pane(view, window, cx),
            PaneEvent::OpenLog(path) => self.split_right(
                path.parent().map(Path::to_path_buf),
                Some(logs::viewer_command(path)),
                window,
                cx,
            ),
            PaneEvent::Bell => {
                if !self.in_sight(view, window) {
                    self.set_attention(view, cx);
                }
            }
            PaneEvent::LongCommandFinished {
                command,
                duration,
                exit_code,
                cwd,
            } => {
                if !self.in_sight(view, window)
                    && let Some(tab_id) = self.set_attention(view, cx)
                {
                    let failed = exit_code.is_some_and(|code| code != 0 && code != 130);
                    let mut body = match exit_code {
                        Some(code) if failed => format!(
                            "Failed with exit {code} after {}",
                            copy::duration_label(*duration)
                        ),
                        _ => format!("Finished in {}", copy::duration_label(*duration)),
                    };
                    if let Some(cwd) = cwd {
                        body.push_str(&format!(" · {}", copy::short_folder(cwd)));
                    }
                    cx.show_system_notification(gpui::SystemNotification {
                        tag: notification_tag(window.window_handle().window_id().as_u64(), tab_id)
                            .into(),
                        title: notification_title(command).into(),
                        body: body.into(),
                        actions: Vec::new(),
                    });
                }
            }
        }
    }

    /// Whether the pane is on screen and the window is in front.
    fn in_sight(&self, view: &Entity<TerminalView>, window: &Window) -> bool {
        window.is_window_active()
            && self.tabs.get(self.active).is_some_and(|tab| {
                tab.panes
                    .iter()
                    .enumerate()
                    .any(|(i, pane)| &pane.view == view && (!self.focus_mode || i == tab.focused))
            })
    }

    /// Marks the pane's session for attention; returns the session's id.
    fn set_attention(
        &mut self,
        view: &Entity<TerminalView>,
        cx: &mut Context<Self>,
    ) -> Option<u64> {
        let tab = self
            .tabs
            .iter_mut()
            .find(|tab| tab.panes.iter().any(|pane| &pane.view == view))?;
        tab.attention = true;
        cx.notify();
        Some(tab.id)
    }

    /// Shows the session with this id (a clicked notification).
    pub fn activate_tab_id(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.tabs.iter().position(|tab| tab.id == id) {
            self.focus_mode = false;
            self.activate(index, window, cx);
        }
    }

    // ----- find -----------------------------------------------------------------------------

    fn open_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pane) = self.focused_view() else {
            return;
        };
        if let Some(search) = &self.search
            && search.pane == pane
        {
            window.focus(&search.input.focus_handle(cx), cx);
            return;
        }
        // Find moves to this pane.
        if let Some(old) = self.search.take() {
            old.pane.update(cx, |pane, cx| {
                pane.set_search("", cx);
                pane.set_find_open(false, cx);
            });
        }
        pane.update(cx, |pane, cx| pane.set_find_open(true, cx));
        let input = cx.new(|cx| TextInput::new("Find", cx));
        let events = cx.subscribe_in(
            &input,
            window,
            |this, input, event, window, cx| match event {
                TextInputEvent::Changed => {
                    let query = input.read(cx).text().to_string();
                    this.run_search(&query, cx);
                }
                // Enter goes back in time (older), Shift+Enter forward, like scrolling a terminal.
                TextInputEvent::Submit { shift } => {
                    this.step_search(if *shift { 1 } else { -1 }, cx)
                }
                TextInputEvent::Up => this.step_search(-1, cx),
                TextInputEvent::Down => this.step_search(1, cx),
                TextInputEvent::Cancel => this.close_search(window, cx),
            },
        );
        window.focus(&input.focus_handle(cx), cx);
        self.search = Some(SearchBar {
            input,
            pane,
            matches: Vec::new(),
            current: None,
            _events: events,
        });
        cx.notify();
    }

    fn run_search(&mut self, query: &str, cx: &mut Context<Self>) {
        let Some(search) = self.search.as_mut() else {
            return;
        };
        let matches = search
            .pane
            .update(cx, |pane, cx| pane.set_search(query, cx));
        search.current = matches.len().checked_sub(1);
        search.matches = matches;
        if let Some(m) = search.current.map(|i| search.matches[i]) {
            search.pane.update(cx, |pane, cx| pane.reveal_match(m, cx));
        }
        cx.notify();
    }

    fn step_search(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(search) = self.search.as_mut() else {
            return;
        };
        if search.matches.is_empty() {
            return;
        }
        let len = search.matches.len() as isize;
        let next = (search.current.unwrap_or(0) as isize + delta).rem_euclid(len) as usize;
        search.current = Some(next);
        let m = search.matches[next];
        search.pane.update(cx, |pane, cx| pane.reveal_match(m, cx));
        cx.notify();
    }

    fn close_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(search) = self.search.take() {
            search.pane.update(cx, |pane, cx| {
                pane.set_search("", cx);
                pane.set_find_open(false, cx);
            });
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    // ----- palette --------------------------------------------------------------------------

    /// Whether the shortcuts pop-up is open.
    pub fn shortcuts_open(&self) -> bool {
        self.shortcuts.is_some()
    }

    /// Whether the command palette is open.
    pub fn palette_open(&self) -> bool {
        self.palette.is_some()
    }

    /// The shortcuts pop-up: opens over everything, closes with the same keys, Esc or a click
    /// outside.
    fn toggle_shortcuts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shortcuts.take().is_some() {
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        self.palette = None;
        let style = key_style();
        let rows = keymap::live_rows(cx, style);
        let view = cx.new(|cx| ShortcutsView::new(rows, style, window, cx));
        let events = cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
            ShortcutsEvent::Dismiss => {
                this.shortcuts = None;
                this.focus_active(window, cx);
                cx.notify();
            }
        });
        self.shortcuts = Some((view, events));
        cx.notify();
    }

    /// Whether the Settings pop-up is open.
    pub fn settings_open(&self) -> bool {
        self.settings.is_some()
    }

    /// The Settings pop-up, while it is open.
    pub fn settings_view(&self) -> Option<Entity<SettingsView>> {
        self.settings.as_ref().map(|(view, _)| view.clone())
    }

    /// The Settings pop-up (⌘, or Ctrl+Shift+,): opens over everything; Esc, a click outside
    /// or the same keys close it.
    fn toggle_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.settings.take().is_some() {
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        self.palette = None;
        self.shortcuts = None;
        let shortcut = keymap::keys_for(&EditConfig, cx, key_style());
        let mac = chrome() == Chrome::Mac;
        let view = cx.new(|cx| SettingsView::new(mac, shortcut, window, cx));
        let events = cx.subscribe_in(&view, window, |this, _, event, window, cx| match event {
            SettingsEvent::Changed(config) => this.apply_config((**config).clone(), window, cx),
            SettingsEvent::OpenFile => {
                this.settings = None;
                this.edit_config(window, cx);
                cx.notify();
            }
            SettingsEvent::Dismiss => {
                this.settings = None;
                this.focus_active(window, cx);
                cx.notify();
            }
        });
        self.settings = Some((view, events));
        cx.notify();
    }

    fn toggle_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            self.focus_active(window, cx);
            cx.notify();
            return;
        }
        let items = self.palette_items(cx);
        let palette = cx.new(|cx| CommandPalette::new(items, window, cx));
        let events = cx.subscribe_in(&palette, window, |this, _, event, window, cx| match event {
            PaletteEvent::Dismiss => {
                this.palette = None;
                this.focus_active(window, cx);
                cx.notify();
            }
            PaletteEvent::Confirm(target) => {
                this.palette = None;
                this.focus_active(window, cx);
                match target {
                    PaletteTarget::Action(action) => {
                        window.dispatch_action(action.boxed_clone(), cx)
                    }
                    PaletteTarget::Session(index) => this.activate(*index, window, cx),
                    PaletteTarget::Project(root) => this.open_project(root.clone(), window, cx),
                    PaletteTarget::RunCommand(command) => {
                        if let Some(view) = this.focused_view() {
                            view.update(cx, |view, cx| view.run_command(command, cx));
                        }
                    }
                }
                cx.notify();
            }
        });
        self.palette = Some((palette, events));
        cx.notify();
    }

    fn palette_items(&mut self, cx: &App) -> Vec<PaletteItem> {
        let names = self.session_names(cx);
        let mut items = Vec::new();
        // Keys come from the keymap in effect, so they are always the ones that work.
        let style = key_style();
        let mut action = |label: &str, action: Box<dyn Action>| {
            let mut detail = keymap::keys_for(&*action, cx, style);
            // Windows and Linux: Ctrl+Shift+C copies the last output when nothing is selected.
            if detail.is_empty() && action.partial_eq(&CopyLastOutput) {
                detail = keymap::keys_for(&crate::Copy, cx, style);
            }
            items.push(PaletteItem {
                label: label.into(),
                detail,
                group: PaletteGroup::Actions,
                dot: None,
                target: PaletteTarget::Action(action),
            })
        };
        action("New session", Box::new(NewSession));
        action("Split right", Box::new(SplitRight));
        action("Find", Box::new(Find));
        action("Copy last output (fenced)", Box::new(CopyLastOutput));
        action("Copy last output (raw)", Box::new(CopyLastOutputRaw));
        action("Keyboard shortcuts", Box::new(ToggleShortcuts));
        action("Rename session", Box::new(RenameSession));
        action("Rename project", Box::new(RenameProject));
        action("Project colour", Box::new(ProjectColour));
        action("Toggle sidebar", Box::new(ToggleSidebar));
        action("Toggle context rail", Box::new(ToggleRail));
        action("Focus mode", Box::new(ToggleFocusMode));
        action("Close pane", Box::new(ClosePane));
        action("Next session", Box::new(NextSession));
        action("Previous session", Box::new(PrevSession));
        action("Bigger text", Box::new(ZoomIn));
        action("Smaller text", Box::new(ZoomOut));
        action("Reset text size", Box::new(ZoomReset));
        action("Settings", Box::new(EditConfig));
        action("Open config file", Box::new(OpenConfigFile));
        action("New window", Box::new(NewWindow));
        action("Quit Kiln", Box::new(Quit));
        if let Some(label) = crate::update::state(cx).label() {
            items.insert(
                0,
                PaletteItem {
                    label,
                    detail: String::new(),
                    group: PaletteGroup::Actions,
                    dot: None,
                    target: PaletteTarget::Action(Box::new(InstallUpdate)),
                },
            );
        }

        for (index, tab) in self.tabs.iter().enumerate() {
            let Some(pane) = tab.panes.get(tab.focused) else {
                continue;
            };
            let cwd = pane
                .view
                .read(cx)
                .cwd()
                .map(str::to_string)
                .or_else(|| pane.start_cwd.clone());
            // The sidebar's label; the full folder as the dim detail.
            let label = names.get(index).map(|(_, l)| l.clone()).unwrap_or_default();
            items.push(PaletteItem {
                label,
                detail: cwd.as_deref().map(copy::short_folder).unwrap_or_default(),
                group: PaletteGroup::Sessions,
                dot: None,
                target: PaletteTarget::Session(index),
            });
        }

        for project in state::current().projects {
            let root = PathBuf::from(&project.root);
            if !root.is_dir() {
                continue;
            }
            items.push(PaletteItem {
                label: root
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| project.root.clone()),
                detail: "open".into(),
                group: PaletteGroup::Projects,
                dot: Some(state::project_color(&project.root, &config::current())),
                target: PaletteTarget::Project(root),
            });
        }

        // Past commands from every session, newest first, each once, with the project they
        // ran in.
        let mut seen = std::collections::HashSet::new();
        let mut commands: Vec<(String, String)> = Vec::new();
        for tab in &self.tabs {
            for pane in &tab.panes {
                let view = pane.view.read(cx);
                let project = view.pills().project.unwrap_or_default();
                commands.extend(view.commands().into_iter().map(|c| (c, project.clone())));
            }
        }
        for (command, project) in commands.into_iter().rev() {
            if seen.insert(command.clone()) {
                items.push(PaletteItem {
                    label: command.clone(),
                    detail: project,
                    group: PaletteGroup::Commands,
                    dot: None,
                    target: PaletteTarget::RunCommand(command),
                });
            }
            if seen.len() >= 200 {
                break;
            }
        }
        items
    }

    // ----- config ---------------------------------------------------------------------------

    /// Polls the config file once a second and applies changes.
    fn watch_config(&self, window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        let mut last: Option<SystemTime> = config::config_path()
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let now = config::config_path()
                    .and_then(|p| std::fs::metadata(p).ok())
                    .and_then(|m| m.modified().ok());
                if now == last {
                    continue;
                }
                last = now;
                let (config, _) = config::load();
                if this
                    .update_in(cx, |this, window, cx| this.apply_config(config, window, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
    }

    /// Applies a config this window has not applied yet: from Settings at once, or from the
    /// file watcher when the file changed (which, after Settings wrote it, is the same config,
    /// so nothing happens twice).
    fn apply_config(&mut self, config: Config, window: &mut Window, cx: &mut Context<Self>) {
        if config == self.applied {
            return;
        }
        log::info!("config applied");
        let old = std::mem::replace(&mut self.applied, config.clone());
        config::set_current(config.clone());
        if config.font_size != old.font_size {
            self.set_font_size(px(config.font_size.clamp(6., 72.)), window, cx);
        }
        if config.window.sidebar != old.window.sidebar {
            self.sidebar_open = config.window.sidebar;
        }
        if config.app_icon() != old.app_icon() {
            crate::app_icon::apply(&config.app_icon(), window, false);
        }
        // Open sessions switch at their next prompt (the scripts read this file); one waiting
        // at an empty prompt draws a new one now, so the change shows at once.
        if config.shell.prompt != old.shell.prompt {
            match kiln_term::integration::set_prompt_mode(
                config.shell.prompt == config::PromptStyle::Kiln,
            ) {
                Ok(()) => {
                    for tab in &self.tabs {
                        for pane in &tab.panes {
                            pane.view.update(cx, |view, _| view.redraw_empty_prompt());
                        }
                    }
                }
                Err(err) => log::warn!("could not switch the prompt: {err}"),
            }
        }
        if let Some((settings, _)) = &self.settings {
            settings.update(cx, |settings, cx| {
                settings.reload();
                cx.notify();
            });
        }
        window.refresh();
    }

    fn set_font_size(&mut self, font_size: Pixels, window: &mut Window, cx: &mut Context<Self>) {
        self.metrics = CellMetrics::measure(font_size, window);
        for tab in &self.tabs {
            for pane in &tab.panes {
                pane.view
                    .update(cx, |view, cx| view.set_font_size(font_size, cx));
            }
        }
        cx.notify();
    }

    fn zoom(&mut self, delta: f32, window: &mut Window, cx: &mut Context<Self>) {
        let size = if delta == 0. {
            config::current().font_size
        } else {
            f32::from(self.metrics.font_size) + delta
        };
        self.set_font_size(px(size.clamp(6., 72.)), window, cx);
    }

    fn edit_config(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = config::config_path() else {
            return;
        };
        if !path.exists() {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Err(err) = std::fs::write(&path, config::TEMPLATE) {
                log::error!("could not create {}: {err}", path.display());
                return;
            }
        }
        let editor = std::env::var("VISUAL")
            .or_else(|_| std::env::var("EDITOR"))
            .ok()
            .filter(|e| !e.trim().is_empty());
        let file = path.to_string_lossy().into_owned();
        let shell = if cfg!(windows) {
            ShellCommand::new(editor.unwrap_or_else(|| "notepad.exe".into()), [file])
        } else {
            // Through a login shell, so $EDITOR and PATH come from the user's setup.
            let editor = editor.unwrap_or_else(|| "${VISUAL:-${EDITOR:-vi}}".into());
            ShellCommand::new(
                "/bin/sh",
                ["-lc".to_string(), format!("{editor} \"$0\"",), file],
            )
        };
        self.split_right(
            path.parent().map(Path::to_path_buf),
            Some(shell),
            window,
            cx,
        );
    }

    // ----- projects -------------------------------------------------------------------------

    /// The project a folder belongs to: its git root, else the folder itself.
    fn project_of(&mut self, cwd: &str) -> PathBuf {
        self.resolve(cwd).1
    }

    /// The folder with links resolved, and its project.
    fn resolve(&mut self, cwd: &str) -> (PathBuf, PathBuf) {
        if let Some(found) = self.projects.get(cwd) {
            return found.clone();
        }
        // Resolve links, so /var/… and /private/var/… are one project. (Not on Windows, where
        // it would turn C:\… into \\?\C:\….)
        let path = if cfg!(windows) {
            PathBuf::from(cwd)
        } else {
            std::fs::canonicalize(cwd).unwrap_or_else(|_| PathBuf::from(cwd))
        };
        let root = path
            .ancestors()
            .find(|dir| dir.join(".git").exists())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| path.clone());
        self.projects
            .insert(cwd.to_string(), (path.clone(), root.clone()));
        (path, root)
    }

    /// Each session's project and short label, in sidebar order (see `naming`).
    pub(crate) fn session_names(&mut self, cx: &App) -> Vec<(Option<String>, String)> {
        let mut facts = Vec::with_capacity(self.tabs.len());
        for index in 0..self.tabs.len() {
            let focused = self.tabs[index].focused;
            facts.push(self.pane_facts(index, focused, cx));
        }
        let labels = naming::session_labels(&facts);
        facts
            .into_iter()
            .map(|f| f.project)
            .zip(labels)
            .zip(&self.tabs)
            // A session given a name keeps it.
            .map(|((project, label), tab)| (project, tab.name.clone().unwrap_or(label)))
            .collect()
    }

    /// What a pane's name is made of: its project, folder, program and shell.
    fn pane_facts(&mut self, tab: usize, pane: usize, cx: &App) -> naming::SessionFacts {
        let (cwd, program, shell) = match self.tabs.get(tab).and_then(|t| t.panes.get(pane)) {
            Some(pane) => {
                let view = pane.view.read(cx);
                (
                    view.cwd()
                        .map(str::to_string)
                        .or_else(|| pane.start_cwd.clone()),
                    view.running_program(),
                    view.shell_label(),
                )
            }
            None => (None, None, String::new()),
        };
        let (cwd, project) = match cwd {
            Some(cwd) => {
                let (path, root) = self.resolve(&cwd);
                (
                    Some(path.to_string_lossy().into_owned()),
                    Some(root.to_string_lossy().into_owned()),
                )
            }
            None => (None, None),
        };
        naming::SessionFacts {
            project,
            cwd,
            program,
            shell,
        }
    }

    /// The name in each pane's header for the active session: the sidebar's label for the
    /// focused pane, each other pane's own.
    /// "acme-web › api" for the focused session.
    fn focused_breadcrumb(&mut self, cx: &App) -> String {
        let names = self.session_names(cx);
        names
            .get(self.active)
            .map(|(project, label)| naming::breadcrumb(project.as_deref(), label))
            .unwrap_or_default()
    }

    // ----- renaming sessions and projects, project colours ----------------------------------

    /// The sessions' names as the sidebar shows them (the snapshots check them).
    pub fn session_titles(&mut self, cx: &App) -> Vec<String> {
        self.session_names(cx)
            .into_iter()
            .map(|(_, name)| name)
            .collect()
    }

    /// Per project in the sidebar: (its header's y, its first session's y, the hint's y).
    pub fn sidebar_rows(&self) -> Vec<(f32, f32, f32)> {
        let (rows, hint) = &self.sidebar_ys;
        rows.iter()
            .map(|(header, session)| (*header, *session, *hint))
            .collect()
    }

    /// Whether an inline name editor or the colour pop-up is open (the snapshots check them).
    pub fn editing(&self) -> (bool, bool) {
        (self.renaming.is_some(), self.colour_menu.is_some())
    }

    /// The sidebar has to show for its inline editors; the keys and the palette open it.
    fn show_sidebar(&mut self) {
        self.sidebar_open = true;
        self.focus_mode = false;
    }

    /// The active session's project folder, if it has one.
    fn active_project(&mut self, cx: &App) -> Option<String> {
        self.session_names(cx)
            .get(self.active)
            .and_then(|(project, _)| project.clone())
            .filter(|root| !root.is_empty())
    }

    /// Starts the inline name editor on a session or a project, holding its current name.
    pub fn start_rename(
        &mut self,
        target: RenameTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_menu = None;
        self.colour_menu = None;
        self.show_sidebar();
        let current = match &target {
            RenameTarget::Session(id) => {
                let names = self.session_names(cx);
                self.tabs
                    .iter()
                    .position(|t| t.id == *id)
                    .and_then(|i| names.get(i).map(|(_, label)| label.clone()))
                    .unwrap_or_default()
            }
            RenameTarget::Project(root) => naming::project_name(Some(root)),
        };
        let input = cx.new(|cx| {
            let mut input = TextInput::new(
                match target {
                    RenameTarget::Session(_) => "Session name",
                    RenameTarget::Project(_) => "Project name",
                },
                cx,
            )
            .with_size(px(12.));
            input.set_text(&current, cx);
            input
        });
        let events = cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
            TextInputEvent::Submit { .. } => this.finish_rename(true, window, cx),
            TextInputEvent::Cancel => this.finish_rename(false, window, cx),
            _ => {}
        });
        window.focus(&input.focus_handle(cx), cx);
        self.renaming = Some(Renaming {
            target,
            input,
            _events: events,
        });
        cx.notify();
    }

    /// Enter keeps the name (an empty one goes back to the automatic name); Esc drops it.
    fn finish_rename(&mut self, keep: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(renaming) = self.renaming.take() else {
            return;
        };
        if keep {
            let text = renaming.input.read(cx).text().trim().to_string();
            let name = (!text.is_empty()).then_some(text);
            match renaming.target {
                RenameTarget::Session(id) => {
                    if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
                        tab.name = name;
                    }
                }
                RenameTarget::Project(root) => {
                    self.write_project(&root, "name", name, window, cx);
                }
            }
        }
        self.focus_active(window, cx);
        cx.notify();
    }

    /// Writes one key of a project's `[projects."…"]` table (`None` removes it), and applies
    /// the result.
    fn write_project(
        &mut self,
        root: &str,
        key: &'static str,
        value: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(path) = config::config_path() else {
            return;
        };
        let project = state::project_key(root, &config::current());
        let edit: crate::config_write::Edit = (
            &["projects", project.as_str(), key],
            value.map(toml_edit::Value::from),
        );
        match crate::config_write::write_edits(&path, &[edit]) {
            Ok(config) => self.apply_config(config, window, cx),
            Err(err) => log::warn!("could not save the project's {key}: {err:?}"),
        }
    }

    /// Opens the colour pop-up under a project in the sidebar, on its current colour.
    pub fn open_colour_menu(&mut self, root: String, window: &mut Window, cx: &mut Context<Self>) {
        self.sidebar_menu = None;
        self.renaming = None;
        self.show_sidebar();
        let chosen = state::project_config(&root, &config::current())
            .and_then(|(_, project)| project.color.clone());
        let selected = chosen
            .and_then(|c| state::PROJECT_COLOURS.iter().position(|n| *n == c.trim()))
            .map_or(0, |i| i + 1);
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.colour_menu = Some(ColourMenu {
            root,
            selected,
            focus,
        });
        cx.notify();
    }

    /// Picks entry `index` of the colour pop-up (0 is Auto) and closes it.
    fn pick_colour(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.colour_menu.take() else {
            return;
        };
        let value = index
            .checked_sub(1)
            .and_then(|i| state::PROJECT_COLOURS.get(i))
            .map(|name| name.to_string());
        self.write_project(&menu.root, "color", value, window, cx);
        self.focus_active(window, cx);
        cx.notify();
    }

    /// The colour pop-up's keys: the arrows move (three to a row), Enter picks, Esc closes.
    fn colour_menu_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.colour_menu.as_mut() else {
            return;
        };
        let count = state::PROJECT_COLOURS.len() + 1;
        let at = menu.selected as i32;
        let next = match event.keystroke.key.as_str() {
            "left" => at - 1,
            "right" | "tab" => at + 1,
            "up" => at - COLOUR_COLUMNS as i32,
            "down" => at + COLOUR_COLUMNS as i32,
            "enter" | "space" => {
                let selected = menu.selected;
                self.pick_colour(selected, window, cx);
                cx.stop_propagation();
                return;
            }
            "escape" => {
                self.colour_menu = None;
                self.focus_active(window, cx);
                cx.notify();
                cx.stop_propagation();
                return;
            }
            _ => return,
        };
        menu.selected = next.rem_euclid(count as i32) as usize;
        cx.stop_propagation();
        cx.notify();
    }

    /// The pop-up under a project: Auto and the theme's colours, three to a row in equal
    /// columns, each a round swatch with its name below, the current one ringed.
    fn render_colour_menu(&self, menu: &ColourMenu, cx: &mut Context<Self>) -> AnyElement {
        let p = palette();
        let auto = state::project_color(&menu.root, &Config::default());
        let mut grid = div().flex().flex_row().flex_wrap();
        for i in 0..=state::PROJECT_COLOURS.len() {
            let (name, color) = match i {
                0 => ("Auto", auto),
                i => {
                    let name = state::PROJECT_COLOURS[i - 1];
                    (name, state::colour_value(name, &p).unwrap_or(auto))
                }
            };
            let on = i == menu.selected;
            let swatch = div().size(px(18.)).rounded_full().bg(theme::hsla(color));
            // Auto: the colour the project would get, with a hole, as "picked for you".
            let swatch = if i == 0 {
                swatch
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(div().size(px(8.)).rounded_full().bg(theme::hsla(p.raised)))
            } else {
                swatch
            };
            grid = grid.child(
                div()
                    .id(("project-colour", i))
                    .w(px(COLOUR_COLUMN_W))
                    .py(px(4.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(4.))
                    .cursor_pointer()
                    .text_size(px(9.5))
                    .text_color(theme::hsla(if on { p.fg } else { p.fg_dim }))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.pick_colour(i, window, cx)),
                    )
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
                            .child(swatch),
                    )
                    .child(SharedString::from(capitalised(name))),
            );
        }
        div()
            .id("project-colours")
            .track_focus(&menu.focus)
            .on_key_down(cx.listener(Self::colour_menu_key))
            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                this.colour_menu = None;
                cx.notify();
            }))
            .mx(px(4.))
            .mt(px(2.))
            .mb(px(6.))
            .px(px(8.))
            .py(px(8.))
            .rounded(px(10.))
            .bg(theme::hsla(p.raised))
            .border_1()
            .border_color(theme::hsla(p.bg1))
            .child(
                div()
                    .px(px(4.))
                    .mb(px(4.))
                    .text_size(px(10.))
                    .text_color(theme::hsla(p.muted))
                    .child("Colour"),
            )
            .child(grid)
            .into_any_element()
    }

    /// The inline editor, in place of a name in the sidebar.
    fn render_rename_input(&self, renaming: &Renaming) -> AnyElement {
        let p = palette();
        div()
            .flex_1()
            .min_w_0()
            .px(px(6.))
            .py(px(2.))
            .rounded(px(5.))
            .bg(theme::hsla(p.bg))
            .border_1()
            .border_color(theme::hsla(p.accent))
            .text_color(theme::hsla(p.fg))
            .child(renaming.input.clone())
            .into_any_element()
    }

    fn open_sidebar_menu(
        &mut self,
        target: RenameTarget,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        self.sidebar_menu = Some(SidebarMenu {
            target,
            position,
            selected: 0,
            focus,
        });
        cx.notify();
    }

    /// Item `index` of the right-click menu: 0 Rename, 1 Colour (projects only).
    fn pick_menu_item(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.sidebar_menu.take() else {
            return;
        };
        match (index, menu.target) {
            (0, target) => self.start_rename(target, window, cx),
            (_, RenameTarget::Project(root)) => self.open_colour_menu(root, window, cx),
            (_, RenameTarget::Session(_)) => self.focus_active(window, cx),
        }
        cx.notify();
    }

    fn sidebar_menu_key(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.sidebar_menu.as_mut() else {
            return;
        };
        let count = match menu.target {
            RenameTarget::Project(_) => 2,
            RenameTarget::Session(_) => 1,
        };
        match event.keystroke.key.as_str() {
            "up" => menu.selected = (menu.selected + count - 1) % count,
            "down" | "tab" => menu.selected = (menu.selected + 1) % count,
            "enter" | "space" => {
                let selected = menu.selected;
                self.pick_menu_item(selected, window, cx);
            }
            "escape" => {
                self.sidebar_menu = None;
                self.focus_active(window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// The right-click menu on a sidebar row: Rename, and Colour for a project.
    fn render_sidebar_menu(&self, menu: &SidebarMenu, cx: &mut Context<Self>) -> AnyElement {
        let p = palette();
        let selected = menu.selected;
        let item = |i: usize, id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px(px(10.))
                .py(px(5.))
                .rounded(px(5.))
                .text_color(theme::hsla(p.fg))
                .when(i == selected, |d| d.bg(theme::hsla(p.bg1)))
                .hover(|s| s.bg(theme::hsla(p.bg1)))
                .cursor_pointer()
                .child(label)
        };
        let rename = item(0, "menu-rename", "Rename")
            .on_click(cx.listener(|this, _, window, cx| this.pick_menu_item(0, window, cx)));
        let colour = matches!(menu.target, RenameTarget::Project(_)).then(|| {
            item(1, "menu-colour", "Colour…")
                .on_click(cx.listener(|this, _, window, cx| this.pick_menu_item(1, window, cx)))
        });
        gpui::deferred(
            gpui::anchored().position(menu.position).child(
                div()
                    .id("sidebar-menu")
                    .track_focus(&menu.focus)
                    .on_key_down(cx.listener(Self::sidebar_menu_key))
                    .min_w(px(140.))
                    .p(px(4.))
                    .rounded(px(8.))
                    .bg(theme::hsla(p.raised))
                    .border_1()
                    .border_color(theme::hsla(p.bg1))
                    .shadow(vec![gpui::BoxShadow {
                        color: p.shadow(),
                        offset: gpui::point(px(0.), px(8.)),
                        blur_radius: px(24.),
                        spread_radius: px(0.),
                        inset: false,
                    }])
                    .font_family(FONT_FAMILY)
                    .text_size(px(12.))
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.sidebar_menu = None;
                        cx.notify();
                    }))
                    .child(rename)
                    .children(colour),
            ),
        )
        .with_priority(1)
        .into_any_element()
    }

    // ----- saved sessions and project spaces ------------------------------------------------

    /// Opens a saved window's sessions. Returns false when none could start.
    fn restore(&mut self, saved: WindowState, window: &mut Window, cx: &mut Context<Self>) -> bool {
        for tab in &saved.tabs {
            self.open_tab_layout(tab, window, cx);
        }
        if self.tabs.is_empty() {
            return false;
        }
        self.sidebar_open = saved.sidebar;
        if saved.rail && self.rail.is_none() {
            self.toggle_rail(window, cx);
        }
        self.activate(saved.active.min(self.tabs.len() - 1), window, cx);
        true
    }

    /// A new session with the saved panes, side by side.
    fn open_tab_layout(&mut self, tab: &TabState, window: &mut Window, cx: &mut Context<Self>) {
        let mut cwds = tab.panes.iter().map(|cwd| cwd.as_ref().map(PathBuf::from));
        let before = self.tabs.len();
        self.new_tab(cwds.next().flatten(), None, window, cx);
        if self.tabs.len() == before {
            return;
        }
        for cwd in cwds {
            self.split_right(cwd, None, window, cx);
        }
        if let Some(current) = self.tabs.get_mut(self.active) {
            current.focused = tab.focused.min(current.panes.len() - 1);
            current.name = tab.name.clone();
        }
        self.focus_active(window, cx);
    }

    /// Shows a project: its open session, else its saved layout, else a new session there.
    fn open_project(&mut self, root: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let key = root.to_string_lossy().into_owned();
        state::touch_project(&key);
        for index in 0..self.tabs.len() {
            if self.tab_cwd(index, cx).map(|cwd| self.project_of(&cwd)) == Some(root.clone()) {
                self.activate(index, window, cx);
                return;
            }
        }
        let saved = state::current()
            .projects
            .into_iter()
            .find(|p| p.root == key)
            .map(|p| p.tabs)
            .unwrap_or_default();
        if saved.is_empty() {
            self.new_tab(Some(root), None, window, cx);
        } else {
            for tab in &saved {
                self.open_tab_layout(tab, window, cx);
            }
        }
    }

    fn tab_cwd(&self, index: usize, cx: &App) -> Option<String> {
        let tab = self.tabs.get(index)?;
        let pane = tab.panes.get(tab.focused)?;
        pane.view
            .read(cx)
            .cwd()
            .map(str::to_string)
            .or_else(|| pane.start_cwd.clone())
    }

    /// This window's sessions, as saved.
    fn window_state(&self, cx: &App) -> WindowState {
        let mut active = 0;
        let mut tabs = Vec::new();
        for (index, tab) in self.tabs.iter().enumerate() {
            let panes: Vec<_> = tab.panes.iter().filter(|pane| !pane.ephemeral).collect();
            if panes.is_empty() {
                continue;
            }
            if index == self.active {
                active = tabs.len();
            }
            let focused = tab
                .panes
                .get(tab.focused)
                .and_then(|f| panes.iter().position(|p| p.view == f.view))
                .unwrap_or(0);
            tabs.push(TabState {
                panes: panes
                    .iter()
                    .map(|pane| {
                        pane.view
                            .read(cx)
                            .cwd()
                            .map(str::to_string)
                            .or_else(|| pane.start_cwd.clone())
                    })
                    .collect(),
                focused,
                name: tab.name.clone(),
            });
        }
        WindowState {
            sidebar: self.sidebar_open,
            rail: self.rail.is_some(),
            active,
            tabs,
        }
    }

    fn save_state(&mut self, cx: &mut Context<Self>) {
        let window = self.window_state(cx);
        // Only folders in git are projects.
        let mut roots: HashMap<String, Option<String>> = HashMap::new();
        for cwd in window.tabs.iter().flat_map(|t| t.panes.iter().flatten()) {
            let root = self.project_of(cwd);
            let is_project = root.join(".git").exists();
            roots.insert(
                cwd.clone(),
                is_project.then(|| root.to_string_lossy().into_owned()),
            );
        }
        state::save_window(window, |cwd| roots.get(cwd).cloned().flatten());
    }

    /// Saves the sessions every two seconds when they changed.
    /// Saves the sessions every two seconds when they changed, and re-reads the focused pane's
    /// branch and changes every six (files change outside the shell too).
    fn watch_state(&self, window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            let mut tick = 0u32;
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                tick = tick.wrapping_add(1);
                let updated = this.update(cx, |this, cx| {
                    this.save_state(cx);
                    if tick.is_multiple_of(3)
                        && let Some(view) = this.focused_view()
                    {
                        view.update(cx, |view, cx| view.refresh_git(cx));
                    }
                });
                if updated.is_err() {
                    break;
                }
            }
        })
    }

    // ----- rendering ------------------------------------------------------------------------

    fn render_sidebar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette();
        let fullscreen = window.is_fullscreen();
        // Group sessions by project, in order of first appearance.
        let names = self.session_names(cx);
        let mut groups: Vec<(Option<String>, Vec<usize>)> = Vec::new();
        for (index, (project, _)) in names.iter().enumerate() {
            match groups.iter_mut().find(|(root, _)| root == project) {
                Some((_, tabs)) => tabs.push(index),
                None => groups.push((project.clone(), vec![index])),
            }
        }

        let mut list = div().flex().flex_col().gap(px(2.));
        for (group_index, (root, tabs)) in groups.into_iter().enumerate() {
            // The repository or folder name (or the one given it); "~" for home; "other" when
            // no folder is known yet (a shell without integration, or one still starting).
            let name = naming::project_name(root.as_deref());
            // Projects can be renamed and given a colour; "other" cannot.
            let target = root
                .clone()
                .filter(|r| !r.is_empty())
                .map(RenameTarget::Project);
            let editing = self
                .renaming
                .as_ref()
                .filter(|r| target.as_ref() == Some(&r.target))
                .map(|r| self.render_rename_input(r));
            let colour_menu = self
                .colour_menu
                .as_ref()
                .filter(|m| root.as_deref() == Some(m.root.as_str()))
                .map(|m| self.render_colour_menu(m, cx));
            let root = root.map(PathBuf::from).unwrap_or_default();
            // Projects (folders in git) get their colour; Home and loose folders do not.
            let color = (!root.as_os_str().is_empty()
                && !naming::is_home(&root.to_string_lossy())
                && root.join(".git").exists())
            .then(|| state::project_color(&root.to_string_lossy(), &config::current()));
            let header = div()
                .id(("project", group_index))
                .when_some(target, |d, target| {
                    let on_right = target.clone();
                    d.on_click(
                        cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                            if event.click_count() == 2 {
                                this.start_rename(target.clone(), window, cx);
                            }
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                            this.open_sidebar_menu(on_right.clone(), event.position, window, cx)
                        }),
                    )
                });
            let entity = cx.entity();
            list = list.child(
                header
                    .relative()
                    .child(record_row(entity.clone(), RowAt::Header(group_index)))
                    .pt(px(12.))
                    .pb(px(4.))
                    .px(px(10.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .text_size(px(11.))
                    .text_color(theme::hsla(p.muted))
                    .when_some(color, |d, color| {
                        d.child(
                            div()
                                .flex_none()
                                .size(px(7.))
                                .rounded(px(2.))
                                .bg(theme::hsla(color)),
                        )
                    })
                    .map(|d| match editing {
                        Some(input) => d.child(input),
                        None => d.child(SharedString::from(name)),
                    }),
            );
            list = list.children(colour_menu);
            for (n, index) in tabs.into_iter().enumerate() {
                let label = names[index].1.clone();
                let row = self.render_session_row(index, label, cx);
                list = list.child(if n == 0 {
                    div()
                        .relative()
                        .child(row)
                        .child(record_row(entity.clone(), RowAt::Session(group_index)))
                        .into_any_element()
                } else {
                    row
                });
            }
        }

        let menu = self
            .sidebar_menu
            .as_ref()
            .map(|menu| self.render_sidebar_menu(menu, cx));
        div()
            .id("sidebar")
            .group("sidebar")
            .children(menu)
            .flex_none()
            .w(SIDEBAR_WIDTH)
            .h_full()
            .flex()
            .flex_col()
            .bg(theme::hsla(p.bar))
            .font_family(FONT_FAMILY)
            .text_size(px(12.))
            // The traffic lights sit over the sidebar on macOS.
            .child(
                div()
                    .h(TITLEBAR_HEIGHT)
                    .flex_none()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .px(px(10.))
                    .when(chrome() != Chrome::Mac || fullscreen, |d| {
                        d.justify_between().child(app_mark())
                    })
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(MouseButton::Left, |event, window, _| {
                        if event.click_count == 2 {
                            window.titlebar_double_click();
                        } else if !cfg!(windows) {
                            window.start_window_move();
                        }
                    })
                    .child(
                        div()
                            .id("new-session")
                            .px(px(6.))
                            .text_size(px(14.))
                            .text_color(theme::hsla(p.muted))
                            .invisible()
                            .group_hover("sidebar", |s| s.visible())
                            .hover(|s| s.text_color(theme::hsla(p.fg)))
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(|this, _, window, cx| {
                                let cwd = this.focused_cwd(cx);
                                this.new_tab(cwd, None, window, cx)
                            }))
                            .child("+"),
                    ),
            )
            .child(
                div()
                    .id("sessions")
                    .flex_1()
                    .overflow_y_scroll()
                    .px(px(6.))
                    .child(list),
            )
            .when_some(crate::update::state(cx).label(), |d, label| {
                // A new version: one quiet line at the bottom, until it is installed.
                d.child(
                    div()
                        .id("update")
                        .flex_none()
                        .m(px(6.))
                        .h(px(28.))
                        .px(px(10.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(6.))
                        .text_color(theme::hsla(p.fg_dim))
                        .hover(|s| s.bg(theme::hsla(p.raised)))
                        .cursor_pointer()
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(InstallUpdate), cx)
                        })
                        .child(
                            div()
                                .flex_none()
                                .size(px(6.))
                                .rounded_full()
                                .bg(theme::hsla(p.accent)),
                        )
                        .child(SharedString::from(label)),
                )
            })
            .child(
                // The way to every key, for anyone who has not learnt them yet: the shortcuts
                // binding as the keymap has it ("⌘/", "Ctrl+Shift+/").
                div().flex_none().px(px(14.)).pt(px(4.)).pb(px(10.)).child(
                    div()
                        .id("shortcuts-hint")
                        .relative()
                        .child(record_row(cx.entity(), RowAt::Hint))
                        .flex()
                        .flex_row()
                        .gap(px(6.))
                        .text_size(px(10.5))
                        .text_color(theme::hsla(p.bg3))
                        .hover(|s| s.text_color(theme::hsla(p.fg_dim)))
                        .cursor_pointer()
                        .on_click(|_, window, cx| {
                            window.dispatch_action(Box::new(ToggleShortcuts), cx)
                        })
                        .child(SharedString::from(keymap::keys_for(
                            &ToggleShortcuts,
                            cx,
                            key_style(),
                        )))
                        .child("Shortcuts"),
                ),
            )
            .into_any_element()
    }

    fn render_session_row(
        &self,
        index: usize,
        title: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let p = palette();
        let tab = &self.tabs[index];
        let active = index == self.active;
        // A session's status is the busiest of its panes.
        let status = tab
            .panes
            .iter()
            .map(|pane| pane.view.read(cx).status())
            .max_by_key(|s| match s {
                PaneStatus::Running => 2,
                PaneStatus::Failed => 1,
                PaneStatus::Idle => 0,
            })
            .unwrap_or(PaneStatus::Idle);
        let dot = match status {
            PaneStatus::Failed => Some(p.failed),
            // Something to look at: the bell rang, or a long command finished out of sight.
            _ if tab.attention => Some(p.accent),
            PaneStatus::Running => Some(p.ok),
            PaneStatus::Idle => None,
        };
        let editing = self
            .renaming
            .as_ref()
            .filter(|r| r.target == RenameTarget::Session(tab.id))
            .map(|r| self.render_rename_input(r));
        let tab_id = tab.id;
        let group: SharedString = format!("session-{}", tab.id).into();
        let first_view = tab.panes.first().map(|pane| pane.view.clone());
        div()
            .id(("session", tab.id as usize))
            .group(group.clone())
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .h(px(28.))
            .px(px(10.))
            .rounded(px(6.))
            .when(active, |d| d.bg(theme::hsla(p.raised)))
            .hover(|s| s.bg(theme::hsla(p.raised)))
            .cursor_pointer()
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    this.activate(index, window, cx);
                    if event.click_count() == 2 {
                        this.start_rename(RenameTarget::Session(tab_id), window, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                    let target = RenameTarget::Session(tab_id);
                    this.open_sidebar_menu(target, event.position, window, cx)
                }),
            )
            .child(
                div()
                    .flex_none()
                    .size(px(6.))
                    .rounded_full()
                    .when_some(dot, |d, color| d.bg(theme::hsla(color))),
            )
            .map(|d| match editing {
                Some(input) => d.child(input),
                None => d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .text_color(theme::hsla(if active { p.fg } else { p.fg_dim }))
                        .child(SharedString::from(title)),
                ),
            })
            .when(tab.panes.len() > 1, |d| {
                d.child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme::hsla(p.muted))
                        .child(format!("{}", tab.panes.len())),
                )
            })
            .child(
                div()
                    .id(("close-session", tab.id as usize))
                    .px(px(4.))
                    .text_color(theme::hsla(p.muted))
                    .invisible()
                    .group_hover(group, |s| s.visible())
                    .hover(|s| s.text_color(theme::hsla(p.fg)))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // Closing a session closes all its panes (their shells get SIGHUP).
                        if let Some(view) = first_view.clone() {
                            let tab_index = this
                                .tabs
                                .iter()
                                .position(|t| t.panes.iter().any(|p| p.view == view));
                            if let Some(t) = tab_index {
                                let views: Vec<_> =
                                    this.tabs[t].panes.iter().map(|p| p.view.clone()).collect();
                                for view in views {
                                    this.close_pane(&view, window, cx);
                                }
                            }
                        }
                    }))
                    .child("×"),
            )
            .into_any_element()
    }

    /// A pane's header, one line: on the right how many things run (in focus mode) and the
    /// pills, the project's first; the rest is room to drag the window by. `lead` and `trail`
    /// keep it clear of the traffic lights, the `>_` mark and the window controls.
    fn render_pane_header(
        &self,
        view: &Entity<TerminalView>,
        lead: f32,
        trail: f32,
        cx: &App,
    ) -> AnyElement {
        let p = palette();
        let pills = view.read(cx).pills();
        let running = Some(self.running_count(cx)).filter(|n| self.focus_mode && *n > 0);
        div()
            .flex_none()
            .h(TITLEBAR_HEIGHT)
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(10.))
            .pl(px(lead))
            .pr(px(trail))
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, start_window_drag)
            .font_family(FONT_FAMILY)
            .text_size(px(11.))
            .child(div().flex_1().min_w_0())
            .when_some(running, |d, running| {
                d.child(
                    div()
                        .flex_none()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(10.5))
                        .text_color(theme::hsla(p.bg3))
                        .child(div().size(px(5.)).rounded_full().bg(theme::hsla(p.ok)))
                        .child(format!("{running} running")),
                )
            })
            .child(crate::pills::render(&pills))
            .into_any_element()
    }

    /// Room the window's own controls take at the top left and right of the panes: the traffic
    /// lights (macOS) or the `>_` mark when the sidebar is closed, and the window controls
    /// (Windows and Linux) when the rail is.
    fn chrome_reserve(&self, window: &Window) -> (f32, f32) {
        let mac = chrome() == Chrome::Mac;
        let left = match (self.sidebar_visible(), mac) {
            (true, _) => 0.,
            (false, true) if window.is_fullscreen() => 0.,
            (false, true) => f32::from(TRAFFIC_LIGHTS_WIDTH),
            (false, false) => MARK_WIDTH,
        };
        let right = if !mac && !self.rail_visible() {
            WINDOW_CONTROLS_WIDTH + 8.
        } else {
            0.
        };
        (left, right)
    }

    /// Panes with a command running, in every session.
    fn running_count(&self, cx: &App) -> usize {
        self.tabs
            .iter()
            .flat_map(|tab| tab.panes.iter())
            .filter(|pane| pane.view.read(cx).status() == PaneStatus::Running)
            .count()
    }

    fn sidebar_visible(&self) -> bool {
        self.sidebar_open && !self.focus_mode
    }

    pub(crate) fn rail_visible(&self) -> bool {
        self.rail.is_some() && !self.focus_mode
    }

    /// Find: a bar across the top of the pane's timeline, "⌕ query" on the left and the count
    /// with the keys on the right.
    fn render_search_bar(&self) -> Option<AnyElement> {
        let search = self.search.as_ref()?;
        let p = palette();
        let status = find_status(search.current, search.matches.len());
        // Its edges line up with the blocks' backgrounds.
        let inset = crate::terminal_view::PADDING_X;
        Some(
            div()
                .absolute()
                .top(px(crate::terminal_view::FIND_BAR_TOP))
                .left(px(inset))
                .right(px(inset))
                .h(px(crate::terminal_view::FIND_BAR_HEIGHT))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .px(px(11.))
                .rounded(px(8.))
                .bg(theme::hsla(p.raised))
                .font_family(FONT_FAMILY)
                .child(
                    div()
                        .flex_none()
                        .text_color(theme::hsla(p.accent))
                        .child("⌕"),
                )
                .child(div().flex_1().min_w_0().child(search.input.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(10.5))
                        .text_color(theme::hsla(p.bg3))
                        .child(status),
                )
                .into_any_element(),
        )
    }

    fn render_panes(&self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(tab) = self.tabs.get(self.active) else {
            let p = palette();
            let message = self
                .start_error
                .clone()
                .unwrap_or_else(|| "No sessions".into());
            return div()
                .p(px(16.))
                .pt(TITLEBAR_HEIGHT)
                .text_color(theme::hsla(p.failed))
                .font_family(FONT_FAMILY)
                .child(format!("Kiln could not start the shell: {message}"))
                .into_any_element();
        };
        let several = tab.panes.len() > 1 && !self.focus_mode;
        // Focus mode: the pane narrows to reading width, centred: about 72% of the window,
        // and never wider than about 100 columns.
        let reading = self.focus_mode.then(|| {
            let window_w = f32::from(window.viewport_size().width);
            let columns = f32::from(self.metrics.cell_width) * READING_COLUMNS
                + 2. * crate::terminal_view::PADDING_X;
            px((window_w * READING_SHARE).min(columns))
        });
        // Each header keeps clear of the window's controls where it reaches them; elsewhere its
        // text lines up with the timeline's.
        let (left_reserve, right_reserve) = self.chrome_reserve(window);
        let margin = reading.map_or(0., |width| {
            (f32::from(window.viewport_size().width) - f32::from(width)) / 2.
        });
        let last = tab.panes.len() - 1;
        let mut row = div()
            .size_full()
            .flex()
            .flex_row()
            .justify_center()
            .gap(PANE_GAP);
        for (i, pane) in tab.panes.iter().enumerate() {
            let focused = i == tab.focused;
            if self.focus_mode && !focused {
                continue;
            }
            let first = i == 0 || self.focus_mode;
            let lead = HEADER_LEAD.max(if first { left_reserve - margin } else { 0. });
            let trail = HEADER_TRAIL.max(if i == last || self.focus_mode {
                right_reserve - margin
            } else {
                0.
            });
            let header = self.render_pane_header(&pane.view, lead, trail, cx);
            let view = pane.view.clone();
            row = row.child(
                div()
                    .id(("pane", i))
                    .map(|d| match reading {
                        Some(width) => d.flex_none().w(width),
                        None => d.flex_1(),
                    })
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    // Panes that do not have focus step back a little; no borders.
                    .when(several && !focused, |d| d.opacity(0.72))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            if let Some(tab) = this.tabs.get_mut(this.active)
                                && let Some(index) = tab.panes.iter().position(|p| p.view == view)
                                && tab.focused != index
                            {
                                tab.focused = index;
                                this.update_title(window, cx);
                                this.refresh_rail(cx);
                                cx.notify();
                            }
                        }),
                    )
                    .child(header)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .child(
                                gpui::AnyView::from(pane.view.clone())
                                    .cached(gpui::StyleRefinement::default().size_full()),
                            )
                            .when(
                                self.search.as_ref().is_some_and(|s| s.pane == pane.view),
                                |d| d.children(self.render_search_bar()),
                            ),
                    ),
            );
        }
        row.into_any_element()
    }
}

/// Records the window's appearance (dark or light) and, when it changed, recomputes the
/// palette: the "system" theme is Kiln Dark or Kiln Light with it. Dark where it cannot be told.
fn follow_appearance(window: &mut Window) {
    let dark = !matches!(
        window.appearance(),
        gpui::WindowAppearance::Light | gpui::WindowAppearance::VibrantLight
    );
    if theme::set_system_dark(dark) {
        config::set_current(config::current());
        window.refresh();
    }
}

/// The veil behind the palette and the shortcuts pop-up, and how far down they sit.
const SCRIM_TOP: f32 = 52.;

fn scrim() -> gpui::Hsla {
    palette().veil()
}

/// The find bar's right side: "2 of 5 · ⏎ next · ⇧⏎ previous · esc".
pub fn find_status(current: Option<usize>, total: usize) -> String {
    match (current, total) {
        (_, 0) => "no matches · esc".to_string(),
        (Some(i), n) => format!("{} of {n} · ⏎ next · ⇧⏎ previous · esc", i + 1),
        (None, n) => format!("{n} · ⏎ next · ⇧⏎ previous · esc"),
    }
}

/// Identifies a session in a system notification, so a click can bring it back.
pub fn notification_tag(window: u64, tab: u64) -> String {
    format!("kiln:{window}:{tab}")
}

pub fn parse_notification_tag(tag: &str) -> Option<(u64, u64)> {
    let mut parts = tag.strip_prefix("kiln:")?.split(':');
    let window = parts.next()?.parse().ok()?;
    let tab = parts.next()?.parse().ok()?;
    Some((window, tab))
}

/// The program and its first argument ("cargo test"): notifications can show on a locked
/// screen, and the rest of a command line may hold secrets.
fn notification_title(command: &str) -> String {
    let words: Vec<&str> = command.split_whitespace().take(2).collect();
    let mut title = words.join(" ");
    if title.chars().count() > 40 {
        title = title.chars().take(39).collect::<String>() + "…";
    }
    title
}

/// How keys are written: the Mac way under the Mac chrome, else "Ctrl+Shift+D" (follows the
/// `KILN_CHROME` preview, so the Windows preview shows Windows keys).
pub fn key_style() -> KeyStyle {
    if chrome() == Chrome::Mac {
        KeyStyle::Mac
    } else {
        KeyStyle::Pc
    }
}

/// Which platform's window chrome to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chrome {
    Mac,
    Windows,
    Linux,
}

/// The chrome for this platform. `KILN_CHROME=windows|linux|mac` overrides it, so the other
/// platforms' title strip can be previewed (and screenshotted) on any machine.
pub fn chrome() -> Chrome {
    match std::env::var("KILN_CHROME").as_deref() {
        Ok("windows") => Chrome::Windows,
        Ok("linux") => Chrome::Linux,
        Ok("mac") => Chrome::Mac,
        _ if cfg!(target_os = "macos") => Chrome::Mac,
        _ if cfg!(windows) => Chrome::Windows,
        _ => Chrome::Linux,
    }
}

/// A press on a title area: a double click zooms (or whatever the system does), a press moves
/// the window (Windows does that itself, from the drag area).
fn start_window_drag(event: &gpui::MouseDownEvent, window: &mut Window, _: &mut App) {
    if event.click_count == 2 {
        window.titlebar_double_click();
    } else if !cfg!(windows) {
        window.start_window_move();
    }
}

/// The small `>_` mark where the traffic lights would be (Windows and Linux).
fn app_mark() -> AnyElement {
    let p = palette();
    div()
        .font_family(FONT_FAMILY)
        .text_size(px(12.))
        .font_weight(gpui::FontWeight::BOLD)
        .text_color(theme::hsla(p.muted))
        .child(">_")
        .into_any_element()
}

/// Minimize, maximize and close, Windows 11 style (also used on Linux when the app draws its
/// own title bar).
pub(crate) fn window_controls(window: &Window) -> AnyElement {
    let p = palette();
    // Codicon glyphs from the embedded Nerd Font.
    let maximized = window.is_maximized();
    let button = |id: &'static str, glyph: &'static str, area: WindowControlArea, close: bool| {
        div()
            .id(id)
            .w(px(46.))
            .h(TITLEBAR_HEIGHT)
            .flex()
            .items_center()
            .justify_center()
            .font_family(FONT_FAMILY)
            .text_size(px(12.))
            .text_color(theme::hsla(p.fg_dim))
            .when(close, |d| {
                d.hover(|s| s.bg(gpui::rgb(0xc42b1c)).text_color(gpui::white()))
            })
            .when(!close, |d| d.hover(|s| s.bg(theme::hsla(p.bg1))))
            .window_control_area(area)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(move |_, window, cx| match area {
                WindowControlArea::Min => window.minimize_window(),
                WindowControlArea::Max => window.zoom_window(),
                WindowControlArea::Close => window.dispatch_action(Box::new(CloseWindow), cx),
                WindowControlArea::Drag => {}
            })
            .child(glyph)
    };
    div()
        .flex()
        .flex_row()
        .flex_none()
        .child(button("win-min", "\u{eaba}", WindowControlArea::Min, false))
        .child(button(
            "win-max",
            if maximized { "\u{eabb}" } else { "\u{eab9}" },
            WindowControlArea::Max,
            false,
        ))
        .child(button(
            "win-close",
            "\u{eab8}",
            WindowControlArea::Close,
            true,
        ))
        .into_any_element()
}

/// Invisible edges for resizing when Kiln draws its own window frame (Linux).
fn resize_edges(window: &Window) -> Option<AnyElement> {
    let Decorations::Client { tiling } = window.window_decorations() else {
        return None;
    };
    let edge = |id: &'static str, edge: ResizeEdge| {
        div()
            .id(id)
            .absolute()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                window.start_window_resize(edge);
                cx.stop_propagation();
            })
    };
    let w = px(5.);
    Some(
        div()
            .absolute()
            .size_full()
            .when(!tiling.top, |d| {
                d.child(
                    edge("edge-top", ResizeEdge::Top)
                        .top_0()
                        .left(w)
                        .right(w)
                        .h(w),
                )
            })
            .when(!tiling.bottom, |d| {
                d.child(
                    edge("edge-bottom", ResizeEdge::Bottom)
                        .bottom_0()
                        .left(w)
                        .right(w)
                        .h(w),
                )
            })
            .when(!tiling.left, |d| {
                d.child(
                    edge("edge-left", ResizeEdge::Left)
                        .left_0()
                        .top(w)
                        .bottom(w)
                        .w(w),
                )
            })
            .when(!tiling.right, |d| {
                d.child(
                    edge("edge-right", ResizeEdge::Right)
                        .right_0()
                        .top(w)
                        .bottom(w)
                        .w(w),
                )
            })
            .child(
                edge("edge-tl", ResizeEdge::TopLeft)
                    .top_0()
                    .left_0()
                    .size(w),
            )
            .child(
                edge("edge-tr", ResizeEdge::TopRight)
                    .top_0()
                    .right_0()
                    .size(w),
            )
            .child(
                edge("edge-bl", ResizeEdge::BottomLeft)
                    .bottom_0()
                    .left_0()
                    .size(w),
            )
            .child(
                edge("edge-br", ResizeEdge::BottomRight)
                    .bottom_0()
                    .right_0()
                    .size(w),
            )
            .into_any_element(),
    )
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let p = palette();
        let sidebar = self
            .sidebar_visible()
            .then(|| self.render_sidebar(window, cx));
        let rail = if self.rail_visible() {
            self.render_rail(window, cx)
        } else {
            None
        };
        let panes = self.render_panes(window, cx);
        // Behind the pane headers, a strip that moves the window where no header is (beside a
        // pane in focus mode), and over them the `>_` mark and the window controls.
        let mac = chrome() == Chrome::Mac;
        let drag_strip = div()
            .absolute()
            .top_0()
            .left_0()
            .w_full()
            .h(TITLEBAR_HEIGHT)
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, start_window_drag);
        let mark = (!mac && !self.sidebar_visible()).then(|| {
            div()
                .absolute()
                .top_0()
                .left(px(10.))
                .h(TITLEBAR_HEIGHT)
                .flex()
                .items_center()
                .child(app_mark())
        });
        // The flames, over the input boxes, painted after the panes so their places are known.
        // A frame of the flame redraws this view only.
        let flames: Vec<(Entity<TerminalView>, usize, f32)> = self
            .tabs
            .get(self.active)
            .map(|tab| {
                let several = tab.panes.len() > 1 && !self.focus_mode;
                tab.panes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| !self.focus_mode || *i == tab.focused)
                    .map(|(i, pane)| {
                        let dim = if several && i != tab.focused {
                            0.72
                        } else {
                            1.
                        };
                        (pane.view.clone(), self.flame_frame_of(&pane.view), dim)
                    })
                    .collect()
            })
            .unwrap_or_default();
        if self
            .focused_view()
            .is_some_and(|v| v.read(cx).flame().is_some())
        {
            self.ensure_flame(window, cx);
        }
        let flame_overlay = canvas(
            |_, _, _| {},
            move |_, _, window, cx| {
                for (view, frame, opacity) in &flames {
                    if let Some((bounds, failed)) = view.read(cx).flame() {
                        flame::paint(*frame, failed, *opacity, bounds, window);
                    }
                }
            },
        )
        .absolute()
        .size_full();
        let controls = (!mac && !self.rail_visible()).then(|| {
            div()
                .absolute()
                .top_0()
                .right_0()
                .child(window_controls(window))
        });
        let palette_overlay = self.palette.as_ref().map(|(palette, _)| {
            div()
                .id("palette-scrim")
                .absolute()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(SCRIM_TOP))
                .px(px(24.))
                .bg(scrim())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.palette = None;
                        this.focus_active(window, cx);
                        cx.notify();
                    }),
                )
                .child(palette.clone())
        });

        let settings_overlay = self.settings.as_ref().map(|(view, _)| {
            div()
                .id("settings-scrim")
                .absolute()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(22.))
                .px(px(24.))
                .bg(scrim())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.settings = None;
                        this.focus_active(window, cx);
                        cx.notify();
                    }),
                )
                .child(view.clone())
        });

        let shortcuts_overlay = self.shortcuts.as_ref().map(|(view, _)| {
            div()
                .id("shortcuts-scrim")
                .absolute()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(SCRIM_TOP))
                .px(px(24.))
                .bg(scrim())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| {
                        this.shortcuts = None;
                        this.focus_active(window, cx);
                        cx.notify();
                    }),
                )
                .child(view.clone())
        });

        div()
            .id("workspace")
            .key_context("Workspace")
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .bg(theme::hsla(p.bg))
            .text_color(theme::hsla(p.fg))
            .on_action(cx.listener(|this, _: &NewSession, window, cx| {
                let cwd = this.focused_cwd(cx);
                this.new_tab(cwd, None, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SplitRight, window, cx| {
                this.split_right(None, None, window, cx)
            }))
            .on_action(cx.listener(|this, _: &ClosePane, window, cx| {
                if let Some(view) = this.focused_view() {
                    this.close_pane(&view, window, cx);
                }
            }))
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .on_action(
                cx.listener(|this, _: &FocusNextPane, window, cx| this.focus_pane(1, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &FocusPrevPane, window, cx| this.focus_pane(-1, window, cx)),
            )
            .on_action(cx.listener(|this, _: &NextSession, window, cx| {
                let next = (this.active + 1) % this.tabs.len().max(1);
                this.activate(next, window, cx)
            }))
            .on_action(cx.listener(|this, _: &PrevSession, window, cx| {
                let len = this.tabs.len().max(1);
                this.activate((this.active + len - 1) % len, window, cx)
            }))
            .on_action(cx.listener(|this, _: &SelectSession1, w, cx| this.activate(0, w, cx)))
            .on_action(cx.listener(|this, _: &SelectSession2, w, cx| this.activate(1, w, cx)))
            .on_action(cx.listener(|this, _: &SelectSession3, w, cx| this.activate(2, w, cx)))
            .on_action(cx.listener(|this, _: &SelectSession4, w, cx| this.activate(3, w, cx)))
            .on_action(cx.listener(|this, _: &SelectSession5, w, cx| this.activate(4, w, cx)))
            .on_action(cx.listener(|this, _: &SelectSession6, w, cx| this.activate(5, w, cx)))
            .on_action(cx.listener(|this, _: &SelectSession7, w, cx| this.activate(6, w, cx)))
            .on_action(cx.listener(|this, _: &SelectSession8, w, cx| this.activate(7, w, cx)))
            .on_action(cx.listener(|this, _: &SelectSession9, w, cx| {
                let last = this.tabs.len().saturating_sub(1);
                this.activate(last, w, cx)
            }))
            .on_action(cx.listener(|this, _: &ToggleSidebar, _, cx| {
                this.focus_mode = false;
                this.sidebar_open = !this.sidebar_open;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleRail, window, cx| {
                if this.focus_mode && this.rail.is_some() {
                    this.focus_mode = false;
                    cx.notify();
                } else {
                    this.focus_mode = false;
                    this.toggle_rail(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleFocusMode, _, cx| {
                this.focus_mode = !this.focus_mode;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Find, window, cx| this.open_search(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleShortcuts, window, cx| {
                this.toggle_shortcuts(window, cx)
            }))
            .on_action(cx.listener(|this, _: &RenameSession, window, cx| {
                if let Some(id) = this.tabs.get(this.active).map(|t| t.id) {
                    this.start_rename(RenameTarget::Session(id), window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &RenameProject, window, cx| {
                if let Some(root) = this.active_project(cx) {
                    this.start_rename(RenameTarget::Project(root), window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &ProjectColour, window, cx| {
                if let Some(root) = this.active_project(cx) {
                    this.open_colour_menu(root, window, cx)
                }
            }))
            .on_action(cx.listener(|this, _: &CommandPaletteToggle, window, cx| {
                this.toggle_palette(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, window, cx| this.zoom(1., window, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, window, cx| this.zoom(-1., window, cx)))
            .on_action(cx.listener(|this, _: &ZoomReset, window, cx| this.zoom(0., window, cx)))
            .on_action(
                cx.listener(|this, _: &EditConfig, window, cx| this.toggle_settings(window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &OpenConfigFile, window, cx| this.edit_config(window, cx)),
            )
            .on_action(|_: &InstallUpdate, window, cx| crate::update::install(window, cx))
            .when_some(sidebar, |d, sidebar| d.child(sidebar))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .relative()
                    .child(drag_strip)
                    .child(panes)
                    .children(mark)
                    .children(controls),
            )
            .child(flame_overlay)
            .when_some(rail, |d, rail| d.child(rail))
            .when_some(palette_overlay, |d, overlay| d.child(overlay))
            .when_some(shortcuts_overlay, |d, overlay| d.child(overlay))
            .when_some(settings_overlay, |d, overlay| d.child(overlay))
            .when_some(resize_edges(window), |d, edges| d.child(edges))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_find_bar_says_where_you_are() {
        assert_eq!(
            find_status(Some(1), 5),
            "2 of 5 · ⏎ next · ⇧⏎ previous · esc"
        );
        assert_eq!(find_status(None, 0), "no matches · esc");
    }

    #[test]
    fn notification_tags_round_trip_and_titles_stay_short() {
        assert_eq!(
            parse_notification_tag(&notification_tag(7, 42)),
            Some((7, 42))
        );
        assert_eq!(parse_notification_tag("other"), None);
        assert_eq!(
            notification_title("curl -H 'Authorization: Bearer abc' https://x"),
            "curl -H"
        );
        assert_eq!(notification_title("cargo test --workspace"), "cargo test");
    }
}
