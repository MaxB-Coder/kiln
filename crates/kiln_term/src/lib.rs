//! Kiln's terminal session layer.
//!
//! Spawns the shell on a PTY, runs the read loop that feeds `alacritty_terminal`, watches for
//! shell-integration marks, and encodes keyboard and mouse input. No UI code lives here, so it
//! builds and tests headless on every platform.

pub mod blocks;
pub mod input;
pub mod integration;
pub mod marks;
pub mod process;
mod pty_loop;
mod session;
mod shell;

pub use alacritty_terminal as engine;
pub use blocks::{Block, LiveState, Phase};
pub use marks::{Mark, MarkRecord};
pub use session::{EngineTerm, GridSize, Session, SessionEvent, SessionOptions};
pub use shell::ShellCommand;
