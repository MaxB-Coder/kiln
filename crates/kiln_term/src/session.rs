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
}

/// Forwards engine events to the UI's callback.
#[derive(Clone)]
pub struct EventProxy(Arc<dyn Fn(SessionEvent) + Send + Sync>);

impl EventProxy {
    pub(crate) fn send(&self, event: SessionEvent) {
        (self.0)(event)
    }
}

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
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
        Self {
            shell: ShellCommand::platform_default(),
            working_directory: home_dir(),
            env,
            scrollback_lines: 10_000,
        }
    }
}

pub struct Session {
    term: Arc<FairMutex<EngineTerm>>,
    sender: LoopSender,
    size: GridSize,
    shell: ShellCommand,
}

impl Session {
    /// Starts the shell and its PTY thread. `on_event` runs on the PTY thread.
    pub fn spawn(
        options: SessionOptions,
        size: GridSize,
        on_event: impl Fn(SessionEvent) + Send + Sync + 'static,
    ) -> Result<Self> {
        let proxy = EventProxy(Arc::new(on_event));

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

        let pty_loop = PtyLoop::new(term.clone(), proxy, pty, true)?;
        let sender = pty_loop.sender();
        pty_loop.spawn();

        Ok(Self {
            term,
            sender,
            size,
            shell: options.shell,
        })
    }

    pub fn term(&self) -> &Arc<FairMutex<EngineTerm>> {
        &self.term
    }

    pub fn shell(&self) -> &ShellCommand {
        &self.shell
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
        self.term.lock().resize(size);
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
