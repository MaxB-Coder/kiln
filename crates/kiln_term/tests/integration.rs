//! Runs real shells with Kiln's integration (in an empty HOME, so no user config interferes)
//! and checks the blocks the recorder produces.
#![cfg(unix)]

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use kiln_term::{Block, GridSize, Mark, Session, SessionEvent, SessionOptions, ShellCommand};

const SIZE: GridSize = GridSize {
    columns: 80,
    lines: 24,
    cell_width: 8,
    cell_height: 16,
};

fn run_block(program: &str, args: &[&str], command: &str) -> Option<Block> {
    if !Path::new(program).exists() {
        eprintln!("skipping: {program} not installed");
        return None;
    }
    let home = std::env::temp_dir().join(format!(
        "kiln-home-{}-{}",
        std::process::id(),
        program.len()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let mut env = SessionOptions::default().env;
    env.insert("HOME".into(), home.to_string_lossy().into_owned());
    env.insert("PS1".into(), "$ ".into());
    // Ubuntu's /etc/zsh/zshrc runs compinit, which asks about "insecure directories" in a
    // fresh temporary HOME and blocks the test.
    env.insert("skip_global_compinit".into(), "1".into());
    let options = SessionOptions {
        shell: ShellCommand::new(program, args.iter().copied()),
        working_directory: Some(home.clone()),
        env,
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    let session = Session::spawn(options, SIZE, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    assert!(session.integration_loaded());

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut sent = false;
    let mut seen = Vec::new();
    loop {
        let event = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|_| {
                let text = session
                    .term()
                    .lock()
                    .grid()
                    .display_iter()
                    .map(|c| c.c)
                    .collect::<String>();
                panic!(
                    "{program}: no block before timeout; marks {seen:?}; screen {:?}",
                    text.trim()
                )
            });
        if let SessionEvent::Mark(record) = &event {
            seen.push(record.mark.clone());
        }
        match event {
            // Type once the prompt is fully drawn (input mark), like a person would.
            SessionEvent::Mark(record) if record.mark == Mark::InputStart && !sent => {
                std::thread::sleep(Duration::from_millis(200));
                session.write(format!("{command}\r").into_bytes());
                sent = true;
            }
            SessionEvent::Block(block) if block.startup => {
                // macOS bash 3.2 prints a banner before its first prompt.
                assert!(
                    block.output_text().contains("zsh"),
                    "startup: {:?}",
                    block.output_text()
                );
            }
            SessionEvent::Block(block) => return Some(block),
            _ => {}
        }
    }
}

fn check(block: Block, expect_command: &str, expect_output: &str) {
    assert_eq!(block.command, expect_command);
    assert_eq!(block.exit_code, Some(3), "block: {block:?}");
    assert_eq!(block.output_text(), expect_output);
    assert!(block.failed());
}

const COMMAND: &str = "printf 'first\\nsecond 宽\\n'; (exit 3)";
const OUTPUT: &str = "first\nsecond 宽";

#[test]
fn zsh_block() {
    if let Some(block) = run_block("/bin/zsh", &["-l"], COMMAND) {
        check(block, COMMAND, OUTPUT);
    }
}

#[test]
fn bash5_block() {
    if let Some(block) = run_block("/opt/homebrew/bin/bash", &["-l"], COMMAND) {
        check(block, COMMAND, OUTPUT);
    }
}

#[test]
fn bash3_block_without_output_mark() {
    // macOS ships bash 3.2: no PS0, so the recorder infers the output start.
    if let Some(block) = run_block("/bin/bash", &["-l"], COMMAND) {
        check(block, COMMAND, OUTPUT);
    }
}
