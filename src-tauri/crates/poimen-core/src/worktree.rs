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

/// The top level of the worktree that contains `dir`.
pub fn toplevel(dir: &Path) -> Result<PathBuf, String> {
    git(dir, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
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
        let base = crate::git::base_ref(&root.to_string_lossy()).ok_or_else(|| {
            "origin has no release/next and no default branch (refs/remotes/origin/HEAD)".to_owned()
        })?;
        git(&root, &["worktree", "add", "-b", &branch, &path_str, &base])?;
    }
    Ok(Worktree {
        path: path_str,
        branch,
        created: true,
    })
}

/// A worktree of a repository and the branch checked out in it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Checkout {
    pub path: String,
    pub branch: Option<String>,
}

/// The worktrees of the repository of `dir` with their branches (`git worktree list --porcelain`).
pub fn list_worktrees(dir: &str) -> Result<Vec<Checkout>, String> {
    let root = main_worktree(Path::new(dir))?;
    Ok(
        parse_worktrees(&git(&root, &["worktree", "list", "--porcelain"])?)
            .into_iter()
            .map(|(path, branch)| Checkout { path, branch })
            .collect(),
    )
}

/// Creates or reuses a worktree with the head of pull request `nr` checked out, also for forks.
///
/// The head is fetched from `origin` (`refs/pull/<nr>/head`). The branch is `head_ref` itself when
/// that branch exists on `origin` at the same commit (an own pull request), else `pr/<nr>`. A
/// branch already checked out in a worktree returns that worktree; the path is `../<repo>-pr<nr>`
/// next to the main worktree. Never touches an existing worktree, and a local `pr/<nr>` is only
/// moved forward (fast-forward to the fetched head), never reset.
pub fn pr_worktree(repo_dir: &str, nr: u64, head_ref: Option<&str>) -> Result<Worktree, String> {
    let root = main_worktree(Path::new(repo_dir))?;
    let repo = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = root
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", root.display()))?;
    let path = normalize(&parent.join(format!("{repo}-pr{nr}")));
    let path_str = path.to_string_lossy().into_owned();

    git(&root, &["fetch", "origin", &format!("refs/pull/{nr}/head")])
        .map_err(|e| format!("pull request #{nr} could not be fetched from origin: {e}"))?;
    let head = git(&root, &["rev-parse", "FETCH_HEAD^{commit}"])?;

    // An own pull request: its head branch exists on origin at the same commit.
    let own = match head_ref.map(str::trim).filter(|h| !h.is_empty()) {
        Some(h)
            if git(&root, &["check-ref-format", "--branch", h]).is_ok()
                && git(
                    &root,
                    &[
                        "fetch",
                        "origin",
                        &format!("+refs/heads/{h}:refs/remotes/origin/{h}"),
                    ],
                )
                .is_ok()
                && git(
                    &root,
                    &["rev-parse", &format!("refs/remotes/origin/{h}^{{commit}}")],
                )
                .is_ok_and(|sha| sha == head) =>
        {
            Some(h.to_owned())
        }
        _ => None,
    };
    let branch = own.clone().unwrap_or_else(|| format!("pr/{nr}"));

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
        let same = |p: &str| canonical(Path::new(p)) == canonical(&path);
        return match listed.iter().find(|(p, _)| same(p)) {
            Some((existing, other)) => Ok(Worktree {
                path: existing.clone(),
                branch: other.clone().unwrap_or(branch),
                created: false,
            }),
            None => Err(format!(
                "{path_str} already exists and is not a worktree of this repository"
            )),
        };
    }

    let local = format!("refs/heads/{branch}");
    if git_ok(&root, &["show-ref", "--verify", "--quiet", &local]) {
        // Only forward: a local branch with commits of its own stays as it is.
        if git_ok(&root, &["merge-base", "--is-ancestor", &local, &head]) {
            git(&root, &["update-ref", &local, &head])?;
        }
        git(&root, &["worktree", "add", &path_str, &branch])?;
    } else if own.is_some() {
        let remote = format!("origin/{branch}");
        git(
            &root,
            &[
                "worktree", "add", "--track", "-b", &branch, &path_str, &remote,
            ],
        )?;
    } else {
        git(&root, &["worktree", "add", "-b", &branch, &path_str, &head])?;
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
    /// The project has a teardown but is not trusted: nothing was removed; ask whether to
    /// remove without teardown.
    #[serde(rename = "teardownUntrusted")]
    pub teardown_untrusted: bool,
}

