//! The pills in each pane's header: the project, the git branch with its change count, the
//! runtimes the project uses, and the WSL distribution. Kiln draws them itself, so they look the
//! same on every system and shell.

use gpui::{AnyElement, IntoElement, ParentElement, SharedString, Styled, div, px};

use crate::grid_paint::FONT_FAMILY;
use crate::naming;
use crate::theme::{self, palette};

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
    /// The colour chosen for the project (`None`: the accent).
    pub project_color: Option<u32>,
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
    let root = match (git, cwd) {
        (Some(git), _) => Some(git.root.as_str()),
        (None, Some(cwd)) if !cwd.is_empty() => Some(cwd),
        _ => None,
    };
    let project = root.map(|root| naming::project_name(Some(root)));
    let project_color =
        root.and_then(|root| crate::state::chosen_project_color(root, &crate::config::current()));
    let branch = git.map(|git| {
        (
            git.branch.clone().unwrap_or_else(|| "detached".into()),
            git.changes,
        )
    });
    Pills {
        project,
        project_color,
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

/// The branch pill's text (#d5c4a1 in the default palette).
fn branch_text(p: &theme::Palette) -> u32 {
    theme::blend(p.fg, p.fg_dim, 0.5)
}

/// The pills as a row, sized for the header (about 18 px high at the 11 pt UI size).
pub fn render(pills: &Pills) -> AnyElement {
    let p = palette();
    let pill = |bg: u32, fg: u32| {
        div()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .h(px(18.))
            .px(px(9.))
            .rounded_full()
            .bg(theme::hsla(bg))
            .text_color(theme::hsla(fg))
    };
    let mut row = div()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.))
        .font_family(FONT_FAMILY)
        .text_size(px(10.5))
        .font_weight(gpui::FontWeight::BOLD);
    if let Some(project) = &pills.project {
        let bg = pills.project_color.unwrap_or(p.accent);
        row = row.child(pill(bg, p.bar).child(SharedString::from(project.clone())));
    }
    if let Some((branch, changes)) = &pills.branch {
        let mut branch_pill =
            pill(p.bg1, branch_text(&p)).child(SharedString::from(format!("\u{e0a0} {branch}")));
        if *changes > 0 {
            branch_pill = branch_pill.child(
                div()
                    .text_color(theme::hsla(p.accent))
                    .child(SharedString::from(format!("✱{changes}"))),
            );
        }
        row = row.child(branch_pill);
    }
    for runtime in pills.runtimes.iter().chain(&pills.wsl) {
        row = row.child(pill(p.raised, p.ansi[7]).child(SharedString::from(runtime.clone())));
    }
    row.into_any_element()
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
