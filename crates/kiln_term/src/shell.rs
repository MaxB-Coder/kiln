//! Which program a session runs.

use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl ShellCommand {
    pub fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    /// The user's login shell on macOS and Linux; PowerShell on Windows.
    pub fn platform_default() -> Self {
        #[cfg(unix)]
        {
            Self::new(unix_login_shell(), ["-l"])
        }
        #[cfg(windows)]
        {
            Self::powershell()
        }
    }

    /// PowerShell 7 when it is installed, else Windows PowerShell, else cmd.exe: by its full
    /// path (`windows_shell`), so a PATH without PowerShell's folder still finds it.
    #[cfg(windows)]
    pub fn powershell() -> Self {
        let env = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
        windows_shell(env, |path| std::path::Path::new(path).is_file())
    }

    /// A WSL session, in the distribution's home folder.
    #[cfg(windows)]
    pub fn wsl(distribution: Option<&str>) -> Self {
        let mut command = Self::wsl_command(distribution);
        let wsl = format!("{}\\wsl.exe", system_dir(|name| std::env::var(name).ok()));
        if std::path::Path::new(&wsl).is_file() {
            command.program = wsl;
        }
        command
    }

    /// The WSL command line, on any platform (for tests).
    #[doc(hidden)]
    pub fn wsl_for_test(distribution: &str) -> Self {
        Self::wsl_command(Some(distribution))
    }

    fn wsl_command(distribution: Option<&str>) -> Self {
        let mut args = Vec::new();
        if let Some(distribution) = distribution {
            args.extend(["--distribution".to_string(), distribution.to_string()]);
        }
        args.extend(["--cd".to_string(), "~".to_string()]);
        Self {
            program: "wsl.exe".into(),
            args,
        }
    }

    /// Short label for the window title and notes.
    pub fn label(&self) -> String {
        PathBuf::from(&self.program)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.program.clone())
    }
}

#[cfg(unix)]
fn unix_login_shell() -> String {
    if let Ok(shell) = std::env::var("SHELL")
        && !shell.is_empty()
    {
        return shell;
    }
    // Apps started from Finder or a desktop launcher may have no $SHELL; ask the user database.
    // SAFETY: getpwuid returns a pointer into static storage or null; we copy the string out
    // before any other call could overwrite it.
    unsafe {
        let pw = libc::getpwuid(libc::getuid());
        if !pw.is_null() && !(*pw).pw_shell.is_null() {
            let shell = std::ffi::CStr::from_ptr((*pw).pw_shell)
                .to_string_lossy()
                .into_owned();
            if !shell.is_empty() {
                return shell;
            }
        }
    }
    if cfg!(target_os = "macos") {
        "/bin/zsh".into()
    } else {
        "/bin/sh".into()
    }
}

/// Where Windows' shell is looked for, in order: pwsh.exe on PATH, PowerShell 7's own folder
/// and its Microsoft Store alias, Windows PowerShell in System32, then `%ComSpec%` (cmd.exe).
/// `env` reads an environment variable. Windows paths, on any platform (for tests).
pub fn windows_shell_candidates(env: impl Fn(&str) -> Option<String>) -> Vec<String> {
    let dir = |d: &str| {
        d.trim()
            .trim_matches('"')
            .trim_end_matches('\\')
            .to_string()
    };
    let mut out: Vec<String> = env("PATH")
        .unwrap_or_default()
        .split(';')
        .map(dir)
        .filter(|d| !d.is_empty())
        .map(|d| format!("{d}\\pwsh.exe"))
        .collect();
    if let Some(programs) = env("ProgramFiles") {
        out.push(format!("{}\\PowerShell\\7\\pwsh.exe", dir(&programs)));
    }
    if let Some(local) = env("LOCALAPPDATA") {
        out.push(format!("{}\\Microsoft\\WindowsApps\\pwsh.exe", dir(&local)));
    }
    let system = system_dir(&env);
    out.push(format!("{system}\\WindowsPowerShell\\v1.0\\powershell.exe"));
    out.push(env("ComSpec").unwrap_or_else(|| format!("{system}\\cmd.exe")));
    out
}

