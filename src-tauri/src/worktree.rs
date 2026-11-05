//! The git worktree and branch for an issue (`task_worktree`): created from the freshly fetched
//! base, or reused when the branch already has one. No herdr call, so it works on every backend.
//! Existing worktrees are never modified: no checkout, reset or file change.

use crate::tasks::{TaskVars, expand};
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

/// The issue a worktree is for.
#[derive(Debug, Clone, Deserialize)]
pub struct Issue {
    pub nr: u64,
    pub title: String,
    #[serde(default)]
    pub url: String,
}

/// The worktree of a task.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Worktree {
    pub path: String,
    pub branch: String,
    /// `false` when an existing worktree of the branch was returned.
    pub created: bool,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = crate::gui_path::command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("git could not be started: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(format!("git {} failed: {}", args.join(" "), err.trim()))
    }
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    crate::gui_path::command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// The main worktree of the repository containing `dir`, also when `dir` is a linked worktree.
fn main_worktree(dir: &Path) -> Result<PathBuf, String> {
    let common = git(
        dir,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )?;
    let common = PathBuf::from(common);
    match common.parent() {
        Some(parent) if common.file_name().is_some_and(|n| n == ".git") => Ok(parent.to_path_buf()),
        _ => Err(format!(
            "{} is not in a repository with a working tree",
            dir.display()
        )),
    }
}

/// `path` without `.` and `..` components (lexically; the path need not exist).
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The checked-out worktrees as (path, branch) from `git worktree list --porcelain`.
pub fn parse_worktrees(output: &str) -> Vec<(String, Option<String>)> {
    let mut list: Vec<(String, Option<String>)> = Vec::new();
    for line in output.lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            list.push((path.to_owned(), None));
        } else if let Some(branch) = line.strip_prefix("branch ")
            && let Some(last) = list.last_mut()
        {
            last.1 = Some(
                branch
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch)
                    .to_owned(),
            );
        }
    }
    list
}

/// The ref new branches start from: `origin/release/next`, else the default branch of `origin`.
fn base_ref(root: &Path) -> Result<String, String> {
    if git_ok(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/remotes/origin/release/next",
        ],
    ) {
        return Ok("origin/release/next".into());
    }
    let head = git(root, &["symbolic-ref", "refs/remotes/origin/HEAD"]).map_err(|_| {
        "origin has no release/next and no default branch (refs/remotes/origin/HEAD)".to_owned()
    })?;
    let name = head.strip_prefix("refs/remotes/").unwrap_or(&head);
    Ok(name.to_owned())
}

