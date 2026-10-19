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
    WindowControlArea, div, px,
};
use kiln_term::{GridSize, SessionOptions, ShellCommand};

use crate::config::{self, Config};
use crate::copy;
use crate::grid_paint::{CellMetrics, FONT_FAMILY};
use crate::logs;
use crate::palette::{CommandPalette, PaletteEvent, PaletteItem, PaletteTarget};
use crate::rail::Rail;
use crate::search::Match;
use crate::state::{self, TabState, WindowState};
use crate::terminal_view::{PaneEvent, PaneStatus, TerminalView};
use crate::text_input::{TextInput, TextInputEvent};
use crate::theme::{self, palette};
use crate::{
    ClosePane, CloseWindow, CommandPaletteToggle, CopyLastOutput, CopyLastOutputRaw, EditConfig,
    Find, FocusNextPane, FocusPrevPane, NewSession, NewWindow, NextSession, PrevSession, Quit,
    SelectSession1, SelectSession2, SelectSession3, SelectSession4, SelectSession5, SelectSession6,
    SelectSession7, SelectSession8, SelectSession9, SplitRight, ToggleFocusMode, ToggleRail,
    ToggleSidebar, ZoomIn, ZoomOut, ZoomReset,
};

/// Height of the strip at the top that holds the window controls.
pub const TITLEBAR_HEIGHT: Pixels = px(32.);
const SIDEBAR_WIDTH: Pixels = px(220.);
/// Room for the macOS traffic lights when the sidebar is hidden.
const TRAFFIC_LIGHTS_WIDTH: Pixels = px(78.);
const PANE_GAP: Pixels = px(10.);

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
    search: Option<SearchBar>,
    palette: Option<(Entity<CommandPalette>, Subscription)>,
    metrics: CellMetrics,
    started: Instant,
    bench_startup: bool,
    shell_override: Option<ShellCommand>,
    /// Git root (or folder) per working directory, for grouping sessions by project.
    projects: HashMap<String, PathBuf>,
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
            search: None,
            palette: None,
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
            },
        );
        self.activate(insert_at, window, cx);
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

    fn update_title(&self, window: &mut Window, cx: &App) {
        if let Some(view) = self.focused_view() {
            window.set_window_title(&view.read(cx).title());
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
            });
            self.focus_active(window, cx);
            cx.notify();
        }
    }

    // ----- palette --------------------------------------------------------------------------

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

    fn palette_items(&self, cx: &App) -> Vec<PaletteItem> {
        let mut items = Vec::new();
        let shortcut = |keys: &str| -> String {
            if cfg!(target_os = "macos") {
                keys.replace("Mod+", "⌘")
                    .replace("Shift+", "⇧")
                    .replace("Alt+", "⌥")
            } else {
                keys.replace("Mod+", "Ctrl+Shift+")
                    .replace("Shift+Shift+", "Shift+")
            }
        };
        let mut action = |label: &str, keys: &str, action: Box<dyn Action>| {
            items.push(PaletteItem {
                label: label.into(),
                detail: shortcut(keys),
                kind: "action",
                target: PaletteTarget::Action(action),
            })
        };
        action("New session", "Mod+T", Box::new(NewSession));
        action("Split right", "Mod+D", Box::new(SplitRight));
        action("Find", "Mod+F", Box::new(Find));
        action(
            "Copy last output (fenced)",
            "Mod+Shift+C",
            Box::new(CopyLastOutput),
        );
        action(
            "Copy last output (raw)",
            "Mod+Alt+C",
            Box::new(CopyLastOutputRaw),
        );
        action("Toggle sidebar", "Mod+B", Box::new(ToggleSidebar));
        action("Toggle context rail", "Mod+I", Box::new(ToggleRail));
        action("Focus mode", "Mod+.", Box::new(ToggleFocusMode));
        action("Close pane", "Mod+W", Box::new(ClosePane));
        action("Next session", "Mod+Shift+]", Box::new(NextSession));
        action("Previous session", "Mod+Shift+[", Box::new(PrevSession));
        action("Bigger text", "Mod+=", Box::new(ZoomIn));
        action("Smaller text", "Mod+-", Box::new(ZoomOut));
        action("Reset text size", "Mod+0", Box::new(ZoomReset));
        action("Edit config", "Mod+,", Box::new(EditConfig));
        action("New window", "Mod+N", Box::new(NewWindow));
        action("Quit Kiln", "Mod+Q", Box::new(Quit));

        for (index, tab) in self.tabs.iter().enumerate() {
            let Some(view) = tab.panes.get(tab.focused).map(|p| p.view.read(cx)) else {
                continue;
            };
            items.push(PaletteItem {
                label: view.title(),
                detail: view.cwd().map(copy::short_folder).unwrap_or_default(),
                kind: "session",
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
                detail: copy::short_folder(&project.root),
                kind: "project",
                target: PaletteTarget::Project(root),
            });
        }

        // Past commands from every session, newest first, each once.
        let mut seen = std::collections::HashSet::new();
        let mut commands: Vec<String> = Vec::new();
        for tab in &self.tabs {
            for pane in &tab.panes {
                commands.extend(pane.view.read(cx).commands());
            }
        }
        for command in commands.into_iter().rev() {
            if seen.insert(command.clone()) {
                items.push(PaletteItem {
                    label: command.clone(),
                    detail: "run".into(),
                    kind: "command",
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

    fn apply_config(&mut self, config: Config, window: &mut Window, cx: &mut Context<Self>) {
        log::info!("config reloaded");
        let font_size = px(config.font_size.clamp(6., 72.));
        config::set_current(config);
        self.set_font_size(font_size, window, cx);
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
        if let Some(root) = self.projects.get(cwd) {
            return root.clone();
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
            .unwrap_or(path);
        self.projects.insert(cwd.to_string(), root.clone());
        root
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
    fn watch_state(&self, window: &mut Window, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(2)).await;
                if this.update(cx, |this, cx| this.save_state(cx)).is_err() {
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
        let mut groups: Vec<(PathBuf, Vec<usize>)> = Vec::new();
        for index in 0..self.tabs.len() {
            let cwd = {
                let tab = &self.tabs[index];
                tab.panes
                    .get(tab.focused)
                    .and_then(|pane| pane.view.read(cx).cwd().map(str::to_string))
            };
            let project = cwd.map(|cwd| self.project_of(&cwd)).unwrap_or_default();
            match groups.iter_mut().find(|(root, _)| *root == project) {
                Some((_, tabs)) => tabs.push(index),
                None => groups.push((project, vec![index])),
            }
        }

        let mut list = div().flex().flex_col().gap(px(2.));
        for (root, tabs) in groups {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_default();
            let name = if !home.is_empty() && root == Path::new(&home) {
                "~".to_string()
            } else {
                root.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| {
                        if root.as_os_str().is_empty() {
                            // No folder reported (a shell without integration, or one still starting).
                            "other".into()
                        } else {
                            copy::short_folder(&root.to_string_lossy())
                        }
                    })
            };
            // Projects (folders in git) get their colour; home and loose folders do not.
            let color = (!root.as_os_str().is_empty() && root.join(".git").exists())
                .then(|| state::project_color(&root.to_string_lossy(), &config::current()));
            list = list.child(
                div()
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
                    .child(SharedString::from(name)),
            );
            for index in tabs {
                list = list.child(self.render_session_row(index, cx));
            }
        }

        div()
            .id("sidebar")
            .group("sidebar")
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
            .into_any_element()
    }

    fn render_session_row(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let p = palette();
        let tab = &self.tabs[index];
        let active = index == self.active;
        let view = tab.panes.get(tab.focused).map(|pane| pane.view.read(cx));
        let title = view.map(|v| v.title()).unwrap_or_default();
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
            .on_click(cx.listener(move |this, _, window, cx| this.activate(index, window, cx)))
            .child(
                div()
                    .flex_none()
                    .size(px(6.))
                    .rounded_full()
                    .when_some(dot, |d, color| d.bg(theme::hsla(color))),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_color(theme::hsla(if active { p.fg } else { p.fg_dim }))
                    .child(SharedString::from(title)),
            )
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

    fn render_titlebar(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let p = palette();
        let leading =
            if chrome() == Chrome::Mac && !self.sidebar_visible() && !window.is_fullscreen() {
                TRAFFIC_LIGHTS_WIDTH
            } else {
                px(10.)
            };
        let title = self
            .focused_view()
            .map(|v| v.read(cx).title())
            .unwrap_or_default();
        div()
            .id("titlebar")
            .flex_none()
            .h(TITLEBAR_HEIGHT)
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .pl(leading)
            .window_control_area(WindowControlArea::Drag)
            .on_mouse_down(MouseButton::Left, |event, window, _| {
                if event.click_count == 2 {
                    window.titlebar_double_click();
                } else if !cfg!(windows) {
                    window.start_window_move();
                }
            })
            .when(chrome() != Chrome::Mac && !self.sidebar_visible(), |d| {
                d.child(app_mark())
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .justify_center()
                    .font_family(FONT_FAMILY)
                    .text_size(px(11.))
                    .text_color(theme::hsla(p.muted))
                    .whitespace_nowrap()
                    .overflow_hidden()
                    .text_ellipsis()
                    .child(SharedString::from(title)),
            )
            .when(chrome() != Chrome::Mac && !self.rail_visible(), |d| {
                d.child(window_controls(window))
            })
            .into_any_element()
    }

    fn sidebar_visible(&self) -> bool {
        self.sidebar_open && !self.focus_mode
    }

    pub(crate) fn rail_visible(&self) -> bool {
        self.rail.is_some() && !self.focus_mode
    }

    fn render_search_bar(&self) -> Option<AnyElement> {
        let search = self.search.as_ref()?;
        let p = palette();
        let count = match (search.current, search.matches.len()) {
            (_, 0) => "no matches".to_string(),
            (Some(i), n) => format!("{}/{}", i + 1, n),
            (None, n) => format!("{n}"),
        };
        Some(
            div()
                .absolute()
                .top(px(8.))
                .right(px(16.))
                .w(px(320.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(8.))
                .bg(theme::hsla(p.bar))
                .shadow_lg()
                .font_family(FONT_FAMILY)
                .child(div().flex_1().min_w_0().child(search.input.clone()))
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(theme::hsla(p.muted))
                        .child(count),
                )
                .into_any_element(),
        )
    }

    fn render_panes(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(tab) = self.tabs.get(self.active) else {
            let p = palette();
            let message = self
                .start_error
                .clone()
                .unwrap_or_else(|| "No sessions".into());
            return div()
                .p(px(16.))
                .text_color(theme::hsla(p.failed))
                .font_family(FONT_FAMILY)
                .child(format!("Kiln could not start the shell: {message}"))
                .into_any_element();
        };
        let several = tab.panes.len() > 1 && !self.focus_mode;
        let mut row = div().size_full().flex().flex_row().gap(PANE_GAP);
        for (i, pane) in tab.panes.iter().enumerate() {
            let focused = i == tab.focused;
            if self.focus_mode && !focused {
                continue;
            }
            let view = pane.view.clone();
            row = row.child(
                div()
                    .id(("pane", i))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .relative()
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
                    .child(pane.view.clone()),
            );
        }
        row.into_any_element()
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
        let titlebar = self.render_titlebar(window, cx);
        let panes = self.render_panes(cx);
        let search = self.render_search_bar();
        let palette_overlay = self.palette.as_ref().map(|(palette, _)| {
            div()
                .id("palette-scrim")
                .absolute()
                .size_full()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(72.))
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
            .on_action(cx.listener(|this, _: &CommandPaletteToggle, window, cx| {
                this.toggle_palette(window, cx)
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, window, cx| this.zoom(1., window, cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, window, cx| this.zoom(-1., window, cx)))
            .on_action(cx.listener(|this, _: &ZoomReset, window, cx| this.zoom(0., window, cx)))
            .on_action(cx.listener(|this, _: &EditConfig, window, cx| this.edit_config(window, cx)))
            .when_some(sidebar, |d, sidebar| d.child(sidebar))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .flex()
                    .flex_col()
                    .child(titlebar)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .relative()
                            .child(panes)
                            .when_some(search, |d, s| d.child(s)),
                    ),
            )
            .when_some(rail, |d, rail| d.child(rail))
            .when_some(palette_overlay, |d, overlay| d.child(overlay))
            .when_some(resize_edges(window), |d, edges| d.child(edges))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