/// The first candidate that exists (`exists`), with its arguments; Windows PowerShell's path
/// when none does, so the error names it.
pub fn windows_shell(
    env: impl Fn(&str) -> Option<String>,
    exists: impl Fn(&str) -> bool,
) -> ShellCommand {
    let candidates = windows_shell_candidates(&env);
    let program = candidates
        .iter()
        .find(|c| exists(c))
        .cloned()
        .unwrap_or_else(|| {
            format!(
                "{}\\WindowsPowerShell\\v1.0\\powershell.exe",
                system_dir(&env)
            )
        });
    let cmd = PathBuf::from(program.replace('\\', "/"))
        .file_stem()
        .is_some_and(|s| s.eq_ignore_ascii_case("cmd"));
    if cmd {
        ShellCommand::new(program, Vec::<String>::new())
    } else {
        ShellCommand::new(program, ["-NoLogo"])
    }
}

/// What Kiln tries for Windows' shell, for the error when none starts.
pub fn windows_shell_tried() -> String {
    "pwsh.exe on PATH, PowerShell 7 (Program Files and the Microsoft Store), Windows PowerShell \
     (System32\\WindowsPowerShell\\v1.0\\powershell.exe) and cmd.exe"
        .into()
}

/// `%SystemRoot%\System32` (or `%windir%`'s); the system's own answer when neither is set.
fn system_dir(env: impl Fn(&str) -> Option<String>) -> String {
    if let Some(root) = env("SystemRoot").or_else(|| env("windir")) {
        return format!("{}\\System32", root.trim_end_matches('\\'));
    }
    system_directory().unwrap_or_else(|| "C:\\Windows\\System32".into())
}

#[cfg(windows)]
fn system_directory() -> Option<String> {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = [0u16; 512];
    // SAFETY: the buffer is valid for its length; the call writes at most that many units.
    let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    (len > 0 && len < buffer.len()).then(|| String::from_utf16_lossy(&buffer[..len]))
}

#[cfg(not(windows))]
fn system_directory() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| v.to_string())
        }
    }

    const VARS: &[(&str, &str)] = &[
        ("PATH", "C:\\Tools;\"C:\\Program Files\\Git\\cmd\\\";;"),
        ("ProgramFiles", "C:\\Program Files"),
        ("LOCALAPPDATA", "C:\\Users\\max\\AppData\\Local"),
        ("SystemRoot", "D:\\Windows"),
        ("ComSpec", "D:\\Windows\\system32\\cmd.exe"),
    ];

    #[test]
    fn candidates_in_order() {
        assert_eq!(
            windows_shell_candidates(env(VARS)),
            [
                "C:\\Tools\\pwsh.exe",
                "C:\\Program Files\\Git\\cmd\\pwsh.exe",
                "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
                "C:\\Users\\max\\AppData\\Local\\Microsoft\\WindowsApps\\pwsh.exe",
                "D:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
                "D:\\Windows\\system32\\cmd.exe",
            ]
        );
    }

    #[test]
    fn the_first_that_exists_by_its_full_path() {
        // No PowerShell on PATH (the reported case): Windows PowerShell from System32.
        let only = |present: &'static [&'static str]| move |p: &str| present.contains(&p);
        let shell = windows_shell(
            env(VARS),
            only(&["D:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"]),
        );
        assert_eq!(
            shell,
            ShellCommand::new(
                "D:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
                ["-NoLogo"]
            )
        );
        // PowerShell 7 installed but off PATH.
        let shell = windows_shell(
            env(VARS),
            only(&[
                "C:\\Program Files\\PowerShell\\7\\pwsh.exe",
                "D:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe",
            ]),
        );
        assert_eq!(shell.program, "C:\\Program Files\\PowerShell\\7\\pwsh.exe");
        // Only cmd.exe: no PowerShell arguments.
        let shell = windows_shell(env(VARS), only(&["D:\\Windows\\system32\\cmd.exe"]));
        assert_eq!(
            shell,
            ShellCommand::new("D:\\Windows\\system32\\cmd.exe", Vec::<String>::new())
        );
        // Nothing found: Windows PowerShell's path, so the error names it.
        assert_eq!(
            windows_shell(env(VARS), |_| false).program,
            "D:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"
        );
        // No SystemRoot either: still never a bare name.
        let bare = windows_shell(env(&[]), |_| false).program;
        // GetSystemDirectoryW may say "system32": Windows paths ignore case.
        assert!(
            bare.to_ascii_lowercase()
                .ends_with("\\system32\\windowspowershell\\v1.0\\powershell.exe"),
            "{bare}"
        );
    }
}
