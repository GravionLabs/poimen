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
pub fn main_worktree(dir: &Path) -> Result<PathBuf, String> {
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

// ---------------------------------------------------------------------------------------------
// Clean up merged tasks: remove the worktrees, spaces and local branches of tasks whose pull
// request is merged. Invariants: never a worktree with uncommitted or untracked changes, the main
// worktree or the worktree of the focused space; never a protected branch (`main`, `master`,
// `release/*`, the default branch of `origin`); never a branch with commits on no remote branch.
// Nothing here forces: `git worktree remove` and `git branch -d` without `--force`/`-D`.
// ---------------------------------------------------------------------------------------------

/// A merged pull request: its head branch and number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedPr {
    pub branch: String,
    pub number: u64,
}

/// Parses `gh pr list --json headRefName,number`; the highest number wins per head branch.
pub fn parse_merged_prs(json: &str) -> Result<Vec<MergedPr>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("unexpected gh answer: {e}"))?;
    let list = value.as_array().ok_or("unexpected gh answer")?;
    let mut out: Vec<MergedPr> = Vec::new();
    for item in list {
        let (Some(branch), Some(number)) = (item["headRefName"].as_str(), item["number"].as_u64())
        else {
            continue;
        };
        match out.iter_mut().find(|p| p.branch == branch) {
            Some(p) => p.number = p.number.max(number),
            None => out.push(MergedPr {
                branch: branch.to_owned(),
                number,
            }),
        }
    }
    Ok(out)
}

/// The merged pull requests of `repo` (`gh pr list --state merged`).
pub fn merged_prs<R: crate::github::Runner>(
    runner: &R,
    repo: &crate::github::Repo,
) -> Result<Vec<MergedPr>, String> {
    let args: Vec<String> = [
        "pr",
        "list",
        "--repo",
        &format!("{}/{}", repo.owner, repo.name),
        "--state",
        "merged",
        "--json",
        "headRefName,number",
        "--limit",
        "500",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    let out = runner.gh(&args).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            "gh is not installed".to_owned()
        } else {
            format!("gh could not be run: {e}")
        }
    })?;
    if out.code != Some(0) {
        return Err(format!("gh pr list failed: {}", out.stderr.trim()));
    }
    parse_merged_prs(&out.stdout)
}

/// The issue number of a task branch `<type>/<nr>-<slug>`.
pub fn issue_of_branch(branch: &str) -> Option<u64> {
    let (_, rest) = branch.split_once('/')?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let after = &rest[digits.len()..];
    if digits.is_empty() || !(after.is_empty() || after.starts_with('-')) {
        return None;
    }
    digits.parse().ok()
}

/// The default branch of `origin` (`origin/HEAD`), when known.
fn default_branch(root: &Path) -> Option<String> {
    let head = git(
        root,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .ok()?;
    head.strip_prefix("origin/").map(str::to_owned)
}

/// Whether `branch` is never deleted.
pub fn is_protected(branch: &str, default: Option<&str>) -> bool {
    branch == "main"
        || branch == "master"
        || branch.starts_with("release/")
        || default == Some(branch)
}

/// What the cleanup removed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Removed {
    pub path: String,
    pub branch: String,
    pub pr: u64,
    pub branch_deleted: bool,
    /// Why the branch was kept.
    pub note: Option<String>,
}

/// A worktree the cleanup left, with the reason and the command to remove it by hand.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Left {
    pub path: String,
    pub branch: String,
    pub pr: u64,
    pub reason: String,
    pub command: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct CleanupReport {
    pub removed: Vec<Removed>,
    pub left: Vec<Left>,
}

/// What a cleanup runs for each worktree: the built-in removal or the `cleanup` command of a
/// task mode (a template, expanded per worktree).
#[derive(Debug, Clone, PartialEq)]
pub enum CleanupMode {
    Builtin,
    Command(String),
}

/// The command that removes a worktree (and its branch) by hand.
pub fn manual_command(path: &str, branch: &str) -> String {
    format!(
        "git worktree remove {} && git branch -d {}",
        crate::tasks::shell_quote(path),
        crate::tasks::shell_quote(branch)
    )
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| normalize(path))
}

