//! Shell-integration marks: OSC 133 (prompt, command, output, exit code) and OSC 7 (folder).
//!
//! `alacritty_terminal` drops OSC sequences it does not know, so Kiln scans the PTY stream
//! itself, alongside the engine. The read loop stops feeding the engine at each mark, so the
//! cursor position recorded with a mark is exact. Command blocks (phase 1) are built from these.

use alacritty_terminal::vte::{Parser, Perform};

/// A shell-integration mark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    /// `OSC 133 ; A`: the prompt starts.
    PromptStart,
    /// `OSC 133 ; B`: the prompt ends and the user's input starts.
    InputStart,
    /// `OSC 133 ; C`: the command runs; its output starts.
    OutputStart,
    /// `OSC 133 ; D [; exit]`: the command finished.
    CommandEnd { exit_code: Option<i32> },
    /// `OSC 7 ; file://host/path`: the shell's current folder.
    Cwd { host: String, path: String },
}

/// A mark plus where the cursor was when it arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkRecord {
    pub mark: Mark,
    /// Cursor line in screen coordinates (0 is the top of the live screen).
    pub line: i32,
    pub column: usize,
    /// Scrollback length at that moment. `history_size + line` is a stable row index until the
    /// scrollback starts trimming or the grid reflows; phase 1 needs a sturdier anchor.
    pub history_size: usize,
}

/// Finds marks in a byte stream, keeping state across reads.
#[derive(Default)]
pub struct MarkScanner {
    parser: Parser,
    found: Found,
}

impl MarkScanner {
    /// Scans `bytes` up to and including the end of the next mark.
    ///
    /// Returns how many bytes were consumed and the mark, if one ended there. Call again with
    /// the rest of the slice until it is empty.
    pub fn scan(&mut self, bytes: &[u8]) -> (usize, Option<Mark>) {
        let consumed = self.parser.advance_until_terminated(&mut self.found, bytes);
        (consumed, self.found.0.take())
    }
}

#[derive(Default)]
struct Found(Option<Mark>);

impl Perform for Found {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        self.0 = parse_osc(params);
    }

    fn terminated(&self) -> bool {
        self.0.is_some()
    }
}

fn parse_osc(params: &[&[u8]]) -> Option<Mark> {
    match params {
        [b"133", kind, rest @ ..] => match *kind {
            b"A" => Some(Mark::PromptStart),
            b"B" => Some(Mark::InputStart),
            b"C" => Some(Mark::OutputStart),
            b"D" => {
                let exit_code = rest
                    .first()
                    .and_then(|code| std::str::from_utf8(code).ok()?.parse().ok());
                Some(Mark::CommandEnd { exit_code })
            }
            _ => None,
        },
        // A URI may contain ';', which the parser splits on.
        [b"7", uri @ ..] if !uri.is_empty() => {
            let uri = uri
                .iter()
                .map(|p| String::from_utf8_lossy(p))
                .collect::<Vec<_>>()
                .join(";");
            parse_file_uri(&uri)
        }
        _ => None,
    }
}

fn parse_file_uri(uri: &str) -> Option<Mark> {
    let rest = uri.strip_prefix("file://")?;
    let slash = rest.find('/')?;
    let (host, path) = rest.split_at(slash);
    Some(Mark::Cwd {
        host: host.to_string(),
        path: percent_decode(path),
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_all(input: &[u8]) -> Vec<(usize, Mark)> {
        let mut scanner = MarkScanner::default();
        let mut out = Vec::new();
        let mut offset = 0;
        while offset < input.len() {
            let (n, mark) = scanner.scan(&input[offset..]);
            offset += n;
            if let Some(mark) = mark {
                out.push((offset, mark));
            }
        }
        out
    }

    #[test]
    fn finds_prompt_and_command_marks_with_offsets() {
        let input = b"\x1b]133;A\x07$ \x1b]133;B\x07ls\r\n\x1b]133;C\x07out\r\n\x1b]133;D;2\x1b\\";
        let marks = scan_all(input);
        assert_eq!(
            marks.iter().map(|(_, m)| m.clone()).collect::<Vec<_>>(),
            vec![
                Mark::PromptStart,
                Mark::InputStart,
                Mark::OutputStart,
                Mark::CommandEnd { exit_code: Some(2) },
            ]
        );
        // Each mark ends right after its terminator (BEL), so the engine can be fed up to there.
        assert_eq!(marks[0].0, b"\x1b]133;A\x07".len());
    }

    #[test]
    fn finds_marks_split_across_reads() {
        let mut scanner = MarkScanner::default();
        assert_eq!(scanner.scan(b"hello \x1b]13"), (10, None));
        assert_eq!(
            scanner.scan(b"3;D\x07rest"),
            (4, Some(Mark::CommandEnd { exit_code: None }))
        );
    }

    #[test]
    fn parses_osc7_folder() {
        let marks = scan_all(b"\x1b]7;file://mac.local/Users/max/My%20Code\x07");
        assert_eq!(
            marks[0].1,
            Mark::Cwd {
                host: "mac.local".into(),
                path: "/Users/max/My Code".into()
            }
        );
    }

    #[test]
    fn ignores_other_osc() {
        assert!(scan_all(b"\x1b]0;title\x07\x1b]8;;http://x\x07link\x1b]8;;\x07").is_empty());
    }
}
