//! Paste-ready copies of a command's output.

use std::time::Duration;

use kiln_term::Block;

/// "312 ms", "2.31 s", "1 min 04 s".
pub fn duration_label(d: Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.2} s", d.as_secs_f64())
    } else {
        let s = d.as_secs();
        format!("{} min {:02} s", s / 60, s % 60)
    }
}

/// The folder with the home directory shortened to `~`.
pub fn short_folder(path: &str) -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    if !home.is_empty() {
        let home = home.replace('\\', "/");
        if path == home {
            return "~".into();
        }
        if let Some(rest) = path.strip_prefix(&format!("{home}/")) {
            return format!("~/{rest}");
        }
    }
    path.to_string()
}

/// A Markdown fenced block: folder and command, the output, then exit code and timing.
///
/// ````text
/// ```console
/// ~/developer/kiln $ cargo test
/// test result: ok. 13 passed
/// # exit 0 · 2.31 s
/// ```
/// ````
pub fn fenced(block: &Block) -> String {
    let output = block.output_text();
    // A fence longer than any backtick run in the output, so the output cannot close it.
    let longest = longest_backtick_run(&output).max(2);
    let fence = "`".repeat(longest + 1);
    let folder = block.cwd.as_deref().map(short_folder);
    let mut out = format!("{fence}console\n");
    match folder {
        Some(folder) => out.push_str(&format!("{folder} $ {}\n", block.command)),
        None => out.push_str(&format!("$ {}\n", block.command)),
    }
    if !output.is_empty() {
        out.push_str(&output);
        out.push('\n');
    }
    let exit = match block.exit_code {
        Some(code) => format!("exit {code}"),
        None => "exit ?".into(),
    };
    out.push_str(&format!("# {exit} · {}\n", duration_label(block.duration)));
    out.push_str(&fence);
    out.push('\n');
    out
}

fn longest_backtick_run(text: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use kiln_term::engine::term::cell::Cell;

    use super::*;

    fn block(command: &str, output: &[&str], exit: i32) -> Block {
        Block {
            id: 1,
            command: command.into(),
            cwd: Some("/tmp/project".into()),
            started_at: SystemTime::now(),
            duration: Duration::from_millis(2310),
            exit_code: Some(exit),
            lines: output
                .iter()
                .map(|l| {
                    l.chars()
                        .map(|c| Cell {
                            c,
                            ..Default::default()
                        })
                        .collect()
                })
                .collect(),
            truncated: false,
            startup: false,
        }
    }

    #[test]
    fn fenced_block_has_folder_command_output_exit_and_time() {
        let text = fenced(&block("cargo test", &["ok", "done"], 1));
        assert_eq!(
            text,
            "```console\n/tmp/project $ cargo test\nok\ndone\n# exit 1 · 2.31 s\n```\n"
        );
    }

    #[test]
    fn fence_outgrows_backticks_in_the_output() {
        let text = fenced(&block(
            "cat README.md",
            &["```rust", "fn main() {}", "```"],
            0,
        ));
        assert!(text.starts_with("````console\n"));
        assert!(text.ends_with("\n````\n"));
    }

    #[test]
    fn durations_read_naturally() {
        assert_eq!(duration_label(Duration::from_millis(312)), "312 ms");
        assert_eq!(duration_label(Duration::from_millis(64_000)), "1 min 04 s");
    }
}