/// The spaces with terminals in `worktree`: those with all their terminals in it and those with
/// only some of them in it.
fn spaces_in(snapshot: &crate::backend::Snapshot, worktree: &Path) -> (Vec<String>, Vec<String>) {
    let root = canonical(worktree);
    let mut seen: Vec<(String, bool, bool)> = Vec::new(); // (space, all in, any in)
    for pane in snapshot.panes.as_array().into_iter().flatten() {
        let Some(space) = pane["space_id"].as_str() else {
            continue;
        };
        let dir = ["foreground_cwd", "cwd"]
            .iter()
            .filter_map(|k| pane[*k].as_str())
            .find(|d| !d.is_empty());
        let inside = dir.is_some_and(|d| canonical(Path::new(d)).starts_with(&root));
        match seen.iter_mut().find(|s| s.0 == space) {
            Some(s) => {
                s.1 &= inside;
                s.2 |= inside;
            }
            None => seen.push((space.to_owned(), inside, inside)),
        }
    }
    let whole = seen.iter().filter(|s| s.1).map(|s| s.0.clone()).collect();
    let partial = seen
        .iter()
        .filter(|s| s.2 && !s.1)
        .map(|s| s.0.clone())
        .collect();
    (whole, partial)
}

/// Runs the expanded cleanup command in `root` through the platform shell.
fn run_cleanup_command(root: &Path, command: &str) -> Result<(), String> {
    let mut cmd = if cfg!(windows) {
        let mut c = crate::gui_path::command("powershell");
        c.args(["-NoProfile", "-Command", command]);
        c
    } else {
        let mut c = crate::gui_path::command("sh");
        c.args(["-c", command]);
        c
    };
    let out = cmd
        .current_dir(root)
        .env("PATH", crate::gui_path::child_path())
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("the cleanup command could not be started: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    let text = if err.trim().is_empty() {
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        err.into_owned()
    };
    let last = text
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("");
    Err(format!(
        "the cleanup command failed ({}){}{}",
        out.status
            .code()
            .map_or("killed".to_owned(), |c| format!("exit {c}")),
        if last.is_empty() { "" } else { ": " },
        last.trim()
    ))
}

/// Cleans up the worktrees of the repository at `root` (its main worktree) whose branch is the
/// head of a merged pull request. `focused_space` is never touched. Spaces with terminals in a
/// worktree are closed through `backend` before it is removed.
pub fn cleanup(
    root: &Path,
    merged: &[MergedPr],
    mode: &CleanupMode,
    backend: &dyn crate::backend::Backend,
    focused_space: Option<&str>,
) -> Result<CleanupReport, String> {
    let root = canonical(root);
    let listed = parse_worktrees(&git(&root, &["worktree", "list", "--porcelain"])?);
    let default = default_branch(&root);
    let repo = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut report = CleanupReport::default();
    for (path, branch) in listed {
        let Some(branch) = branch else { continue };
        let Some(pr) = merged.iter().find(|p| p.branch == branch) else {
            continue;
        };
        if canonical(Path::new(&path)) == root {
            continue; // the main worktree
        }
        let left = |reason: String| Left {
            path: path.clone(),
            branch: branch.clone(),
            pr: pr.number,
            reason,
            command: manual_command(&path, &branch),
        };
        if is_protected(&branch, default.as_deref()) {
            report
                .left
                .push(left(format!("{branch} is a protected branch")));
            continue;
        }
        let snapshot = backend.snapshot()?;
        let (whole, partial) = spaces_in(&snapshot, Path::new(&path));
        if focused_space.is_some_and(|f| whole.iter().chain(&partial).any(|s| s == f)) {
            report
                .left
                .push(left("its space is the focused one".into()));
            continue;
        }
        if !partial.is_empty() {
            report.left.push(left(format!(
                "space {} also has terminals outside the worktree",
                partial.join(", ")
            )));
            continue;
        }
        match mode {
            CleanupMode::Builtin => match git(Path::new(&path), &["status", "--porcelain"]) {
                Ok(status) if status.is_empty() => {}
                Ok(_) => {
                    report
                        .left
                        .push(left("it has uncommitted or untracked changes".into()));
                    continue;
                }
                Err(e) => {
                    report.left.push(left(e));
                    continue;
                }
            },
            CleanupMode::Command(_) => {
                if issue_of_branch(&branch).is_none() {
                    report
                        .left
                        .push(left("the branch name has no issue number".into()));
                    continue;
                }
            }
        }
        let mut close_failed = None;
        for space in &whole {
            if let Err(e) = backend.close_space(space) {
                close_failed = Some(format!("space {space} could not be closed: {e}"));
                break;
            }
        }
        if let Some(reason) = close_failed {
            report.left.push(left(reason));
            continue;
        }
        match mode {
            CleanupMode::Builtin => {
                match remove_builtin(&root, &path, &branch, default.as_deref()) {
                    Ok((branch_deleted, note)) => report.removed.push(Removed {
                        path: path.clone(),
                        branch: branch.clone(),
                        pr: pr.number,
                        branch_deleted,
                        note,
                    }),
                    Err(e) => report.left.push(left(e)),
                }
            }
            CleanupMode::Command(template) => {
                let nr = issue_of_branch(&branch).unwrap_or_default();
                let mut vars = TaskVars::new(nr, "", "", &repo);
                vars.branch = branch.clone();
                vars.pr = pr.number.to_string();
                let command = expand(template, &vars, true);
                match run_cleanup_command(&root, &command) {
                    Ok(()) => report.removed.push(Removed {
                        path: path.clone(),
                        branch: branch.clone(),
                        pr: pr.number,
                        branch_deleted: !git_ok(
                            &root,
                            &[
                                "show-ref",
                                "--verify",
                                "--quiet",
                                &format!("refs/heads/{branch}"),
                            ],
                        ),
                        note: None,
                    }),
                    Err(e) => report.left.push(Left { command, ..left(e) }),
                }
            }
        }
    }
    Ok(report)
}