/// Creates or reuses the worktree of `issue`: `branch_template` and `worktree_dir` are expanded
/// for it, `worktree_dir` relative to the main worktree of the repository of `repo_dir`.
pub fn ensure(
    repo_dir: &str,
    issue: &Issue,
    branch_template: &str,
    worktree_dir: &str,
) -> Result<Worktree, String> {
    let root = main_worktree(Path::new(repo_dir))?;
    let repo = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let vars = TaskVars::new(issue.nr, &issue.title, &issue.url, &repo);
    let branch = expand(branch_template, &vars, false);
    if branch.trim().is_empty() || git(&root, &["check-ref-format", "--branch", &branch]).is_err() {
        return Err(format!("{branch:?} is not a valid branch name"));
    }
    let dir = PathBuf::from(expand(worktree_dir, &vars, false));
    let path = normalize(&if dir.is_absolute() {
        dir
    } else {
        root.join(dir)
    });
    let path_str = path.to_string_lossy().into_owned();

    let listed = parse_worktrees(&git(&root, &["worktree", "list", "--porcelain"])?);
    if let Some((existing, _)) = listed
        .iter()
        .find(|(_, b)| b.as_deref() == Some(branch.as_str()))
    {
        return Ok(Worktree {
            path: existing.clone(),
            branch,
            created: false,
        });
    }
    if path.exists() {
        return Err(format!(
            "{path_str} already exists and is not a worktree of branch {branch}"
        ));
    }

    let local = format!("refs/heads/{branch}");
    if git_ok(&root, &["show-ref", "--verify", "--quiet", &local]) {
        git(&root, &["worktree", "add", &path_str, &branch])?;
    } else {
        git(&root, &["fetch", "origin"])?;
        let base = base_ref(&root)?;
        git(&root, &["worktree", "add", "-b", &branch, &path_str, &base])?;
    }
    Ok(Worktree {
        path: path_str,
        branch,
        created: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    fn run(dir: &Path, args: &[&str]) -> String {
        let out = crate::gui_path::command("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    struct Fixture {
        base: PathBuf,
        repo: PathBuf,
    }

    impl Fixture {
        /// A bare `origin` with branch `main` (and `release/next` when asked) and a clone `app`.
        fn new(release_next: bool) -> Self {
            let base = std::env::temp_dir().join(format!(
                "poimen-wt-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).unwrap();
            let base = base.canonicalize().unwrap();
            let origin = base.join("origin.git");
            let seed = base.join("seed");
            std::fs::create_dir_all(&seed).unwrap();
            run(&base, &["init", "--bare", "-b", "main", "origin.git"]);
            run(&seed, &["init", "-b", "main"]);
            std::fs::write(seed.join("a.txt"), "a").unwrap();
            run(&seed, &["add", "."]);
            run(&seed, &["commit", "-m", "init"]);
            run(
                &seed,
                &["remote", "add", "origin", origin.to_str().unwrap()],
            );
            run(&seed, &["push", "origin", "main"]);
            if release_next {
                run(&seed, &["checkout", "-b", "release/next"]);
                std::fs::write(seed.join("b.txt"), "b").unwrap();
                run(&seed, &["add", "."]);
                run(&seed, &["commit", "-m", "next"]);
                run(&seed, &["push", "origin", "release/next"]);
            }
            run(&base, &["clone", origin.to_str().unwrap(), "app"]);
            let repo = base.join("app");
            Self { base, repo }
        }

        fn issue() -> Issue {
            Issue {
                nr: 7,
                title: "feat(x): Add thing".into(),
                url: "https://example.com/7".into(),
            }
        }

        fn ensure(&self, dir: &Path) -> Result<Worktree, String> {
            ensure(
                dir.to_str().unwrap(),
                &Self::issue(),
                "{type}/{nr}-{slug}",
                "../{repo}-{nr}",
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    #[test]
    fn creates_worktree_from_release_next() {
        let f = Fixture::new(true);
        let wt = f.ensure(&f.repo).unwrap();
        assert_eq!(wt.branch, "feat/7-add-thing");
        assert_eq!(PathBuf::from(&wt.path), f.base.join("app-7"));
        assert!(wt.created);
        assert!(f.base.join("app-7/b.txt").exists(), "based on release/next");
    }

    #[test]
    fn falls_back_to_default_branch() {
        let f = Fixture::new(false);
        let wt = f.ensure(&f.repo).unwrap();
        assert!(wt.created);
        assert!(f.base.join("app-7/a.txt").exists());
        assert!(!f.base.join("app-7/b.txt").exists());
    }

    #[test]
    fn base_is_fetched_in_the_same_call() {
        let f = Fixture::new(false);
        let seed = f.base.join("seed");
        std::fs::write(seed.join("c.txt"), "c").unwrap();
        run(&seed, &["add", "."]);
        run(&seed, &["commit", "-m", "later"]);
        run(&seed, &["push", "origin", "main"]);
        f.ensure(&f.repo).unwrap();
        assert!(f.base.join("app-7/c.txt").exists());
    }

    #[test]
    fn reuses_the_worktree_of_the_branch() {
        let f = Fixture::new(true);
        let first = f.ensure(&f.repo).unwrap();
        std::fs::write(f.base.join("app-7/wip.txt"), "wip").unwrap();
        let head = run(&f.base.join("app-7"), &["rev-parse", "HEAD"]);
        let again = f.ensure(&f.repo).unwrap();
        assert_eq!(again.path, first.path);
        assert!(!again.created);
        assert!(f.base.join("app-7/wip.txt").exists());
        assert_eq!(run(&f.base.join("app-7"), &["rev-parse", "HEAD"]), head);
    }

    #[test]
    fn reuses_an_existing_branch_without_worktree() {
        let f = Fixture::new(true);
        run(&f.repo, &["branch", "feat/7-add-thing", "main"]);
        let wt = f.ensure(&f.repo).unwrap();
        assert!(wt.created);
        assert!(f.base.join("app-7/a.txt").exists());
        assert!(!f.base.join("app-7/b.txt").exists(), "branch kept at main");
    }

    #[test]
    fn path_collision_names_the_path() {
        let f = Fixture::new(true);
        std::fs::create_dir_all(f.base.join("app-7")).unwrap();
        std::fs::write(f.base.join("app-7/keep.txt"), "k").unwrap();
        let err = f.ensure(&f.repo).unwrap_err();
        assert!(err.contains("app-7"), "{err}");
        assert!(f.base.join("app-7/keep.txt").exists());
        let branches = run(&f.repo, &["branch", "--list", "feat/7-add-thing"]);
        assert!(branches.is_empty(), "no branch created");
    }

    #[test]
    fn linked_worktree_as_input_uses_the_main_worktree() {
        let f = Fixture::new(true);
        run(
            &f.repo,
            &[
                "worktree",
                "add",
                "-b",
                "other",
                f.base.join("other").to_str().unwrap(),
                "main",
            ],
        );
        let wt = f.ensure(&f.base.join("other")).unwrap();
        assert_eq!(PathBuf::from(&wt.path), f.base.join("app-7"));
    }

    #[test]
    fn rejects_a_directory_outside_a_repository() {
        let f = Fixture::new(true);
        assert!(f.ensure(&f.base).is_err());
    }

    #[test]
    fn parses_the_porcelain_list() {
        let out =
            "worktree /r\nHEAD abc\nbranch refs/heads/main\n\nworktree /r-1\nHEAD def\ndetached\n";
        assert_eq!(
            parse_worktrees(out),
            vec![("/r".into(), Some("main".into())), ("/r-1".into(), None)]
        );
    }

    #[test]
    fn normalize_resolves_parent_components() {
        assert_eq!(
            normalize(Path::new("/a/app/../app-7")),
            PathBuf::from("/a/app-7")
        );
    }
}
