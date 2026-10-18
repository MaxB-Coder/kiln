//! The PTY read/write loop.
//!
//! Adapted from `alacritty_terminal/src/event_loop.rs` (Apache-2.0, Copyright The Alacritty
//! Project). Kiln runs its own copy for one reason: it feeds the engine up to each
//! shell-integration mark and records the cursor position there (see `marks`). The engine
//! itself is used unmodified.
//!
//! One deliberate difference: alacritty dispatches on poll tokens that are crate-private on Unix,
//! so this loop asks the PTY for a child event and tries a read on every wakeup instead. Both
//! calls are non-blocking, so the extra calls are cheap.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, OnResize, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::tty::{self, ChildEvent, EventedPty, EventedReadWrite};
use alacritty_terminal::vte::ansi;
use log::error;
use polling::{Event as PollingEvent, Events, PollMode, Poller};

use std::sync::Mutex;

use crate::blocks::{BlockRecorder, LiveState};
use crate::marks::{MarkRecord, MarkScanner};
use crate::session::{EngineTerm, EventProxy, SessionEvent};

/// Max bytes to read from the PTY before forced terminal synchronization.
const READ_BUFFER_SIZE: usize = 0x10_0000;

/// Max bytes to read from the PTY while the terminal is locked.
const MAX_LOCKED_READ: usize = u16::MAX as usize;

