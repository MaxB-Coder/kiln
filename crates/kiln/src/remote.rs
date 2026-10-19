//! Which machine a pane is connected to, read from the running command (`ssh`, `mosh`), for the
//! remote and production tints.

use std::path::Path;

use crate::config::{TintConfig, host_matches};
use crate::theme::{self, Palette};

/// How a pane connected to another machine is marked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    /// Any other machine: a faint blue.
    Remote,
    /// A host matching the production patterns: a faint red.
    Production,
}

impl Tint {
    pub fn for_host(host: &str, config: &TintConfig) -> Option<Tint> {
        if host_matches(&config.production, host) {
            Some(Tint::Production)
        } else if config.remote {
            Some(Tint::Remote)
        } else {
            None
        }
    }

    /// The pane background with the tint mixed in.
    pub fn background(self, p: &Palette) -> u32 {
        match self {
            Tint::Remote => theme::blend(p.bg, p.ansi[4], 0.10),
            Tint::Production => theme::blend(p.bg, p.ansi[1], 0.13),
        }
    }

    /// Colour of the corner label.
    pub fn label_color(self, p: &Palette) -> u32 {
        match self {
            Tint::Remote => p.ansi[12],
            Tint::Production => p.failed,
        }
    }

    pub fn label(self, host: &str) -> String {
        match self {
            Tint::Remote => format!("ssh · {host}"),
            Tint::Production => format!("production · {host}"),
        }
    }
}

/// ssh options that take a value (`ssh -p 22`, `ssh -p22`).
const SSH_VALUE_OPTIONS: &str = "BbcDEeFIiJLlmOoPpRSWw";
/// mosh long options that take a value when written without `=`.
const MOSH_VALUE_OPTIONS: &[&str] = &[
    "--ssh",
    "--port",
    "--predict",
    "--family",
    "--server",
    "--client",
    "--bind-server",
    "--experimental-remote-ip",
];

/// The host an ssh or mosh command line connects to:
/// `ssh -p 2222 deploy@db.prod.example.com uptime` → `db.prod.example.com`.
pub fn remote_host(command: &str) -> Option<String> {
    let mut words = shell_words(command).into_iter().peekable();
    // Skip `VAR=value` prefixes and a few wrappers.
    while let Some(word) = words.peek() {
        if word.contains('=') && !word.starts_with('-')
            || ["exec", "command", "nohup"].contains(&word.as_str())
        {
            words.next();
        } else {
            break;
        }
    }
    let program = words.next()?;
    let program = Path::new(&program)
        .file_name()?
        .to_string_lossy()
        .to_ascii_lowercase();
    let program = program.strip_suffix(".exe").unwrap_or(&program).to_string();
    let destination = match program.as_str() {
        "ssh" | "autossh" => {
            let mut destination = None;
            while let Some(word) = words.next() {
                if word == "--" {
                    destination = words.next();
                    break;
                }
                if let Some(flags) = word.strip_prefix('-')
                    && !flags.is_empty()
                {
                    for (i, flag) in flags.char_indices() {
                        if SSH_VALUE_OPTIONS.contains(flag) {
                            if i + flag.len_utf8() == flags.len() {
                                words.next();
                            }
                            break;
                        }
                    }
                    continue;
                }
                destination = Some(word);
                break;
            }
            destination
        }
        "mosh" => {
            let mut destination = None;
            while let Some(word) = words.next() {
                if word == "--" {
                    destination = words.next();
                    break;
                }
                if word.starts_with("--") {
                    if !word.contains('=') && MOSH_VALUE_OPTIONS.contains(&word.as_str()) {
                        words.next();
                    }
                    continue;
                }
                if word == "-p" {
                    words.next();
                    continue;
                }
                if word.starts_with('-') {
                    continue;
                }
                destination = Some(word);
                break;
            }
            destination
        }
        _ => None,
    }?;
    host_of(&destination)
}

/// `user@host`, `ssh://user@host:2222` → `host`.
fn host_of(destination: &str) -> Option<String> {
    let rest = destination.strip_prefix("ssh://").unwrap_or(destination);
    let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
    let host = if let Some(bracketed) = rest.strip_prefix('[') {
        // An IPv6 address: [::1] or [::1]:2222.
        bracketed.split(']').next().unwrap_or(bracketed)
    } else if destination.starts_with("ssh://") {
        rest.split(['/', ':']).next().unwrap_or(rest)
    } else {
        rest
    };
    (!host.is_empty()).then(|| host.to_string())
}

/// Splits a command line into words, honouring simple quotes and backslashes (enough for
/// reading an ssh destination; not a full shell parser).
fn shell_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut in_word = false;
    let mut quote: Option<char> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                in_word = true;
            }
            (None, '\\') => {
                if let Some(next) = chars.next() {
                    word.push(next);
                    in_word = true;
                }
            }
            (None, c) if c.is_whitespace() => {
                if in_word {
                    words.push(std::mem::take(&mut word));
                    in_word = false;
                }
            }
            // A command list or pipe ends the first command.
            (None, ';' | '|' | '&') => break,
            (None, c) => {
                word.push(c);
                in_word = true;
            }
        }
    }
    if in_word {
        words.push(word);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_hosts_get_the_red_tint() {
        let config = TintConfig::default();
        assert_eq!(
            Tint::for_host("db.prod.example.com", &config),
            Some(Tint::Production)
        );
        assert_eq!(Tint::for_host("build-box", &config), Some(Tint::Remote));
        let off = TintConfig {
            remote: false,
            ..TintConfig::default()
        };
        assert_eq!(Tint::for_host("build-box", &off), None);
        assert_eq!(Tint::for_host("web-prod", &off), Some(Tint::Production));
        assert_eq!(theme::blend(0x000000, 0xffffff, 0.5), 0x808080);
    }

    #[test]
    fn reads_ssh_destinations() {
        let host = |c: &str| remote_host(c);
        assert_eq!(host("ssh db1").as_deref(), Some("db1"));
        assert_eq!(
            host("ssh -p 2222 deploy@db.prod.example.com uptime").as_deref(),
            Some("db.prod.example.com")
        );
        assert_eq!(host("ssh -vp22 -i ~/.ssh/k me@box").as_deref(), Some("box"));
        assert_eq!(
            host("ssh -o 'ProxyJump bastion' web-prod").as_deref(),
            Some("web-prod")
        );
        assert_eq!(host("ssh ssh://me@[::1]:2222").as_deref(), Some("::1"));
        assert_eq!(
            host("TERM=xterm /usr/bin/ssh -A host").as_deref(),
            Some("host")
        );
        assert_eq!(host("ssh.exe me@winbox").as_deref(), Some("winbox"));
        assert_eq!(
            host("mosh --ssh 'ssh -p 2' me@vps -- tmux").as_deref(),
            Some("vps")
        );
        assert_eq!(host("mosh --port=60001 vps").as_deref(), Some("vps"));
        assert_eq!(host("sshd -D"), None);
        assert_eq!(host("ssh -V"), None);
        assert_eq!(host("echo ssh host"), None);
        assert_eq!(host("ssh"), None);
    }
}
