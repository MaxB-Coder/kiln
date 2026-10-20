//! The pills under the prompt in the input box: the project, the git branch with its change
//! count, the runtimes the project uses, and the WSL distribution. Kiln draws them itself, so
//! they look the same on every system and shell.

use crate::naming;

/// What git says about a pane's folder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GitSummary {
    /// The repository's root.
    pub root: String,
    /// `None` when detached.
    pub branch: Option<String>,
    /// Changed, added, deleted and untracked files.
    pub changes: usize,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pills {
    /// "receipts": the repository's (or folder's) name.
    pub project: Option<String>,
    /// "feat/auth" and how many files changed; only inside a repository.
    pub branch: Option<(String, usize)>,
    /// "node 25", "rust 1.98".
    pub runtimes: Vec<String>,
    /// "Ubuntu" (Windows, WSL sessions).
    pub wsl: Option<String>,
}

/// "25.0.0" → "25", "1.98.1" → "1.98", "3.14.0" → "3.14".
pub fn short_version(version: &str) -> String {
    let mut parts = version.split('.');
    let major = parts.next().unwrap_or(version);
    match parts.next() {
        Some(minor) if minor != "0" && !minor.is_empty() => format!("{major}.{minor}"),
        _ => major.to_string(),
    }
}

pub fn pills(
    cwd: Option<&str>,
    git: Option<&GitSummary>,
    runtimes: &[(String, String)],
    wsl: Option<&str>,
) -> Pills {
    let project = match (git, cwd) {
        (Some(git), _) => Some(naming::project_name(Some(&git.root))),
        (None, Some(cwd)) if !cwd.is_empty() => Some(naming::project_name(Some(cwd))),
        _ => None,
    };
    let branch = git.map(|git| {
        (
            git.branch.clone().unwrap_or_else(|| "detached".into()),
            git.changes,
        )
    });
    Pills {
        project,
        branch,
        runtimes: runtimes
            .iter()
            .map(|(name, version)| format!("{name} {}", short_version(version)))
            .collect(),
        wsl: wsl.map(|label| {
            // "WSL · Ubuntu" → "Ubuntu".
            label.rsplit(" · ").next().unwrap_or(label).to_string()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_are_short() {
        assert_eq!(short_version("25.0.0"), "25");
        assert_eq!(short_version("1.98.1"), "1.98");
        assert_eq!(short_version("3.14.0"), "3.14");
        assert_eq!(short_version("21"), "21");
    }

    #[test]
    fn a_repository_with_changes() {
        let git = GitSummary {
            root: "/code/receipts".into(),
            branch: Some("feat/auth".into()),
            changes: 2,
        };
        let p = pills(
            Some("/code/receipts/api"),
            Some(&git),
            &[("node".into(), "25.0.0".into())],
            None,
        );
        assert_eq!(p.project.as_deref(), Some("receipts"));
        assert_eq!(p.branch, Some(("feat/auth".into(), 2)));
        assert_eq!(p.runtimes, ["node 25"]);
        assert_eq!(p.wsl, None);
    }

    #[test]
    fn a_clean_repository_has_no_count() {
        let git = GitSummary {
            root: "/code/kiln".into(),
            branch: Some("main".into()),
            changes: 0,
        };
        let p = pills(Some("/code/kiln"), Some(&git), &[], None);
        assert_eq!(p.branch, Some(("main".into(), 0)));
    }

    #[test]
    fn outside_a_repository_there_is_no_branch() {
        let p = pills(Some("/tmp/scratch"), None, &[], Some("WSL · Ubuntu"));
        assert_eq!(p.project.as_deref(), Some("scratch"));
        assert_eq!(p.branch, None);
        assert_eq!(p.wsl.as_deref(), Some("Ubuntu"));
        assert_eq!(pills(None, None, &[], None), Pills::default());
    }
}
