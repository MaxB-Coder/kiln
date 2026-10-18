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

    /// PowerShell 7 when it is installed, else Windows PowerShell.
    #[cfg(windows)]
    pub fn powershell() -> Self {
        let program = if find_on_path("pwsh.exe").is_some() {
            "pwsh.exe"
        } else {
            "powershell.exe"
        };
        Self::new(program, ["-NoLogo"])
    }

    /// A WSL session, in the distribution's home folder.
    #[cfg(windows)]
    pub fn wsl(distribution: Option<&str>) -> Self {
        Self::wsl_command(distribution)
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

#[cfg(windows)]
fn find_on_path(exe: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(exe))
        .find(|path| path.is_file())
}
