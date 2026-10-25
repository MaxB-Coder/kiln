//! Switching the prompt setting while shells are open: each picks it up at its next prompt.
//! (A test binary of its own: the mode file is per process, and the other prompt tests expect
//! Kiln's prompt throughout.)

use std::path::Path;
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

use kiln_term::integration::{self, MinimalPrompt};
use kiln_term::{GridSize, Mark, Session, SessionEvent, SessionOptions, ShellCommand};

const SIZE: GridSize = GridSize {
    columns: 80,
    lines: 24,
    cell_width: 8,
    cell_height: 16,
};

/// Starts with Kiln's prompt, switches to the shell's own and back, pressing Enter between;
/// returns each prompt's line start and the mode the shell reported for it.
/// One shell at a time: they all use (and the switch tests change) this process's mode file.
static SHELLS: Mutex<()> = Mutex::new(());

fn switch(program: &str, args: &[&str]) -> Option<Vec<(String, Option<bool>)>> {
    let _one = SHELLS.lock().unwrap_or_else(|e| e.into_inner());
    use kiln_term::engine::grid::Dimensions as _;
    use kiln_term::engine::index::{Column, Line};

    if !Path::new(program).exists() {
        eprintln!("skipping: {program} not installed");
        return None;
    }
    let home = std::env::temp_dir().join(format!(
        "kiln-switch-home-{}-{}",
        std::process::id(),
        program.len()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let mut env = SessionOptions::default().env;
    env.insert("HOME".into(), home.to_string_lossy().into_owned());
    env.insert("PS1".into(), "$ ".into());
    env.insert("skip_global_compinit".into(), "1".into());
    let options = SessionOptions {
        shell: ShellCommand::new(program, args.iter().copied()),
        working_directory: Some(home),
        env,
        prompt: Some(MinimalPrompt),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    let session = Session::spawn(options, SIZE, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let line = |session: &Session| {
        let term = session.term().lock();
        let grid = term.grid();
        let row = grid.cursor.point.line;
        (0..grid.columns().min(2))
            .map(|c| grid[Line(row.0)][Column(c)].c)
            .collect::<String>()
    };
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut reported = None;
    let mut prompts = Vec::new();
    loop {
        let event = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|_| panic!("{program}: no prompt before timeout"));
        let SessionEvent::Mark(record) = event else {
            continue;
        };
        match record.mark {
            Mark::PromptMode { kiln } => reported = Some(kiln),
            Mark::InputStart => {
                std::thread::sleep(Duration::from_millis(200));
                prompts.push((line(&session), reported));
                match prompts.len() {
                    1 => integration::set_prompt_mode(false).unwrap(),
                    2 => integration::set_prompt_mode(true).unwrap(),
                    _ => return Some(prompts),
                }
                session.write(b"\r".to_vec());
            }
            _ => {}
        }
    }
}

fn check(prompts: Vec<(String, Option<bool>)>) {
    assert_eq!(prompts[0], ("  ".to_string(), Some(true)), "Kiln's prompt");
    // The shell's own prompt (the system's startup files may have set it): not the blanks.
    assert_eq!(prompts[1].1, Some(false), "the shell's own, while open");
    assert!(
        !prompts[1].0.trim().is_empty(),
        "the shell's own prompt shows: {:?}",
        prompts[1]
    );
    assert_eq!(prompts[2], ("  ".to_string(), Some(true)), "Kiln's again");
}

#[test]
fn zsh_switches_while_open() {
    if let Some(prompts) = switch("/bin/zsh", &["-l"]) {
        check(prompts);
    }
}

#[test]
fn bash_switches_while_open() {
    for bash in ["/opt/homebrew/bin/bash", "/usr/bin/bash", "/bin/bash"] {
        if let Some(prompts) = switch(bash, &["-l"]) {
            check(prompts);
            return;
        }
    }
}

#[test]
fn pwsh_switches_while_open() {
    for pwsh in [
        "/usr/local/bin/pwsh",
        "/opt/homebrew/bin/pwsh",
        "/usr/bin/pwsh",
    ] {
        if let Some(prompts) = switch(pwsh, &["-NoLogo", "-NoProfile"]) {
            check(prompts);
            return;
        }
    }
}

/// An empty prompt is told apart from one with something typed (Kiln presses Enter on the
/// empty ones when the setting changes, so they switch at once).
fn empty_prompt(program: &str, args: &[&str]) {
    if !Path::new(program).exists() {
        eprintln!("skipping: {program} not installed");
        return;
    }
    let _one = SHELLS.lock().unwrap_or_else(|e| e.into_inner());
    let home = std::env::temp_dir().join(format!(
        "kiln-empty-home-{}-{}",
        std::process::id(),
        program.len()
    ));
    std::fs::create_dir_all(&home).unwrap();
    let mut env = SessionOptions::default().env;
    env.insert("HOME".into(), home.to_string_lossy().into_owned());
    env.insert("skip_global_compinit".into(), "1".into());
    let options = SessionOptions {
        shell: ShellCommand::new(program, args.iter().copied()),
        working_directory: Some(home),
        env,
        prompt: Some(MinimalPrompt),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    let session = Session::spawn(options, SIZE, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let event = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|_| panic!("{program}: no prompt before timeout"));
        if let SessionEvent::Mark(record) = event
            && record.mark == Mark::InputStart
        {
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(300));
    assert!(session.at_empty_prompt(), "{program}: empty");
    session.write(b"ech".to_vec());
    std::thread::sleep(Duration::from_millis(500));
    assert!(!session.at_empty_prompt(), "{program}: something typed");
}

#[test]
fn zsh_empty_prompt() {
    empty_prompt("/bin/zsh", &["-l"]);
}

#[test]
fn bash_empty_prompt() {
    for bash in ["/opt/homebrew/bin/bash", "/usr/bin/bash", "/bin/bash"] {
        if Path::new(bash).exists() {
            empty_prompt(bash, &["-l"]);
            return;
        }
    }
}

#[test]
fn pwsh_empty_prompt() {
    for pwsh in [
        "/usr/local/bin/pwsh",
        "/opt/homebrew/bin/pwsh",
        "/usr/bin/pwsh",
    ] {
        if Path::new(pwsh).exists() {
            empty_prompt(pwsh, &["-NoLogo", "-NoProfile"]);
            return;
        }
    }
}
