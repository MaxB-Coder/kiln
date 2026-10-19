//! "Open log": a block's full output as a plain-text file in Kiln's private log folder.

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use kiln_term::Block;

use crate::copy;

/// How many log files to keep; older ones are removed when a new one is written.
const KEEP: usize = 200;

pub fn log_dir() -> Option<PathBuf> {
    crate::state::data_dir().map(|d| d.join("logs"))
}

/// Writes the block to a new log file and returns its path.
pub fn write_log(block: &Block) -> io::Result<PathBuf> {
    let dir = log_dir().ok_or_else(|| io::Error::other("no home folder"))?;
    fs::create_dir_all(&dir)?;
    let started = block
        .started_at
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let slug: String = block
        .command
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .take(4)
        .collect::<Vec<_>>()
        .join("-");
    let path = dir.join(format!("{}-{}-{}.log", utc_stamp(started), block.id, slug));

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Output can hold secrets: only the user may read the logs.
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    writeln!(file, "# {}", block.command)?;
    if let Some(cwd) = &block.cwd {
        writeln!(file, "# in {}", copy::short_folder(cwd))?;
    }
    let exit = block.exit_code.map_or("?".to_string(), |c| c.to_string());
    writeln!(
        file,
        "# exit {exit} · {} · started {} UTC",
        copy::duration_label(block.duration),
        utc_stamp(started)
    )?;
    writeln!(file)?;
    file.write_all(block.output_text().as_bytes())?;
    writeln!(file)?;
    prune(&dir);
    Ok(path)
}

fn prune(dir: &std::path::Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "log"))
        .collect();
    if logs.len() <= KEEP {
        return;
    }
    logs.sort();
    for old in &logs[..logs.len() - KEEP] {
        let _ = fs::remove_file(old);
    }
}

/// `20261018-153045` for a Unix time, in UTC.
fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}{m:02}{d:02}-{:02}{:02}{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// The command that shows a log file in a pane.
pub fn viewer_command(path: &std::path::Path) -> kiln_term::ShellCommand {
    let path = path.to_string_lossy().into_owned();
    if cfg!(windows) {
        let quoted = path.replace('\'', "''");
        kiln_term::ShellCommand::new(
            "powershell.exe",
            [
                "-NoLogo",
                "-NoProfile",
                "-Command",
                &format!("Get-Content -LiteralPath '{quoted}' | more"),
            ],
        )
    } else {
        // -R keeps colours, +G starts at the end like a terminal, -X leaves it on screen.
        kiln_term::ShellCommand::new("less", ["-R", "+G", path.as_str()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_stamp_formats_dates() {
        assert_eq!(utc_stamp(0), "19700101-000000");
        assert_eq!(utc_stamp(1_792_337_175), "20261018-152615");
    }
}