/// What a cleanup runs for each worktree: the built-in removal or the `cleanup` command of a
/// task mode (a template, expanded per worktree).
#[derive(Debug, Clone, PartialEq)]
pub enum CleanupMode {
    Builtin,
    Command(crate::tasks::CommandSpec),
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

/// The spaces in `worktree`: those with all their terminals in it (or, without terminals, whose
/// own `cwd` of the poimen backend is in it) and those with only some of their terminals in it.
/// The space's own `cwd` is matched by containment, as the terminals are, unlike the exact rule of
/// `spaces::find_space`; it only counts for a space without terminals.
fn spaces_in(view: &SpaceView, worktree: &Path) -> (Vec<String>, Vec<String>) {
    let root = canonical(worktree);
    let mut seen: Vec<(String, bool, bool)> = Vec::new(); // (space, all in, any in)
    for pane in view.panes.as_array().into_iter().flatten() {
        let Some(space) = pane["workspace_id"].as_str() else {
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
    for workspace in view.workspaces.as_array().into_iter().flatten() {
        let Some(space) = workspace["workspace_id"].as_str() else {
            continue;
        };
        if seen.iter().any(|s| s.0 == space) {
            continue; // its terminals decide
        }
        let inside = workspace["cwd"]
            .as_str()
            .filter(|d| !d.is_empty())
            .is_some_and(|d| canonical(Path::new(d)).starts_with(&root));
        if inside {
            seen.push((space.to_owned(), true, true));
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

/// Runs the expanded cleanup command in `root`: an argv directly, a (deprecated) shell line
/// through the platform shell.
fn run_cleanup_command(root: &Path, command: &crate::tasks::Cmd) -> Result<(), String> {
    use crate::tasks::Cmd;
    let mut cmd = match command {
        Cmd::Argv(argv) => {
            let mut c = crate::gui_path::command(&argv[0]);
            c.args(&argv[1..]);
            c
        }
        Cmd::Shell(line) if cfg!(windows) => {
            let mut c = crate::gui_path::command("powershell");
            c.args(["-NoProfile", "-Command", line]);
            c
        }
        Cmd::Shell(line) => {
            let mut c = crate::gui_path::command("sh");
            c.args(["-c", line]);
            c
        }
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

/// The spaces of the running backend as the cleanup sees them: `panes` with `workspace_id` and
/// `cwd`/`foreground_cwd`, `workspaces` with `workspace_id` and `cwd`.
#[derive(Debug, Clone, Default)]
pub struct SpaceView {
    pub panes: serde_json::Value,
    pub workspaces: serde_json::Value,
}

/// The spaces the cleanup may close; the app implements it over its `Backend`, poimen-core stays
/// free of it.
pub trait Spaces {
    /// The current spaces and their panes.
    fn view(&self) -> Result<SpaceView, String>;
    /// Closes a space with all its terminals.
    fn close_space(&self, id: &str) -> Result<(), String>;
}

/// Runs the project's teardown in a worktree; `Err` carries the text to show.
pub type Teardown<'a> = dyn Fn(&Path) -> Result<(), String> + 'a;

/// Cleans up the worktrees of the repository at `root` (its main worktree) whose branch is the
/// head of a merged pull request. `focused_space` is never touched. Spaces with terminals in a
/// worktree are closed through `spaces` before it is removed.
pub fn cleanup(
    root: &Path,
    merged: &[MergedPr],
    mode: &CleanupMode,
    spaces: &dyn Spaces,
    focused_space: Option<&str>,
    teardown: Option<&Teardown<'_>>,
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
        let view = spaces.view()?;
        let (whole, partial) = spaces_in(&view, Path::new(&path));
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
        match git(Path::new(&path), &["status", "--porcelain"]) {
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
        }
        // The project's teardown runs in the worktree before anything is closed or removed; a
        // failure (or timeout) leaves the worktree with the output as the reason.
        if let Some(teardown) = teardown
            && let Err(reason) = teardown(Path::new(&path))
        {
            report.left.push(left(reason));
            continue;
        }
        match mode {
            CleanupMode::Builtin => {}
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
            if let Err(e) = spaces.close_space(space) {
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
                let command = crate::tasks::expand_command(template, &vars);
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
                    Err(e) => report.left.push(Left {
                        command: command.line(),
                        ..left(e)
                    }),
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
            // Windows canonicalizes to `\\?\C:\...`, which git reads as a host name.
            let base = PathBuf::from(
                base.canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .trim_start_matches(r"\\?\"),
            );
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

    use serde_json::json;
    use std::sync::Mutex;

    /// Records `close_space` calls; `fail` makes them fail with `boom`.
    #[derive(Default)]
    struct FakeBackend {
        snapshot: Mutex<SpaceView>,
        calls: Mutex<Vec<String>>,
        fail: Mutex<bool>,
    }

    impl FakeBackend {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
        fn fail_on(&self, _prefix: &str) {
            *self.fail.lock().unwrap() = true;
        }
    }

    impl Spaces for FakeBackend {
        fn view(&self) -> Result<SpaceView, String> {
            let view = self.snapshot.lock().unwrap().clone();
            Ok(SpaceView {
                panes: if view.panes.is_null() {
                    json!([])
                } else {
                    view.panes
                },
                workspaces: if view.workspaces.is_null() {
                    json!([])
                } else {
                    view.workspaces
                },
            })
        }
        fn close_space(&self, id: &str) -> Result<(), String> {
            if *self.fail.lock().unwrap() {
                return Err("boom".into());
            }
            self.calls.lock().unwrap().push(format!("close_space {id}"));
            Ok(())
        }
    }

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
            cleanup(
                &self.repo,
                merged,
                &CleanupMode::Builtin,
                backend,
                focused,
                None,
            )
            .unwrap()
        }
    }

    fn panes_in(backend: &FakeBackend, panes: &[(&str, &Path)]) {
        let list: Vec<_> = panes
            .iter()
            .map(|(space, dir)| json!({"workspace_id": space, "cwd": dir.to_str().unwrap()}))
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

    /// A snapshot of the poimen backend: spaces in `(id, dir)` without any terminal.
    fn empty_spaces_in(backend: &FakeBackend, spaces: &[(&str, &Path)]) {
        let list: Vec<_> = spaces
            .iter()
            .map(|(id, dir)| json!({"workspace_id": id, "cwd": dir.to_str().unwrap()}))
            .collect();
        backend.snapshot.lock().unwrap().workspaces = json!(list);
    }

    #[test]
    fn a_space_without_terminals_in_the_worktree_is_closed() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let backend = FakeBackend::default();
        empty_spaces_in(&backend, &[("s2", &wt), ("s1", &f.repo)]);
        let report = f.clean(&Fixture::merged(&[7]), &backend, None);
        assert_eq!(report.removed.len(), 1, "{report:?}");
        let closes: Vec<_> = backend
            .calls()
            .into_iter()
            .filter(|c| c.starts_with("close_space"))
            .collect();
        assert_eq!(closes, vec!["close_space s2".to_owned()]);
    }

    #[test]
    fn a_space_in_the_worktree_with_terminals_outside_is_not_closed() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let backend = FakeBackend::default();
        empty_spaces_in(&backend, &[("s2", &wt)]);
        panes_in(&backend, &[("s2", &f.repo)]);
        f.clean(&Fixture::merged(&[7]), &backend, None);
        assert!(!backend.calls().iter().any(|c| c.starts_with("close_space")));
    }

    #[test]
    fn a_focused_space_without_terminals_keeps_its_worktree() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let backend = FakeBackend::default();
        empty_spaces_in(&backend, &[("s2", &wt)]);
        let report = f.clean(&Fixture::merged(&[7]), &backend, Some("s2"));
        assert!(wt.exists());
        assert!(report.left[0].reason.contains("focused"));
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
    fn teardown_runs_in_the_worktree_before_the_removal() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let seen = std::cell::RefCell::new(vec![]);
        let teardown = |dir: &Path| -> Result<(), String> {
            seen.borrow_mut().push((dir.to_path_buf(), dir.exists()));
            Ok(())
        };
        let report = cleanup(
            &f.repo,
            &Fixture::merged(&[7]),
            &CleanupMode::Builtin,
            &FakeBackend::default(),
            None,
            Some(&teardown),
        )
        .unwrap();
        assert_eq!(report.removed.len(), 1, "{report:?}");
        assert_eq!(*seen.borrow(), vec![(canonical(&wt), true)]);
        assert!(!wt.exists());
    }

    #[test]
    fn a_failing_teardown_stops_the_removal_and_shows_its_output() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let backend = FakeBackend::default();
        panes_in(&backend, &[("w2", &wt)]);
        let teardown = |_: &Path| -> Result<(), String> {
            Err("the teardown command failed (exit 1)\nboom".into())
        };
        let report = cleanup(
            &f.repo,
            &Fixture::merged(&[7]),
            &CleanupMode::Builtin,
            &backend,
            None,
            Some(&teardown),
        )
        .unwrap();
        assert!(report.removed.is_empty());
        assert_eq!(
            report.left[0].reason,
            "the teardown command failed (exit 1)\nboom"
        );
        assert!(wt.exists());
        assert!(f.branches().contains("feat/7-x"));
        assert!(!backend.calls().iter().any(|c| c.starts_with("close_space")));
    }

    #[test]
    fn teardown_does_not_run_for_a_worktree_that_is_left_anyway() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        std::fs::write(wt.join("a.txt"), "changed").unwrap();
        let ran = std::cell::Cell::new(false);
        let teardown = |_: &Path| -> Result<(), String> {
            ran.set(true);
            Ok(())
        };
        let report = cleanup(
            &f.repo,
            &Fixture::merged(&[7]),
            &CleanupMode::Builtin,
            &FakeBackend::default(),
            None,
            Some(&teardown),
        )
        .unwrap();
        assert_eq!(report.left.len(), 1);
        assert!(!ran.get());
    }

    #[test]
    fn a_cleanup_command_replaces_the_built_in_removal() {
        let f = Fixture::new(true);
        let wt = f.task(7);
        let mode = CleanupMode::Command(crate::tasks::CommandSpec::Shell(
            "printf '%s-%s-%s' {pr} {nr} {branch} > cleanup.out".into(),
        ));
        let report = cleanup(
            &f.repo,
            &Fixture::merged(&[7]),
            &mode,
            &FakeBackend::default(),
            None,
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
    fn an_argv_cleanup_runs_without_a_shell_and_each_element_stays_one_argument() {
        let f = Fixture::new(true);
        f.task(7);
        let mode = CleanupMode::Command(crate::tasks::CommandSpec::Argv(
            [
                "sh",
                "-c",
                "printf '%s|%s' \"$1\" \"$2\" > cleanup.out",
                "sh",
                "pr {pr};$(x)",
                "{branch}",
            ]
            .map(String::from)
            .to_vec(),
        ));
        let report = cleanup(
            &f.repo,
            &Fixture::merged(&[7]),
            &mode,
            &FakeBackend::default(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(report.removed.len(), 1, "{report:?}");
        assert_eq!(
            std::fs::read_to_string(f.repo.join("cleanup.out")).unwrap(),
            "pr 107;$(x)|feat/7-x"
        );
    }

    #[test]
    fn a_failing_cleanup_command_leaves_the_worktree_with_its_command() {
        let f = Fixture::new(true);
        f.task(7);
        let mode = CleanupMode::Command(crate::tasks::CommandSpec::Shell(
            "echo boom >&2; exit 3 # {pr}".into(),
        ));
        let report = cleanup(
            &f.repo,
            &Fixture::merged(&[7]),
            &mode,
            &FakeBackend::default(),
            None,
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
    fn a_dirty_worktree_is_left_in_command_mode_and_the_command_does_not_run() {
        for untracked in [false, true] {
            let f = Fixture::new(true);
            let wt = f.task(7);
            let file = if untracked { "new.txt" } else { "a.txt" };
            std::fs::write(wt.join(file), "changed").unwrap();
            let backend = FakeBackend::default();
            panes_in(&backend, &[("w2", &wt)]);
            let report = cleanup(
                &f.repo,
                &Fixture::merged(&[7]),
                &CleanupMode::Command(crate::tasks::CommandSpec::Shell("touch ran".into())),
                &backend,
                None,
                None,
            )
            .unwrap();
            assert!(report.removed.is_empty(), "{report:?}");
            assert_eq!(report.left.len(), 1);
            assert!(report.left[0].reason.contains("uncommitted"));
            assert!(!f.repo.join("ran").exists(), "the command must not run");
            assert!(!backend.calls().iter().any(|c| c.starts_with("close_space")));
            assert!(wt.join(file).exists());
        }
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
            &CleanupMode::Command(crate::tasks::CommandSpec::Shell("touch ran".into())),
            &FakeBackend::default(),
            None,
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
    // --- pull request worktrees -------------------------------------------------------------

    impl Fixture {
        /// Pushes a commit that is on no branch to `refs/pull/<nr>/head` (a fork-style head).
        fn fork_pull(&self, nr: u64, file: &str) -> String {
            let seed = self.base.join("seed");
            run(&seed, &["checkout", "-q", "--detach", "main"]);
            std::fs::write(seed.join(file), "x").unwrap();
            run(&seed, &["add", "."]);
            run(&seed, &["commit", "-m", "fork change"]);
            let sha = run(&seed, &["rev-parse", "HEAD"]);
            run(
                &seed,
                &["push", "origin", &format!("HEAD:refs/pull/{nr}/head")],
            );
            sha
        }

        /// Pushes branch `name` with a commit and points `refs/pull/<nr>/head` at it.
        fn own_pull(&self, nr: u64, name: &str) {
            let seed = self.base.join("seed");
            run(&seed, &["checkout", "-q", "-B", name, "main"]);
            std::fs::write(seed.join("own.txt"), "own").unwrap();
            run(&seed, &["add", "."]);
            run(&seed, &["commit", "-m", "own change"]);
            run(&seed, &["push", "origin", name]);
            run(
                &seed,
                &["push", "origin", &format!("HEAD:refs/pull/{nr}/head")],
            );
        }

        fn pr(&self, dir: &Path, nr: u64, head: Option<&str>) -> Result<Worktree, String> {
            pr_worktree(dir.to_str().unwrap(), nr, head)
        }
    }

    #[test]
    fn pr_worktree_of_a_fork_head_uses_pr_branch() {
        let f = Fixture::new(false);
        let sha = f.fork_pull(1, "fork.txt");
        let wt = f.pr(&f.repo, 1, Some("main")).unwrap();
        assert_eq!(wt.branch, "pr/1");
        assert!(wt.created);
        assert_eq!(PathBuf::from(&wt.path), f.base.join("app-pr1"));
        assert!(f.base.join("app-pr1/fork.txt").exists());
        assert_eq!(run(&f.base.join("app-pr1"), &["rev-parse", "HEAD"]), sha);
    }

    #[test]
    fn pr_worktree_of_an_own_pull_request_uses_its_head_branch() {
        let f = Fixture::new(false);
        f.own_pull(2, "feat/own");
        let wt = f.pr(&f.repo, 2, Some("feat/own")).unwrap();
        assert_eq!(wt.branch, "feat/own");
        assert!(f.base.join("app-pr2/own.txt").exists());
        assert_eq!(
            run(
                &f.base.join("app-pr2"),
                &["rev-parse", "--abbrev-ref", "feat/own@{upstream}"]
            ),
            "origin/feat/own"
        );
    }

    #[test]
    fn pr_worktree_reuses_a_worktree_with_the_branch_and_changes_nothing() {
        let f = Fixture::new(false);
        f.fork_pull(1, "fork.txt");
        let first = f.pr(&f.repo, 1, None).unwrap();
        let wt_dir = PathBuf::from(&first.path);
        std::fs::write(wt_dir.join("dirty.txt"), "wip").unwrap();
        let head = run(&wt_dir, &["rev-parse", "HEAD"]);
        let again = f.pr(&f.repo, 1, None).unwrap();
        assert!(!again.created);
        assert_eq!(again.path, first.path);
        assert_eq!(again.branch, "pr/1");
        assert_eq!(run(&wt_dir, &["rev-parse", "HEAD"]), head);
        assert_eq!(
            std::fs::read_to_string(wt_dir.join("dirty.txt")).unwrap(),
            "wip"
        );
    }

    #[test]
    fn pr_worktree_returns_the_worktree_where_the_head_branch_is_checked_out() {
        let f = Fixture::new(false);
        f.own_pull(2, "feat/own");
        run(&f.repo, &["fetch", "origin"]);
        let elsewhere = f.base.join("elsewhere");
        run(
            &f.repo,
            &[
                "worktree",
                "add",
                "--track",
                "-b",
                "feat/own",
                elsewhere.to_str().unwrap(),
                "origin/feat/own",
            ],
        );
        let wt = f.pr(&f.repo, 2, Some("feat/own")).unwrap();
        assert!(!wt.created);
        assert_eq!(PathBuf::from(&wt.path), elsewhere);
        assert!(!f.base.join("app-pr2").exists());
    }

    #[test]
    fn pr_worktree_errors_naming_a_colliding_path() {
        let f = Fixture::new(false);
        f.fork_pull(1, "fork.txt");
        let taken = f.base.join("app-pr1");
        std::fs::create_dir_all(&taken).unwrap();
        std::fs::write(taken.join("mine.txt"), "mine").unwrap();
        let err = f.pr(&f.repo, 1, None).unwrap_err();
        assert!(err.contains(taken.to_str().unwrap()), "{err}");
        assert_eq!(
            std::fs::read_to_string(taken.join("mine.txt")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn pr_worktree_works_from_a_linked_worktree() {
        let f = Fixture::new(true);
        f.fork_pull(1, "fork.txt");
        let linked = f.base.join("linked");
        run(
            &f.repo,
            &[
                "worktree",
                "add",
                "-b",
                "other",
                linked.to_str().unwrap(),
                "origin/release/next",
            ],
        );
        let wt = f.pr(&linked, 1, None).unwrap();
        assert_eq!(PathBuf::from(&wt.path), f.base.join("app-pr1"));
        assert!(f.base.join("app-pr1/fork.txt").exists());
    }

    #[test]
    fn pr_worktree_keeps_a_local_branch_with_its_own_commits() {
        let f = Fixture::new(false);
        let sha = f.fork_pull(1, "fork.txt");
        run(&f.repo, &["branch", "pr/1", "main"]);
        run(&f.repo, &["checkout", "-q", "pr/1"]);
        std::fs::write(f.repo.join("mine.txt"), "m").unwrap();
        run(&f.repo, &["add", "."]);
        run(&f.repo, &["commit", "-m", "local"]);
        let mine = run(&f.repo, &["rev-parse", "HEAD"]);
        run(&f.repo, &["checkout", "-q", "main"]);
        assert_ne!(mine, sha);
        f.pr(&f.repo, 1, None).unwrap();
        assert_eq!(run(&f.repo, &["rev-parse", "pr/1"]), mine);
    }

    #[test]
    fn pr_worktree_moves_an_old_local_branch_forward() {
        let f = Fixture::new(false);
        let sha = f.fork_pull(1, "fork.txt");
        run(&f.repo, &["branch", "pr/1", "main"]);
        f.pr(&f.repo, 1, None).unwrap();
        assert_eq!(run(&f.repo, &["rev-parse", "pr/1"]), sha);
    }

    #[test]
    fn pr_worktree_errors_for_a_missing_pull_request() {
        let f = Fixture::new(false);
        let err = f.pr(&f.repo, 99, None).unwrap_err();
        assert!(err.contains("#99"), "{err}");
    }

    #[test]
    fn lists_worktrees_with_branches() {
        let f = Fixture::new(false);
        f.fork_pull(1, "fork.txt");
        f.pr(&f.repo, 1, None).unwrap();
        let list = list_worktrees(f.repo.to_str().unwrap()).unwrap();
        assert_eq!(list.len(), 2);
        assert!(list.iter().any(|c| c.branch.as_deref() == Some("pr/1")));
    }
}
