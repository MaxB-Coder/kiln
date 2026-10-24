//! A terminal session: one shell on one PTY, and the engine state it drives.

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{self, Osc52, Term};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle};
use anyhow::{Context as _, Result};

use std::sync::Mutex;

use crate::blocks::{Block, BlockRecorder, LiveState};
use crate::integration;
use crate::marks::MarkRecord;
use crate::pty_loop::{LoopSender, Msg, PtyLoop};
use crate::shell::ShellCommand;

/// The engine type a session drives. The UI locks it to render and select.
pub type EngineTerm = Term<EventProxy>;

/// Everything a session reports to the UI. Sent from the PTY thread.
#[derive(Debug)]
pub enum SessionEvent {
    Term(Event),
    Mark(MarkRecord),
    /// A command finished.
    Block(Block),
}

/// Forwards engine events to the UI's callback.
#[derive(Clone)]
pub struct EventProxy {
    callback: Arc<dyn Fn(SessionEvent) + Send + Sync>,
    /// Answers to the program (cursor position, device attributes) go straight back to the PTY
    /// from the PTY thread: programs such as PowerShell wait for them before reading input.
    writer: Arc<std::sync::OnceLock<LoopSender>>,
}

impl EventProxy {
    pub(crate) fn send(&self, event: SessionEvent) {
        (self.callback)(event)
    }

    #[cfg(test)]
    pub(crate) fn for_test() -> Self {
        Self {
            callback: Arc::new(|_| {}),
            writer: Arc::default(),
        }
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = &event
            && let Some(writer) = self.writer.get()
        {
            let _ = writer.send(Msg::Input(text.clone().into_bytes().into()));
            return;
        }
        self.send(SessionEvent::Term(event))
    }
}

/// Grid size in cells, plus the cell size in physical pixels for programs that ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub columns: usize,
    pub lines: usize,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

