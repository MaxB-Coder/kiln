//! Runs Windows PowerShell with Kiln's integration on ConPTY and checks the recorded block.
#![cfg(windows)]

use std::sync::mpsc;
use std::time::{Duration, Instant};

use kiln_term::{GridSize, Mark, Session, SessionEvent, SessionOptions, ShellCommand};

const SIZE: GridSize = GridSize {
    columns: 100,
    lines: 30,
    cell_width: 8,
    cell_height: 16,
};

#[test]
fn powershell_block() {
    let options = SessionOptions {
        shell: ShellCommand::new("powershell.exe", ["-NoLogo", "-NoProfile"]),
        ..Default::default()
    };
    let (tx, rx) = mpsc::channel();
    let session = Session::spawn(options, SIZE, move |e| {
        let _ = tx.send(e);
    })
    .unwrap();
    assert!(session.integration_loaded());

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut sent = false;
    let block = loop {
        let event = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("no block before timeout");
        match event {
            SessionEvent::Mark(record) if record.mark == Mark::PromptStart && !sent => {
                session.write(b"Write-Output first second; cmd /c exit 3\r".to_vec());
                sent = true;
            }
            SessionEvent::Block(block) => break block,
            _ => {}
        }
    };
    assert_eq!(block.command, "Write-Output first second; cmd /c exit 3");
    assert_eq!(block.exit_code, Some(3));
    assert_eq!(block.output_text(), "first\nsecond");
}
