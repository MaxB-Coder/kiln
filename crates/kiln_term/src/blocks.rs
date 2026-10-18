//! Command blocks.
//!
//! The recorder runs on the PTY thread at each shell-integration mark, with the engine locked
//! and fed exactly up to the mark. When a command finishes it copies the command's output out of
//! the live grid into a [`Block`] and clears the scrollback, so blocks survive scrollback
//! trimming and window resizes, and row numbers in the live grid stay meaningful.

use std::time::{Duration, Instant, SystemTime};

use alacritty_terminal::grid::{Dimensions, GridCell};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Flags};

use crate::marks::Mark;
use crate::session::EngineTerm;

/// Where the shell is, as seen through its marks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// No prompt mark yet: no shell integration, or the shell is still starting.
    #[default]
    Unknown,
    /// The shell shows its prompt and the user edits a command.
    Prompt,
    /// A command runs.
    Running,
}

/// What the UI needs to split the live grid into "running output" and "prompt".
///
/// Rows are absolute: `history_size + screen line` when recorded. The recorder clears the
/// scrollback after each command, so they stay valid unless one command alone overflows it.
#[derive(Clone, Debug, Default)]
pub struct LiveState {
    pub phase: Phase,
    /// A prompt mark arrived at least once, so the shell integration works.
    pub integrated: bool,
    /// First row of the current prompt.
    pub prompt_top: Option<usize>,
    /// First row of the running command's output.
    pub output_top: Option<usize>,
    /// The running command.
    pub command: Option<String>,
    pub command_started: Option<SystemTime>,
    pub cwd: Option<String>,
    pub last_exit: Option<i32>,
    /// The `node` on the shell's PATH, as the integration reports it.
    pub node_path: Option<String>,
}

/// One finished command.
#[derive(Clone, Debug)]
pub struct Block {
    pub id: u64,
    pub command: String,
    pub cwd: Option<String>,
    pub started_at: SystemTime,
    pub duration: Duration,
    pub exit_code: Option<i32>,
    /// Output as logical lines (soft-wrapped rows joined), trailing blanks trimmed.
    pub lines: Vec<Vec<Cell>>,
    /// The start of the output scrolled out of the scrollback before the command finished.
    pub truncated: bool,
    /// Output the shell printed before its first prompt (login banner, startup errors).
    pub startup: bool,
}

impl Block {
    /// Exited with an error. Ctrl-C (130, SIGINT) is the user stopping it, not a failure.
    pub fn failed(&self) -> bool {
        self.exit_code.is_some_and(|code| code != 0 && code != 130)
    }

    /// Output as plain text, one line per logical line.
    pub fn output_text(&self) -> String {
        let mut out = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&line_text(line));
        }
        out
    }

    /// Commands that clear the screen also clear the timeline.
    pub fn clears_timeline(&self) -> bool {
        matches!(
            self.command.trim(),
            "clear" | "reset" | "cls" | "Clear-Host" | "tput clear"
        )
    }
}

/// Text of one logical line (wide-character spacers skipped).
pub fn line_text(cells: &[Cell]) -> String {
    let mut out = String::with_capacity(cells.len());
    for cell in cells {
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        out.push(if cell.c == '\t' { ' ' } else { cell.c });
        if let Some(extra) = cell.zerowidth() {
            out.extend(extra);
        }
    }
    out.trim_end().to_string()
}

pub struct BlockRecorder {
    next_id: u64,
    scrollback_limit: usize,
    input_start: Option<(usize, usize)>,
    command: Option<String>,
    started: Option<(Instant, SystemTime)>,
    output_top: Option<usize>,
    cwd: Option<String>,
    seen_prompt: bool,
}

impl BlockRecorder {
    pub fn new(scrollback_limit: usize) -> Self {
        Self {
            next_id: 1,
            scrollback_limit,
            input_start: None,
            command: None,
            started: None,
            output_top: None,
            cwd: None,
            seen_prompt: false,
        }
    }

    /// Handles one mark. Returns a block when a command finished.
    pub fn on_mark(
        &mut self,
        mark: &Mark,
        term: &mut EngineTerm,
        live: &mut LiveState,
    ) -> Option<Block> {
        let (abs, column) = cursor_abs(term);
        match mark {
            Mark::PromptStart => {
                live.integrated = true;
                live.phase = Phase::Prompt;
                live.output_top = None;
                live.command = None;
                let startup = (!self.seen_prompt)
                    .then(|| self.startup_block(term, abs))
                    .flatten();
                self.seen_prompt = true;
                // The startup block cleared the scrollback, which moves the prompt's row.
                let (abs, _) = cursor_abs(term);
                live.prompt_top = Some(abs);
                startup
            }
            Mark::InputStart => {
                self.input_start = Some((abs, column));
                live.phase = Phase::Prompt;
                None
            }
            Mark::OutputStart { command } => {
                let command = command
                    .clone()
                    .filter(|c| !c.trim().is_empty())
                    .or_else(|| self.input_text(term, abs, column));
                let top = if column == 0 { abs } else { abs + 1 };
                let now = (Instant::now(), SystemTime::now());
                self.command = command.clone();
                self.started = Some(now);
                self.output_top = Some(top);
                self.cwd = live.cwd.clone();
                live.phase = Phase::Running;
                live.output_top = Some(top);
                live.command = command;
                live.command_started = Some(now.1);
                None
            }
            Mark::CommandEnd { exit_code } => {
                let mut block = self.finish(term, *exit_code, abs, column);
                if let Some(block) = block.as_mut() {
                    block.cwd = self.cwd.take().or_else(|| live.cwd.clone());
                }
                live.last_exit = *exit_code;
                live.output_top = None;
                live.command = None;
                live.command_started = None;
                block
            }
            Mark::Cwd { path, .. } => {
                live.cwd = Some(path.clone());
                None
            }
            Mark::NodePath(path) => {
                live.node_path = (!path.is_empty()).then(|| path.clone());
                None
            }
        }
    }