/// `git worktree remove` (never forced), then `git branch -d` when every commit of the branch is
/// on a remote branch. Returns whether the branch was deleted and why not.
fn remove_builtin(
    root: &Path,
    path: &str,
    branch: &str,
    default: Option<&str>,
) -> Result<(bool, Option<String>), String> {
    git(root, &["worktree", "remove", path])?;
    if is_protected(branch, default) {
        return Ok((false, Some("branch kept: protected".into())));
    }
    let local = format!("refs/heads/{branch}");
    let unpushed = git(root, &["rev-list", "--count", &local, "--not", "--remotes"])
        .ok()
        .and_then(|n| n.parse::<u64>().ok());
    match unpushed {
        Some(0) => {}
        Some(n) => {
            return Ok((
                false,
                Some(format!("branch kept: {n} commit(s) on no remote branch")),
            ));
        }
        None => {
            return Ok((
                false,
                Some("branch kept: its commits could not be checked".into()),
            ));
        }
    }
    match git(root, &["branch", "-d", branch]) {
        Ok(_) => Ok((true, None)),
        Err(e) => Ok((false, Some(format!("branch kept: {e}")))),
    }
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

    // --- cleanup of merged tasks ---

    use crate::backend_fake::FakeBackend;
    use serde_json::json;

    impl Fixture {
        /// A task worktree `../app-<nr>` on `feat/<nr>-x`, pushed to origin.
        fn task(&self, nr: u64) -> PathBuf {
            let path = self.base.join(format!("app-{nr}"));
            let branch = format!("feat/{nr}-x");
            run(
                &self.repo,
                &[
                    "worktree",
                    "add",
                    "-b",
                    &branch,
                    path.to_str().unwrap(),
                    "main",
                ],
            );
            run(&path, &["push", "-u", "origin", &branch]);
            path
        }

        fn merged(nrs: &[u64]) -> Vec<MergedPr> {
            nrs.iter()
                .map(|n| MergedPr {
                    branch: format!("feat/{n}-x"),
                    number: 100 + n,
                })
                .collect()
        }

        fn branches(&self) -> String {
            run(&self.repo, &["branch", "--format=%(refname:short)"])
        }

        fn clean(
            &self,
            merged: &[MergedPr],
            backend: &FakeBackend,
            focused: Option<&str>,
        ) -> CleanupReport {
            cleanup(&self.repo, merged, &CleanupMode::Builtin, backend, focused).unwrap()
        }
    }

    fn panes_in(backend: &FakeBackend, panes: &[(&str, &Path)]) {
        let list: Vec<_> = panes
            .iter()
            .map(|(space, dir)| json!({"space_id": space, "cwd": dir.to_str().unwrap()}))
            .collect();
        backend.snapshot.lock().unwrap().panes = json!(list);
    }

    #[test]
    fn a_merged_clean_worktree_is_removed_with_its_branch() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let report = f.clean(&Fixture::merged(&[7]), &FakeBackend::default(), None);
        assert_eq!(report.left, vec![]);
        assert_eq!(report.removed.len(), 1);
        assert_eq!(report.removed[0].pr, 107);
        assert!(report.removed[0].branch_deleted);
        assert!(!wt.exists());
        assert!(!f.branches().contains("feat/7-x"));
    }

    #[test]
    fn a_worktree_whose_pull_request_is_not_merged_is_untouched() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let other = f.task(8);
        let report = f.clean(&Fixture::merged(&[8]), &FakeBackend::default(), None);
        assert_eq!(report.removed.len(), 1);
        assert!(wt.exists() && !other.exists());
        assert!(f.branches().contains("feat/7-x"));
    }

    #[test]
    fn uncommitted_changes_leave_the_worktree_with_reason_and_command() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        std::fs::write(wt.join("a.txt"), "changed").unwrap();
        let report = f.clean(&Fixture::merged(&[7]), &FakeBackend::default(), None);
        assert!(report.removed.is_empty());
        assert_eq!(report.left.len(), 1);
        assert!(report.left[0].reason.contains("uncommitted"));
        assert!(report.left[0].command.contains("git worktree remove"));
        assert!(report.left[0].command.contains("feat/7-x"));
        assert_eq!(
            std::fs::read_to_string(wt.join("a.txt")).unwrap(),
            "changed"
        );
        assert!(f.branches().contains("feat/7-x"));
    }

    #[test]
    fn untracked_files_leave_the_worktree() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        std::fs::write(wt.join("new.txt"), "n").unwrap();
        let report = f.clean(&Fixture::merged(&[7]), &FakeBackend::default(), None);
        assert_eq!(report.left.len(), 1);
        assert!(wt.join("new.txt").exists());
    }

    #[test]
    fn an_unpushed_commit_removes_the_worktree_but_keeps_the_branch() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        std::fs::write(wt.join("c.txt"), "c").unwrap();
        run(&wt, &["add", "."]);
        run(&wt, &["commit", "-m", "local only"]);
        let report = f.clean(&Fixture::merged(&[7]), &FakeBackend::default(), None);
        assert_eq!(report.removed.len(), 1);
        assert!(!report.removed[0].branch_deleted);
        let note = report.removed[0].note.as_deref().unwrap();
        assert!(note.contains("no remote branch"), "{note}");
        assert!(!wt.exists());
        assert!(f.branches().contains("feat/7-x"), "commits are kept");
    }

    #[test]
    fn protected_branches_are_never_removed_or_deleted() {
        let f = Fixture::new(true);
        for branch in ["release/next", "master"] {
            let dir = f.base.join(branch.replace('/', "-"));
            run(
                &f.repo,
                &[
                    "worktree",
                    "add",
                    "-b",
                    branch,
                    dir.to_str().unwrap(),
                    "origin/main",
                ],
            );
            // Local-only branches: protection must not depend on the remote state.
        }
        let merged: Vec<_> = ["release/next", "master", "main"]
            .iter()
            .enumerate()
            .map(|(i, b)| MergedPr {
                branch: (*b).into(),
                number: i as u64 + 1,
            })
            .collect();
        let report = f.clean(&merged, &FakeBackend::default(), None);
        assert!(report.removed.is_empty(), "{report:?}");
        assert_eq!(report.left.len(), 2, "the main worktree is not listed");
        assert!(report.left.iter().all(|l| l.reason.contains("protected")));
        let branches = f.branches();
        assert!(branches.contains("release/next") && branches.contains("master"));
        assert!(f.base.join("release-next").exists());
    }

    #[test]
    fn the_default_branch_of_origin_is_protected() {
        let f = Fixture::new(true);
        run(&f.repo, &["checkout", "-q", "-b", "develop"]);
        run(&f.repo, &["push", "origin", "develop"]);
        run(&f.repo, &["checkout", "-q", "main"]);
        run(
            &f.repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/develop",
            ],
        );
        assert_eq!(default_branch(&f.repo), Some("develop".into()));
        let dir = f.base.join("dev");
        run(
            &f.repo,
            &["worktree", "add", dir.to_str().unwrap(), "develop"],
        );
        let merged = vec![MergedPr {
            branch: "develop".into(),
            number: 1,
        }];
        let report = f.clean(&merged, &FakeBackend::default(), None);
        assert!(dir.exists());
        assert_eq!(report.left.len(), 1);
        assert!(is_protected("develop", Some("develop")));
        assert!(!is_protected("feat/1-x", Some("develop")));
    }

    #[test]
    fn the_focused_space_keeps_its_worktree() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let backend = FakeBackend::default();
        panes_in(&backend, &[("w3", &wt)]);
        let report = f.clean(&Fixture::merged(&[7]), &backend, Some("w3"));
        assert!(wt.exists());
        assert!(report.left[0].reason.contains("focused"));
        assert!(!backend.calls().iter().any(|c| c.starts_with("close_space")));
    }

    #[test]
    fn the_space_of_a_removed_worktree_is_closed_before_the_removal() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let nested = wt.join("src");
        std::fs::create_dir_all(&nested).unwrap();
        let main_space = f.repo.clone();
        let backend = FakeBackend::default();
        panes_in(
            &backend,
            &[("w2", &wt), ("w2", &nested), ("w1", &main_space)],
        );
        let report = f.clean(&Fixture::merged(&[7]), &backend, Some("w1"));
        assert_eq!(report.removed.len(), 1);
        let closes: Vec<_> = backend
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("close_space"))
            .collect();
        assert_eq!(closes, vec!["close_space w2".to_owned()]);
    }

    #[test]
    fn a_space_with_terminals_outside_the_worktree_is_not_closed() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let backend = FakeBackend::default();
        panes_in(&backend, &[("w2", &wt), ("w2", &f.repo)]);
        let report = f.clean(&Fixture::merged(&[7]), &backend, None);
        assert!(wt.exists());
        assert!(report.left[0].reason.contains("outside"));
        assert!(!backend.calls().iter().any(|c| c.starts_with("close_space")));
    }

    #[test]
    fn a_dirty_worktree_keeps_its_space_open() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        std::fs::write(wt.join("new.txt"), "n").unwrap();
        let backend = FakeBackend::default();
        panes_in(&backend, &[("w2", &wt)]);
        f.clean(&Fixture::merged(&[7]), &backend, None);
        assert!(!backend.calls().iter().any(|c| c.starts_with("close_space")));
    }

    #[test]
    fn a_failed_space_close_leaves_the_worktree() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let backend = FakeBackend::default();
        panes_in(&backend, &[("w2", &wt)]);
        backend.fail_on("close_space");
        let report = f.clean(&Fixture::merged(&[7]), &backend, None);
        assert!(wt.exists());
        assert!(report.left[0].reason.contains("could not be closed"));
    }

    #[test]
    fn a_cleanup_command_replaces_the_built_in_removal() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let mode =
            CleanupMode::Command("printf '%s-%s-%s' {pr} {nr} {branch} > cleanup.out".into());
        let report = cleanup(
            &f.repo,
            &Fixture::merged(&[7]),
            &mode,
            &FakeBackend::default(),
            None,
        )
        .unwrap();
        assert_eq!(report.removed.len(), 1, "{report:?}");
        assert_eq!(
            std::fs::read_to_string(f.repo.join("cleanup.out")).unwrap(),
            "107-7-feat/7-x",
            "run in the repository root with expanded placeholders"
        );
        assert!(wt.exists(), "the command, not poimen, removes the worktree");
    }

    #[test]
    fn a_failing_cleanup_command_leaves_the_worktree_with_its_command() {
        let f = Fixture::new(true);
        f.task(7);
        let mode = CleanupMode::Command("echo boom >&2; exit 3 # {pr}".into());
        let report = cleanup(
            &f.repo,
            &Fixture::merged(&[7]),
            &mode,
            &FakeBackend::default(),
            None,
        )
        .unwrap();
        assert!(report.removed.is_empty());
        assert!(
            report.left[0].reason.contains("exit 3): boom"),
            "{:?}",
            report.left
        );
        assert!(report.left[0].command.contains("107"));
    }

    #[test]
    fn a_cleanup_command_needs_an_issue_number_in_the_branch() {
        let f = Fixture::new(true);
        let dir = f.base.join("odd");
        run(
            &f.repo,
            &[
                "worktree",
                "add",
                "-b",
                "odd",
                dir.to_str().unwrap(),
                "main",
            ],
        );
        let merged = vec![MergedPr {
            branch: "odd".into(),
            number: 1,
        }];
        let report = cleanup(
            &f.repo,
            &merged,
            &CleanupMode::Command("touch ran".into()),
            &FakeBackend::default(),
            None,
        )
        .unwrap();
        assert_eq!(report.left.len(), 1);
        assert!(!f.repo.join("ran").exists());
    }

    #[test]
    fn issue_numbers_come_from_the_branch_name() {
        assert_eq!(issue_of_branch("feat/192-clean-up"), Some(192));
        assert_eq!(issue_of_branch("fix/7"), Some(7));
        assert_eq!(issue_of_branch("feat/7x-y"), None);
        assert_eq!(issue_of_branch("main"), None);
        assert_eq!(issue_of_branch("feat/no-number"), None);
    }

    #[test]
    fn merged_pull_requests_are_parsed_and_the_highest_number_wins() {
        let json = r#"[{"headRefName":"a","number":3},{"headRefName":"a","number":9},
            {"headRefName":"b","number":4},{"number":5}]"#;
        let got = parse_merged_prs(json).unwrap();
        assert_eq!(
            got,
            vec![
                MergedPr {
                    branch: "a".into(),
                    number: 9
                },
                MergedPr {
                    branch: "b".into(),
                    number: 4
                },
            ]
        );
        assert!(parse_merged_prs("{}").is_err());
        assert!(parse_merged_prs("nope").is_err());
    }

    struct FakeGh(std::io::Result<crate::github::Output>);

    impl crate::github::Runner for FakeGh {
        fn gh(&self, args: &[String]) -> std::io::Result<crate::github::Output> {
            assert_eq!(&args[..2], ["pr", "list"]);
            assert!(args.contains(&"o/r".to_owned()) && args.contains(&"merged".to_owned()));
            match &self.0 {
                Ok(o) => Ok(crate::github::Output {
                    code: o.code,
                    stdout: o.stdout.clone(),
                    stderr: o.stderr.clone(),
                }),
                Err(e) => Err(std::io::Error::new(e.kind(), "x")),
            }
        }
        fn now(&self) -> std::time::Duration {
            std::time::Duration::ZERO
        }
    }

    fn gh_repo() -> crate::github::Repo {
        crate::github::Repo {
            owner: "o".into(),
            name: "r".into(),
        }
    }

    #[test]
    fn the_gh_call_lists_merged_pull_requests_and_reports_failures() {
        let ok = FakeGh(Ok(crate::github::Output {
            code: Some(0),
            stdout: r#"[{"headRefName":"feat/1-x","number":2}]"#.into(),
            stderr: String::new(),
        }));
        assert_eq!(merged_prs(&ok, &gh_repo()).unwrap().len(), 1);
        let failed = FakeGh(Ok(crate::github::Output {
            code: Some(1),
            stdout: String::new(),
            stderr: "not logged in\n".into(),
        }));
        assert!(
            merged_prs(&failed, &gh_repo())
                .unwrap_err()
                .contains("not logged in")
        );
        let missing = FakeGh(Err(std::io::Error::from(std::io::ErrorKind::NotFound)));
        assert!(
            merged_prs(&missing, &gh_repo())
                .unwrap_err()
                .contains("not installed")
        );
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