impl From<GridSize> for WindowSize {
    fn from(size: GridSize) -> Self {
        WindowSize {
            num_lines: size.lines as u16,
            num_cols: size.columns as u16,
            cell_width: size.cell_width,
            cell_height: size.cell_height,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SessionOptions {
    pub shell: ShellCommand,
    pub working_directory: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub scrollback_lines: usize,
    /// Load Kiln's shell integration (blocks need it).
    pub shell_integration: bool,
    /// Kiln's minimal prompt instead of the shell's own (needs the integration).
    pub prompt: Option<integration::MinimalPrompt>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        let mut env = HashMap::new();
        env.insert("TERM".into(), "xterm-256color".into());
        env.insert("COLORTERM".into(), "truecolor".into());
        env.insert("TERM_PROGRAM".into(), "kiln".into());
        env.insert(
            "TERM_PROGRAM_VERSION".into(),
            env!("CARGO_PKG_VERSION").into(),
        );
        // Apps started from Finder or a desktop launcher often have no locale; without one,
        // shells fall back to ASCII and mangle UTF-8 input. C.UTF-8 is built into glibc, so it
        // always exists on Linux (en_US.UTF-8 may not be generated); macOS has en_US.UTF-8.
        if ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .all(|var| std::env::var_os(var).is_none())
        {
            let lang = if cfg!(target_os = "macos") {
                "en_US.UTF-8"
            } else {
                "C.UTF-8"
            };
            env.insert("LANG".into(), lang.into());
        }
        Self {
            shell: ShellCommand::platform_default(),
            working_directory: home_dir(),
            env,
            scrollback_lines: 10_000,
            shell_integration: true,
            prompt: None,
        }
    }
}

pub struct Session {
    term: Arc<FairMutex<EngineTerm>>,
    live: Arc<Mutex<LiveState>>,
    sender: LoopSender,
    size: GridSize,
    shell: ShellCommand,
    integrated: bool,
    /// The shell prints Kiln's minimal prompt (blank cells for the flame).
    minimal_prompt: bool,
    pid: Option<u32>,
}

impl Session {
    /// Starts the shell and its PTY thread. `on_event` runs on the PTY thread.
    pub fn spawn(
        mut options: SessionOptions,
        size: GridSize,
        on_event: impl Fn(SessionEvent) + Send + Sync + 'static,
    ) -> Result<Self> {
        let proxy = EventProxy {
            callback: Arc::new(on_event),
            writer: Arc::default(),
        };

        let label = options.shell.clone();
        let prompt = options.prompt;
        let integrated = options.shell_integration
            && match integration::default_dir().map(|dir| integration::install(&dir)) {
                Some(Ok(installed)) => integration::apply(
                    &installed,
                    &mut options.shell,
                    &mut options.env,
                    options.prompt,
                ),
                Some(Err(err)) => {
                    log::warn!("shell integration not installed: {err}");
                    false
                }
                None => false,
            };

        let config = term::Config {
            scrolling_history: options.scrollback_lines,
            default_cursor_style: CursorStyle {
                shape: CursorShape::Underline,
                blinking: true,
            },
            // Programs may set the clipboard, never read it.
            osc52: Osc52::OnlyCopy,
            ..Default::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(config, &size, proxy.clone())));

        let pty_options = tty::Options {
            shell: Some(tty::Shell::new(
                options.shell.program.clone(),
                options.shell.args.clone(),
            )),
            working_directory: options.working_directory,
            drain_on_exit: true,
            env: options.env,
            #[cfg(windows)]
            escape_args: true,
        };
        let pty = tty::new(&pty_options, size.into(), 0)
            .with_context(|| format!("starting {}", options.shell.program))?;
        #[cfg(unix)]
        let pid = Some(pty.child().id());
        #[cfg(windows)]
        let pid = pty.child_watcher().pid().map(|p| p.get());

        let live = Arc::new(Mutex::new(LiveState::default()));
        let recorder = BlockRecorder::new(options.scrollback_lines);
        let proxy_writer = proxy.writer.clone();
        let pty_loop = PtyLoop::new(term.clone(), proxy, pty, true, recorder, live.clone())?;
        let sender = pty_loop.sender();
        let _ = proxy_writer.set(sender.clone());
        pty_loop.spawn();

        Ok(Self {
            term,
            live,
            sender,
            size,
            shell: label,
            integrated,
            minimal_prompt: integrated && prompt.is_some(),
            pid,
        })
    }

    pub fn term(&self) -> &Arc<FairMutex<EngineTerm>> {
        &self.term
    }

    pub fn shell(&self) -> &ShellCommand {
        &self.shell
    }

    /// The shell's state as its integration marks report it. Lock the term first.
    pub fn live(&self) -> LiveState {
        self.live.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The shell's process id.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Whether Kiln's shell integration was loaded into this shell.
    pub fn integration_loaded(&self) -> bool {
        self.integrated
    }

    /// Whether the shell prints Kiln's minimal prompt, so Kiln draws the flame over it.
    pub fn minimal_prompt(&self) -> bool {
        self.minimal_prompt
    }

    pub fn size(&self) -> GridSize {
        self.size
    }

    /// Sends bytes to the shell.
    pub fn write(&self, bytes: impl Into<Cow<'static, [u8]>>) {
        let bytes = bytes.into();
        if !bytes.is_empty() {
            let _ = self.sender.send(Msg::Input(bytes));
        }
    }

    /// Resizes the grid and the PTY. Does nothing if the size is unchanged.
    pub fn resize(&mut self, size: GridSize) {
        if size == self.size || size.columns == 0 || size.lines == 0 {
            return;
        }
        self.size = size;
        {
            // Same lock order as the PTY thread: the terminal, then the live state.
            let mut term = self.term.lock();
            let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
            crate::blocks::resize_keeping_marks(&mut term, &mut live, size);
        }
        let _ = self.sender.send(Msg::Resize(size.into()));
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.sender.send(Msg::Shutdown);
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(unix)]
    let home = std::env::var_os("HOME");
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE");
    home.map(PathBuf::from)
}