    /// Whatever the shell printed before its first prompt, as a block of its own.
    fn startup_block(&mut self, term: &mut EngineTerm, abs: usize) -> Option<Block> {
        let lines = if abs > 0 {
            logical_lines(term, 0, abs - 1)
        } else {
            Vec::new()
        };
        term.grid_mut().clear_history();
        if lines.is_empty() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        Some(Block {
            id,
            command: String::new(),
            cwd: None,
            started_at: SystemTime::now(),
            duration: Duration::ZERO,
            exit_code: None,
            lines,
            truncated: false,
            startup: true,
        })
    }

    /// Text the user typed: from the input mark to just before the cursor.
    fn input_text(&self, term: &EngineTerm, abs: usize, column: usize) -> Option<String> {
        let (start_abs, start_col) = self.input_start?;
        let history = term.grid().history_size();
        let start = Point::new(Line(start_abs as i32 - history as i32), Column(start_col));
        let end = if column == 0 {
            Point::new(
                Line(abs as i32 - history as i32 - 1),
                Column(term.columns() - 1),
            )
        } else {
            Point::new(Line(abs as i32 - history as i32), Column(column - 1))
        };
        if end < start || start.line < Line(-(history as i32)) {
            return None;
        }
        let text = term.bounds_to_string(start, end);
        let text = text.trim();
        (!text.is_empty()).then(|| text.to_string())
    }

    fn finish(
        &mut self,
        term: &mut EngineTerm,
        exit_code: Option<i32>,
        abs: usize,
        column: usize,
    ) -> Option<Block> {
        let input_start = self.input_start.take();
        let (top, command) = match (self.output_top.take(), self.command.take()) {
            (Some(top), command) => (top, command.unwrap_or_default()),
            // No output mark (bash before 4.4): the command is the input line and the output
            // starts below it. An empty line is not a command.
            (None, _) => {
                let (start_abs, start_col) = input_start?;
                let history = term.grid().history_size() as i32;
                let line = Line(start_abs as i32 - history);
                if line < Line(-history) || start_abs >= abs {
                    return None;
                }
                let end = Point::new(line, Column(term.columns() - 1));
                let command = term.bounds_to_string(Point::new(line, Column(start_col)), end);
                let command = command.trim().to_string();
                if command.is_empty() {
                    return None;
                }
                (start_abs + 1, command)
            }
        };
        let (started, started_at) = self
            .started
            .take()
            .unwrap_or((Instant::now(), SystemTime::now()));

        let history = term.grid().history_size();
        // If one command filled the whole scrollback, its first rows are gone and the stored
        // row number no longer points at them: keep what is left.
        let truncated = history >= self.scrollback_limit && self.scrollback_limit > 0;
        let top = if truncated { 0 } else { top };
        let end = if column > 0 {
            abs
        } else {
            abs.saturating_sub(1)
        };
        let lines = if top <= end {
            logical_lines(term, top, end)
        } else {
            Vec::new()
        };

        // Start the next command with an empty scrollback (see the module docs).
        term.grid_mut().clear_history();
        term.selection = None;

        let id = self.next_id;
        self.next_id += 1;
        Some(Block {
            id,
            command,
            cwd: None,
            started_at,
            duration: started.elapsed(),
            exit_code,
            lines,
            truncated,
            startup: false,
        })
    }
}

fn cursor_abs(term: &EngineTerm) -> (usize, usize) {
    let grid = term.grid();
    let point = grid.cursor.point;
    let abs = grid.history_size() as i32 + point.line.0;
    (abs.max(0) as usize, point.column.0)
}

/// Copies rows `top..=end` (absolute) as logical lines.
fn logical_lines(term: &EngineTerm, top: usize, end: usize) -> Vec<Vec<Cell>> {
    let grid = term.grid();
    let history = grid.history_size() as i32;
    let last_row = grid.screen_lines() as i32 - 1;
    let mut lines = Vec::new();
    let mut current: Vec<Cell> = Vec::new();
    for abs in top..=end {
        let line = abs as i32 - history;
        if line < -history || line > last_row {
            continue;
        }
        let row = &grid[Line(line)];
        let columns = row.len();
        let wrapped = columns > 0 && row[Column(columns - 1)].flags.contains(Flags::WRAPLINE);
        for col in 0..columns {
            let cell = &row[Column(col)];
            if cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let mut cell = cell.clone();
            cell.flags.remove(Flags::WRAPLINE);
            current.push(cell);
        }
        if !wrapped {
            trim_trailing_blanks(&mut current);
            lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        trim_trailing_blanks(&mut current);
        lines.push(current);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

fn trim_trailing_blanks(cells: &mut Vec<Cell>) {
    while cells.last().is_some_and(|cell| cell.is_empty()) {
        cells.pop();
    }
}
