//! Short names for projects and sessions, used in the sidebar, the palette and the header:
//! the project is its repository or folder name ("acme-web", "~" for home), and a session is the
//! subfolder it is in, else the program it runs, else the project's name. Repeats get a number.

use std::path::Path;

/// What a session's label is made from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionFacts {
    /// The project root (resolved), or `None` when the shell has not reported a folder.
    pub project: Option<String>,
    /// The session's folder (resolved the same way as `project`).
    pub cwd: Option<String>,
    /// The running program ("node", or the host for ssh).
    pub program: Option<String>,
    /// The shell's name, for sessions without a folder ("fish").
    pub shell: String,
}

fn home() -> String {
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default()
}

/// "kiln" for a repository, "~" for the home folder, "other" when there is no folder.
pub fn project_name(root: Option<&str>) -> String {
    let Some(root) = root.filter(|r| !r.is_empty()) else {
        return "other".into();
    };
    let home = home();
    if !home.is_empty() && same(root, &home) {
        return "~".into();
    }
    Path::new(root)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string())
}

fn same(a: &str, b: &str) -> bool {
    let trim = |s: &str| s.trim_end_matches(['/', '\\']).to_string();
    trim(a) == trim(b)
}

/// The label alone, before repeats are numbered.
fn base_label(facts: &SessionFacts) -> String {
    if let (Some(project), Some(cwd)) = (&facts.project, &facts.cwd)
        && let Ok(rel) = Path::new(cwd).strip_prefix(project)
        && !rel.as_os_str().is_empty()
    {
        return rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/");
    }
    if let Some(program) = facts.program.as_ref().filter(|p| !p.is_empty()) {
        return program.clone();
    }
    if facts.project.is_some() {
        return project_name(facts.project.as_deref());
    }
    facts.shell.clone()
}

/// Labels for sessions in sidebar order. Within a project, a repeated label gets a number
/// from the second one on: "api", "api 2".
pub fn session_labels(sessions: &[SessionFacts]) -> Vec<String> {
    let mut seen: Vec<(Option<String>, String, usize)> = Vec::new();
    sessions
        .iter()
        .map(|facts| {
            let label = base_label(facts);
            match seen
                .iter_mut()
                .find(|(project, l, _)| *project == facts.project && *l == label)
            {
                Some((_, _, count)) => {
                    *count += 1;
                    format!("{label} {count}")
                }
                None => {
                    seen.push((facts.project.clone(), label.clone(), 1));
                    label
                }
            }
        })
        .collect()
}

/// "acme-web › api"; just "acme-web" (or "acme-web 2") when the session is named after its
/// project.
pub fn breadcrumb(project: Option<&str>, label: &str) -> String {
    let name = project_name(project);
    let numbered = label
        .strip_prefix(name.as_str())
        .and_then(|rest| rest.strip_prefix(' '))
        .is_some_and(|n| n.parse::<usize>().is_ok());
    if project.is_none() || name == label || numbered {
        label.to_string()
    } else {
        format!("{name} › {label}")
    }
}

/// The branch checked out in `root`, read from `.git/HEAD` (no git process): "main", or the
/// short commit when detached. Works for worktrees, whose `.git` is a file.
pub fn git_branch(root: &Path) -> Option<String> {
    let dot_git = root.join(".git");
    let git_dir = if dot_git.is_file() {
        let text = std::fs::read_to_string(&dot_git).ok()?;
        let dir = text.trim().strip_prefix("gitdir:")?.trim().to_string();
        let dir = Path::new(&dir);
        if dir.is_absolute() {
            dir.to_path_buf()
        } else {
            root.join(dir)
        }
    } else {
        dot_git
    };
    let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref:") {
        Some(reference) => {
            let reference = reference.trim();
            Some(
                reference
                    .strip_prefix("refs/heads/")
                    .unwrap_or(reference)
                    .to_string(),
            )
        }
        None => (head.len() >= 7).then(|| head[..7].to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts(project: Option<&str>, cwd: Option<&str>, program: Option<&str>) -> SessionFacts {
        SessionFacts {
            project: project.map(str::to_string),
            cwd: cwd.map(str::to_string),
            program: program.map(str::to_string),
            shell: "zsh".into(),
        }
    }

    #[test]
    fn labels_follow_folder_then_program_then_project() {
        let labels = session_labels(&[
            facts(
                Some("/code/acme-web"),
                Some("/code/acme-web/api"),
                Some("node"),
            ),
            facts(Some("/code/acme-web"), Some("/code/acme-web"), Some("node")),
            facts(Some("/code/acme-web"), Some("/code/acme-web"), None),
            facts(
                Some("/code/acme-web"),
                Some("/code/acme-web/services/web"),
                None,
            ),
            facts(None, None, None),
        ]);
        assert_eq!(labels, ["api", "node", "acme-web", "services/web", "zsh"]);
    }

    #[test]
    fn repeats_are_numbered_within_a_project() {
        let labels = session_labels(&[
            facts(Some("/code/a"), Some("/code/a/api"), None),
            facts(Some("/code/a"), Some("/code/a/api"), None),
            facts(Some("/code/b"), Some("/code/b/api"), None),
            facts(Some("/code/a"), Some("/code/a/api"), None),
        ]);
        assert_eq!(labels, ["api", "api 2", "api", "api 3"]);
    }

    #[test]
    fn projects_and_breadcrumbs() {
        assert_eq!(project_name(Some("/code/kiln")), "kiln");
        assert_eq!(project_name(Some(&home())), "~");
        assert_eq!(project_name(None), "other");
        assert_eq!(breadcrumb(Some("/code/acme-web"), "api"), "acme-web › api");
        assert_eq!(breadcrumb(Some("/code/acme-web"), "acme-web"), "acme-web");
        assert_eq!(
            breadcrumb(Some("/code/acme-web"), "acme-web 2"),
            "acme-web 2"
        );
        assert_eq!(
            breadcrumb(Some("/code/acme-web"), "api 2"),
            "acme-web › api 2"
        );
        assert_eq!(breadcrumb(Some(&home()), "~"), "~");
        assert_eq!(breadcrumb(None, "fish"), "fish");
    }

    #[test]
    fn reads_branches_from_head() {
        let dir = std::env::temp_dir().join(format!("kiln-head-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git/HEAD"), "ref: refs/heads/feature/rail\n").unwrap();
        assert_eq!(git_branch(&dir).as_deref(), Some("feature/rail"));
        std::fs::write(dir.join(".git/HEAD"), "4c3f988aa1b2c3d4e5f6\n").unwrap();
        assert_eq!(git_branch(&dir).as_deref(), Some("4c3f988"));
        let _ = std::fs::remove_dir_all(dir);
    }
}