#[derive(Debug)]
pub(crate) enum Msg {
    Input(Cow<'static, [u8]>),
    Resize(WindowSize),
    Shutdown,
}

#[derive(Clone)]
pub(crate) struct LoopSender {
    sender: Sender<Msg>,
    poller: Arc<Poller>,
}

impl LoopSender {
    pub fn send(&self, msg: Msg) -> io::Result<()> {
        self.sender
            .send(msg)
            .map_err(|_| io::Error::from(ErrorKind::BrokenPipe))?;
        self.poller.notify()
    }
}

pub(crate) struct PtyLoop {
    poll: Arc<Poller>,
    pty: tty::Pty,
    rx: PeekableReceiver<Msg>,
    tx: Sender<Msg>,
    term: Arc<FairMutex<EngineTerm>>,
    proxy: EventProxy,
    drain_on_exit: bool,
    recorder: BlockRecorder,
    live: Arc<Mutex<LiveState>>,
}

#[derive(Default)]
struct State {
    write_list: VecDeque<Cow<'static, [u8]>>,
    writing: Option<Writing>,
    parser: ansi::Processor,
    scanner: MarkScanner,
}

impl PtyLoop {
    pub fn new(
        term: Arc<FairMutex<EngineTerm>>,
        proxy: EventProxy,
        pty: tty::Pty,
        drain_on_exit: bool,
        recorder: BlockRecorder,
        live: Arc<Mutex<LiveState>>,
    ) -> io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            poll: Poller::new()?.into(),
            pty,
            rx: PeekableReceiver::new(rx),
            tx,
            term,
            proxy,
            drain_on_exit,
            recorder,
            live,
        })
    }

    pub fn sender(&self) -> LoopSender {
        LoopSender {
            sender: self.tx.clone(),
            poller: self.poll.clone(),
        }
    }

    /// Returns `false` when a shutdown message was received.
    fn drain_recv_channel(&mut self, state: &mut State) -> bool {
        while let Some(msg) = self.rx.recv() {
            match msg {
                Msg::Input(input) => state.write_list.push_back(input),
                Msg::Resize(size) => self.pty.on_resize(size),
                Msg::Shutdown => return false,
            }
        }
        true
    }

    fn pty_read(&mut self, state: &mut State, buf: &mut [u8]) -> io::Result<()> {
        let mut unprocessed = 0;
        let mut processed = 0;

        // Reserve the next terminal lock for PTY reading.
        let _terminal_lease = Some(self.term.lease());
        let mut terminal = None;

        loop {
            match self.pty.reader().read(&mut buf[unprocessed..]) {
                // Received on Windows/macOS when no more data is readable from the PTY.
                Ok(0) if unprocessed == 0 => break,
                Ok(got) => unprocessed += got,
                Err(err) => match err.kind() {
                    ErrorKind::Interrupted | ErrorKind::WouldBlock => {
                        if unprocessed == 0 {
                            break;
                        }
                    }
                    _ => return Err(err),
                },
            }

            let terminal = match &mut terminal {
                Some(terminal) => terminal,
                None => terminal.insert(match self.term.try_lock_unfair() {
                    // Force block if we are at the buffer size limit.
                    None if unprocessed >= READ_BUFFER_SIZE => self.term.lock_unfair(),
                    None => continue,
                    Some(terminal) => terminal,
                }),
            };

            // Feed the engine, stopping at each mark so the cursor position recorded is exact.
            let mut chunk = &buf[..unprocessed];
            while !chunk.is_empty() {
                let (consumed, mark) = state.scanner.scan(chunk);
                state.parser.advance(&mut **terminal, &chunk[..consumed]);
                if let Some(mark) = mark {
                    let grid = terminal.grid();
                    let record = MarkRecord {
                        mark: mark.clone(),
                        line: grid.cursor.point.line.0,
                        column: grid.cursor.point.column.0,
                        history_size: grid.history_size(),
                    };
                    let block = {
                        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
                        self.recorder.on_mark(&mark, &mut **terminal, &mut live)
                    };
                    self.proxy.send(SessionEvent::Mark(record));
                    if let Some(block) = block {
                        self.proxy.send(SessionEvent::Block(block));
                    }
                }
                chunk = &chunk[consumed..];
            }

            processed += unprocessed;
            unprocessed = 0;

            if processed >= MAX_LOCKED_READ {
                break;
            }
        }

        // Queue a redraw unless all processed bytes were inside a synchronized update.
        if state.parser.sync_bytes_count() < processed && processed > 0 {
            self.proxy.send_event(Event::Wakeup);
        }

        Ok(())
    }

    fn pty_write(&mut self, state: &mut State) -> io::Result<()> {
        state.ensure_next();

        'write_many: while let Some(mut current) = state.writing.take() {
            'write_one: loop {
                match self.pty.writer().write(current.remaining_bytes()) {
                    Ok(0) => {
                        state.writing = Some(current);
                        break 'write_many;
                    }
                    Ok(n) => {
                        current.written += n;
                        if current.finished() {
                            state.goto_next();
                            break 'write_one;
                        }
                    }
                    Err(err) => {
                        state.writing = Some(current);
                        match err.kind() {
                            ErrorKind::Interrupted | ErrorKind::WouldBlock => break 'write_many,
                            _ => return Err(err),
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Handles a child exit, if one is pending. Returns `true` when the loop should stop.
    fn check_child_exit(&mut self, state: &mut State, buf: &mut [u8]) -> bool {
        let Some(ChildEvent::Exited(status)) = self.pty.next_child_event() else {
            return false;
        };
        if self.drain_on_exit {
            let _ = self.pty_read(state, buf);
        }
        if let Some(status) = status {
            self.proxy.send_event(Event::ChildExit(status));
        }
        self.term.lock().exit();
        self.proxy.send_event(Event::Wakeup);
        true
    }

    pub fn spawn(mut self) -> JoinHandle<()> {
        std::thread::Builder::new()
            .name("kiln PTY".into())
            .spawn(move || {
                let mut state = State::default();
                let mut buf = vec![0u8; READ_BUFFER_SIZE];

                let poll_opts = PollMode::Level;
                let mut interest = PollingEvent::readable(0);

                // SAFETY: the PTY outlives its registration; it is deregistered below.
                if let Err(err) = unsafe { self.pty.register(&self.poll, interest, poll_opts) } {
                    error!("PTY registration error: {err}");
                    return;
                }

                let mut events = Events::with_capacity(NonZeroUsize::new(1024).unwrap());

                'event_loop: loop {
                    // Wake up when a synchronized update times out.
                    let timeout = state
                        .parser
                        .sync_timeout()
                        .sync_timeout()
                        .map(|t| t.saturating_duration_since(Instant::now()));

                    events.clear();
                    if let Err(err) = self.poll.wait(&mut events, timeout) {
                        match err.kind() {
                            ErrorKind::Interrupted => continue,
                            _ => {
                                error!("PTY polling error: {err}");
                                break 'event_loop;
                            }
                        }
                    }

                    // Synchronized update timeout.
                    if events.is_empty() && self.rx.peek().is_none() {
                        state.parser.stop_sync(&mut *self.term.lock());
                        self.proxy.send_event(Event::Wakeup);
                        continue;
                    }

                    if !self.drain_recv_channel(&mut state) {
                        break;
                    }

                    let mut readable = false;
                    let mut writable = false;
                    for event in events.iter() {
                        if event.is_interrupt() {
                            continue;
                        }
                        readable |= event.readable;
                        writable |= event.writable;
                    }

                    if readable && let Err(err) = self.pty_read(&mut state, &mut buf) {
                        // On Linux, reading the master side fails with EIO once the child
                        // hangs up; loop round for the exit event.
                        #[cfg(target_os = "linux")]
                        if err.raw_os_error() == Some(libc::EIO) {
                            if self.check_child_exit(&mut state, &mut buf) {
                                break 'event_loop;
                            }
                            continue;
                        }
                        error!("Error reading from PTY: {err}");
                        break 'event_loop;
                    }

                    if self.check_child_exit(&mut state, &mut buf) {
                        break 'event_loop;
                    }

                    if writable && let Err(err) = self.pty_write(&mut state) {
                        error!("Error writing to PTY: {err}");
                        break 'event_loop;
                    }

                    // Register write interest if necessary.
                    let needs_write = state.needs_write();
                    if needs_write != interest.writable {
                        interest.writable = needs_write;
                        if let Err(err) = self.pty.reregister(&self.poll, interest, poll_opts) {
                            error!("PTY re-registration error: {err}");
                            break 'event_loop;
                        }
                    }
                }

                let _ = self.pty.deregister(&self.poll);
                self.proxy.send_event(Event::Exit);
            })
            .expect("spawn PTY thread")
    }
}

impl State {
    fn ensure_next(&mut self) {
        if self.writing.is_none() {
            self.goto_next();
        }
    }

    fn goto_next(&mut self) {
        self.writing = self
            .write_list
            .pop_front()
            .map(|source| Writing { source, written: 0 });
    }

    fn needs_write(&self) -> bool {
        self.writing.is_some() || !self.write_list.is_empty()
    }
}

struct Writing {
    source: Cow<'static, [u8]>,
    written: usize,
}

impl Writing {
    fn remaining_bytes(&self) -> &[u8] {
        &self.source[self.written..]
    }

    fn finished(&self) -> bool {
        self.written >= self.source.len()
    }
}

struct PeekableReceiver<T> {
    rx: Receiver<T>,
    peeked: Option<T>,
}

impl<T> PeekableReceiver<T> {
    fn new(rx: Receiver<T>) -> Self {
        Self { rx, peeked: None }
    }

    fn peek(&mut self) -> Option<&T> {
        if self.peeked.is_none() {
            self.peeked = self.rx.try_recv().ok();
        }
        self.peeked.as_ref()
    }

    fn recv(&mut self) -> Option<T> {
        if self.peeked.is_some() {
            self.peeked.take()
        } else {
            match self.rx.try_recv() {
                Err(TryRecvError::Disconnected) => panic!("PTY loop channel closed"),
                res => res.ok(),
            }
        }
    }
}
