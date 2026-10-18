//! Runs a real program on a real PTY and checks what the engine and the mark scanner saw.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use kiln_term::engine::event::Event;
use kiln_term::engine::grid::Dimensions;
use kiln_term::engine::index::{Column, Line, Point};
use kiln_term::{GridSize, Mark, Session, SessionEvent, SessionOptions, ShellCommand};

const SIZE: GridSize = GridSize {
    columns: 80,
    lines: 24,
    cell_width: 8,
    cell_height: 16,
};

/// A command that prints a prompt mark, some text, and an exit mark, then exits.
fn printing_command() -> ShellCommand {
    #[cfg(unix)]
    {
        ShellCommand::new(
            "/bin/sh",
            ["-c", r"printf '\033]133;A\007kiln-ok\r\n\033]133;D;3\007'"],
        )
    }
    #[cfg(windows)]
    {
        ShellCommand::new(
            "powershell.exe",
            [
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "[Console]::Write(\"$([char]27)]133;A$([char]7)kiln-ok`r`n$([char]27)]133;D;3$([char]7)\")",
            ],
        )
    }
}

#[test]
fn runs_a_program_and_sees_its_output_and_marks() {
    let (tx, rx) = mpsc::channel();
    let options = SessionOptions {
        shell: printing_command(),
        ..Default::default()
    };
    let session = Session::spawn(options, SIZE, move |event| {
        let _ = tx.send(event);
    })
    .expect("spawn session");

    let mut marks = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let timeout = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(timeout) {
            Ok(SessionEvent::Mark(record)) => marks.push(record),
            Ok(SessionEvent::Term(Event::Exit)) => break,
            Ok(_) => {}
            Err(err) => panic!("no exit before timeout: {err}; marks so far: {marks:?}"),
        }
    }

    let kinds: Vec<_> = marks.iter().map(|m| m.mark.clone()).collect();
    assert!(kinds.contains(&Mark::PromptStart), "marks: {kinds:?}");
    assert!(
        kinds.contains(&Mark::CommandEnd { exit_code: Some(3) }),
        "marks: {kinds:?}"
    );

    // The prompt mark arrived before "kiln-ok" was printed, at the start of its line.
    let prompt = marks.iter().find(|m| m.mark == Mark::PromptStart).unwrap();
    assert_eq!(prompt.column, 0);

    let term = session.term().lock();
    let line = Line(prompt.line);
    let text = term.bounds_to_string(
        Point::new(line, Column(0)),
        Point::new(line, Column(term.columns() - 1)),
    );
    assert!(text.contains("kiln-ok"), "line {line:?} was {text:?}");
}
