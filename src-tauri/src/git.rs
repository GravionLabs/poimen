//! Read-only context of a directory's repository: the branch and whether it is a linked worktree.
//! herdr's pane list carries no branch, so the sidebar asks the repository itself.

use std::collections::HashMap;
use std::process::Command;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoInfo {
    /// Branch name; `None` on a detached HEAD.
    pub branch: Option<String>,
    /// True for a linked worktree (not the main checkout).
    pub worktree: bool,
}

/// Parses the output of `rev-parse --abbrev-ref HEAD --absolute-git-dir --git-common-dir`.
pub fn parse_rev_parse(output: &str) -> Option<RepoInfo> {
    let mut lines = output.lines();
    let head = lines.next()?.trim();
    let git_dir = lines.next()?.trim();
    let common = lines.next()?.trim();
    if head.is_empty() {
        return None;
    }
    Some(RepoInfo {
        branch: (head != "HEAD").then(|| head.to_string()),
        worktree: git_dir != common,
    })
}

pub fn info(dir: &str) -> Option<RepoInfo> {
    let out = Command::new("git")
        .args(["-C", dir, "rev-parse", "--abbrev-ref", "HEAD"])
        .args([
            "--absolute-git-dir",
            "--path-format=absolute",
            "--git-common-dir",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_rev_parse(&String::from_utf8_lossy(&out.stdout))
}

/// The top-level directory of the repository containing `dir`; `None` outside a repository.
pub fn toplevel(dir: &str) -> Option<String> {
    let out = Command::new("git")
        .args(["-C", dir, "rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let top = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!top.is_empty()).then_some(top)
}

/// Parses `rev-parse --show-toplevel --git-common-dir` (absolute paths): the toplevel and the
/// common git dir that identifies the repository across its linked worktrees.
pub fn parse_roots(output: &str) -> Option<(String, String)> {
    let mut lines = output.lines();
    let top = lines.next()?.trim();
    let common = lines.next()?.trim();
    (!top.is_empty() && !common.is_empty()).then(|| (top.to_owned(), common.to_owned()))
}

/// The distinct repositories among `dirs` (a linked worktree counts as its main repository), each
/// by the toplevel of the first directory that belongs to it; directories outside one are skipped.
pub fn repo_roots(dirs: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut roots = Vec::new();
    for dir in dirs {
        let Ok(out) = Command::new("git")
            .args(["-C", dir, "rev-parse", "--path-format=absolute"])
            .args(["--show-toplevel", "--git-common-dir"])
            .output()
        else {
            continue;
        };
        if !out.status.success() {
            continue;
        }
        if let Some((top, common)) = parse_roots(&String::from_utf8_lossy(&out.stdout))
            && seen.insert(common)
        {
            roots.push(top);
        }
    }
    roots
}

/// Repository info per directory; directories outside a repository are left out.
pub fn infos(dirs: Vec<String>) -> HashMap<String, RepoInfo> {
    dirs.into_iter()
        .filter_map(|dir| info(&dir).map(|i| (dir, i)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_branch_in_the_main_checkout() {
        let out = "main\n/r/.git\n/r/.git\n";
        assert_eq!(
            parse_rev_parse(out),
            Some(RepoInfo {
                branch: Some("main".into()),
                worktree: false
            })
        );
    }

    #[test]
    fn detects_a_linked_worktree() {
        let out = "feat/x\n/r/.git/worktrees/x\n/r/.git\n";
        assert_eq!(
            parse_rev_parse(out),
            Some(RepoInfo {
                branch: Some("feat/x".into()),
                worktree: true
            })
        );
    }

    #[test]
    fn detached_head_has_no_branch() {
        let out = "HEAD\n/r/.git\n/r/.git\n";
        assert_eq!(parse_rev_parse(out).unwrap().branch, None);
    }

    #[test]
    fn rejects_incomplete_output() {
        assert_eq!(parse_rev_parse(""), None);
        assert_eq!(parse_rev_parse("main\n"), None);
    }

    #[test]
    fn roots_need_a_toplevel_and_a_common_dir() {
        assert_eq!(
            parse_roots("/r\n/r/.git\n"),
            Some(("/r".into(), "/r/.git".into()))
        );
        assert_eq!(parse_roots("/r\n"), None);
        assert_eq!(parse_roots(""), None);
    }

    #[test]
    fn no_roots_outside_a_repository() {
        assert!(repo_roots(&["/nonexistent-poimen-dir".to_owned()]).is_empty());
    }

    #[test]
    fn outside_a_repository_is_none() {
        assert_eq!(info("/nonexistent-poimen-dir"), None);
    }
}
