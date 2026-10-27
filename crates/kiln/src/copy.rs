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

/// The time a command's block shows: none under a second (most commands), so the slow ones
/// stand out. Copies and logs keep the exact time.
pub fn shown_duration(d: Duration) -> Option<String> {
    (d >= Duration::from_secs(1)).then(|| duration_label(d))
}

/// A command line as the timeline shows it: paths in the home folder written `~/…`. For the
/// eye only; copies, logs and Rerun use the command exactly as typed.
pub fn display_command(command: &str) -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    tilde_paths(command, &home)
}

/// `command` with each `home` path that stands as a word (or starts one, before "/") written
/// as "~".
fn tilde_paths(command: &str, home: &str) -> String {
    let home = home.trim_end_matches(['/', '\\']);
    if home.len() < 2 {
        return command.to_string();
    }
    let starts_word = |c: char| c.is_whitespace() || "\"'=:(".contains(c);
    let ends_word = |c: char| c.is_whitespace() || "/\\\"');:".contains(c);
    let mut out = String::with_capacity(command.len());
    let mut rest = command;
    let mut previous: Option<char> = None;
    while let Some(at) = rest.find(home) {
        let before = rest[..at].chars().next_back().or(previous);
        let after = rest[at + home.len()..].chars().next();
        out.push_str(&rest[..at]);
        if before.is_none_or(starts_word) && after.is_none_or(ends_word) {
            out.push('~');
        } else {
            out.push_str(home);
        }
        previous = home.chars().next_back();
        rest = &rest[at + home.len()..];
    }
    out.push_str(rest);
    out
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
    fenced_many(&[block])
}

/// Several commands in one fenced block, in order, a blank line between them.
pub fn fenced_many(blocks: &[&Block]) -> String {
    let bodies: Vec<(String, String)> = blocks.iter().map(|b| (b.output_text(), body(b))).collect();
    // A fence longer than any backtick run in the output, so the output cannot close it.
    let longest = bodies
        .iter()
        .map(|(output, _)| longest_backtick_run(output))
        .max()
        .unwrap_or(0)
        .max(2);
    let fence = "`".repeat(longest + 1);
    let joined = bodies
        .into_iter()
        .map(|(_, body)| body)
        .collect::<Vec<_>>()
        .join("\n");
    format!("{fence}console\n{joined}{fence}\n")
}

/// "folder $ command", the output, and "# exit 0 · 1.2 s", each line ending in a newline.
fn body(block: &Block) -> String {
    let output = block.output_text();
    let mut out = match block.cwd.as_deref().map(short_folder) {
        Some(folder) => format!("{folder} $ {}\n", block.command),
        None => format!("$ {}\n", block.command),
    };
    if !output.is_empty() {
        out.push_str(&output);
        out.push('\n');
    }
    let exit = match block.exit_code {
        Some(code) => format!("exit {code}"),
        None => "exit ?".into(),
    };
    out.push_str(&format!("# {exit} · {}\n", duration_label(block.duration)));
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
    fn only_commands_of_a_second_or_more_show_their_time() {
        assert_eq!(shown_duration(Duration::from_millis(312)), None);
        assert_eq!(shown_duration(Duration::from_millis(999)), None);
        assert_eq!(
            shown_duration(Duration::from_millis(1000)).as_deref(),
            Some("1.00 s")
        );
        assert_eq!(
            shown_duration(Duration::from_millis(2310)).as_deref(),
            Some("2.31 s")
        );
        // The copy keeps the exact time either way.
        let mut quick = block("true", &[], 0);
        quick.duration = Duration::from_millis(312);
        assert!(fenced(&quick).contains("312 ms"), "{}", fenced(&quick));
    }

    #[test]
    fn home_paths_show_as_a_tilde_and_copy_as_typed() {
        let home = "/Users/max";
        for (typed, shown) in [
            ("ls /Users/max/code", "ls ~/code"),
            ("cd /Users/max", "cd ~"),
            ("cp /Users/max/a.txt '/Users/max/b c'", "cp ~/a.txt '~/b c'"),
            (
                "export P=/Users/max/bin:/usr/bin",
                "export P=~/bin:/usr/bin",
            ),
            // Not the home folder: another user's, or a longer name.
            ("ls /Users/maxine/x", "ls /Users/maxine/x"),
            ("ls /tmp/Users/max", "ls /tmp/Users/max"),
            ("echo plain", "echo plain"),
        ] {
            assert_eq!(tilde_paths(typed, home), shown, "{typed}");
        }
        // Display only: the copy has the command as typed.
        let typed = format!("ls {}/code", std::env::var("HOME").unwrap_or_default());
        let copy = fenced(&block(&typed, &["x"], 0));
        assert!(copy.contains(&typed), "{copy}");
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
    fn several_commands_share_one_block_in_order() {
        let text = fenced_many(&[
            &block("ls src", &["auth/  app.ts"], 0),
            &block("npm test", &["FAIL auth.test.ts"], 1),
        ]);
        assert_eq!(
            text,
            "```console\n/tmp/project $ ls src\nauth/  app.ts\n# exit 0 · 2.31 s\n\n\
             /tmp/project $ npm test\nFAIL auth.test.ts\n# exit 1 · 2.31 s\n```\n"
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
