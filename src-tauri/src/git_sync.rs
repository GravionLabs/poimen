//! Fetch and pull from the folder view: `fetch` (`git fetch --prune`) and `pull` (`git pull
//! --ff-only`, refused with a dirty tree, a detached HEAD, no upstream or a diverged branch, with
//! the reason) of one repository, and `fetch_all` for every repository below a directory
//! (`discover`: depth limit, no `node_modules`, no worktrees under `.claude/worktrees`), run in
//! parallel with a limit. All git through argv, never prompting (`changes_git::run_git`).

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde::Serialize;

use crate::changes_git::run_git;
use crate::git::run_bytes;
use crate::spaces::resolve_dir;

/// Longest a fetch or pull may take.
const TIMEOUT: Duration = Duration::from_secs(120);
/// How deep below the chosen directory repositories are looked for.
pub const MAX_DEPTH: usize = 4;
/// Repositories fetched at the same time.
pub const PARALLEL: usize = 4;

/// What happened to one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Outcome {
    /// Refs changed.
    Updated,
    /// Nothing new.
    Current,
    /// Failed or refused, with the reason.
    Failed { reason: String },
}

/// The result for one repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoResult {
    pub path: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// Summary of a run over several repositories.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub results: Vec<RepoResult>,
    pub updated: usize,
    pub current: usize,
    pub failed: usize,
}

/// Counts the outcomes.
pub fn summarize(results: Vec<RepoResult>) -> Summary {
    let count = |f: fn(&Outcome) -> bool| results.iter().filter(|r| f(&r.outcome)).count();
    Summary {
        updated: count(|o| *o == Outcome::Updated),
        current: count(|o| *o == Outcome::Current),
        failed: count(|o| matches!(o, Outcome::Failed { .. })),
        results,
    }
}

/// Directories that are not searched.
fn skipped(parent: &Path, name: &str) -> bool {
    name == "node_modules"
        || name == ".git"
        || (name == "worktrees" && parent.file_name().is_some_and(|n| n == ".claude"))
}

/// The git repositories at or below `root` (a `.git` directory; linked worktrees have a `.git`
/// file and are not listed), sorted; a repository is not searched further. Symlinks are not followed.
pub fn discover(root: &Path, max_depth: usize) -> Vec<String> {
    fn walk(dir: &Path, depth: usize, max: usize, out: &mut Vec<String>) {
        if dir.join(".git").is_dir() {
            out.push(dir.to_string_lossy().into_owned());
            return;
        }
        if depth >= max {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            let name = e.file_name().to_string_lossy().into_owned();
            if kind.is_dir() && !skipped(dir, &name) {
                walk(&e.path(), depth + 1, max, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, 0, max_depth, &mut out);
    out.sort();
    out
}

/// All refs with their ids; differs before and after a fetch exactly when something was updated.
fn refs(top: &str) -> Result<String, String> {
    let out = run_bytes(top, &["for-each-ref", "--format=%(objectname) %(refname)"])?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

fn fetch_in(top: &str) -> Result<Outcome, String> {
    let before = refs(top)?;
    run_git(top, &["fetch", "--prune"], TIMEOUT, "fetch")?;
    Ok(if refs(top)? == before {
        Outcome::Current
    } else {
        Outcome::Updated
    })
}

/// State of a repository that decides whether a pull may run.
#[derive(Debug, Clone, Default)]
pub struct PullState {
    pub dirty: bool,
    pub branch: Option<String>,
    pub has_upstream: bool,
    pub ahead: u32,
    pub behind: u32,
}

/// Why a pull is refused, `None` when it may run.
pub fn pull_refusal(s: &PullState) -> Option<String> {
    if s.dirty {
        return Some(
            "The working tree has uncommitted changes; commit or stash them first.".into(),
        );
    }
    let Some(branch) = &s.branch else {
        return Some("HEAD is detached; check out a branch first.".into());
    };
    if !s.has_upstream {
        return Some(format!("Branch {branch} has no upstream to pull from."));
    }
    if s.ahead > 0 && s.behind > 0 {
        return Some(format!(
            "Branch {branch} has diverged from its upstream ({} ahead, {} behind); rebase or merge by hand.",
            s.ahead, s.behind
        ));
    }
    None
}

fn pull_state(top: &str) -> Result<PullState, String> {
    let dirty = !run_bytes(top, &["status", "--porcelain", "--untracked-files=no"])?.is_empty();
    let branch = run_bytes(top, &["symbolic-ref", "--quiet", "--short", "HEAD"])
        .ok()
        .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
        .filter(|b| !b.is_empty());
    let counts = run_bytes(
        top,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    )
    .ok();
    let (ahead, behind) = counts
        .as_deref()
        .map(|c| String::from_utf8_lossy(c).into_owned())
        .and_then(|c| {
            let mut it = c.split_whitespace().map(|n| n.parse::<u32>().ok());
            Some((it.next()??, it.next()??))
        })
        .unwrap_or((0, 0));
    Ok(PullState {
        dirty,
        branch,
        has_upstream: counts.is_some(),
        ahead,
        behind,
    })
}

fn pull_in(top: &str) -> Result<Outcome, String> {
    // Fetch first so "diverged" is judged against the current upstream.
    let _ = run_git(top, &["fetch", "--prune"], TIMEOUT, "fetch");
    if let Some(reason) = pull_refusal(&pull_state(top)?) {
        return Err(reason);
    }
    let before = run_bytes(top, &["rev-parse", "HEAD"])?;
    run_git(top, &["pull", "--ff-only"], TIMEOUT, "pull")?;
    Ok(if run_bytes(top, &["rev-parse", "HEAD"])? == before {
        Outcome::Current
    } else {
        Outcome::Updated
    })
}

fn repo_top(dir: &str, home: Option<&Path>) -> Result<String, String> {
    let dir = resolve_dir(dir, home)?;
    crate::git::toplevel(&dir).ok_or_else(|| format!("{dir} is not inside a git repository"))
}

/// `git fetch --prune` of the repository containing `dir`.
pub fn fetch(dir: &str, home: Option<&Path>) -> Result<Outcome, String> {
    fetch_in(&repo_top(dir, home)?)
}

/// `git pull --ff-only` of the repository containing `dir`; `Err` carries the reason of a refusal.
pub fn pull(dir: &str, home: Option<&Path>) -> Result<Outcome, String> {
    pull_in(&repo_top(dir, home)?)
}

/// Fetches every repository found below `dir`, `PARALLEL` at a time; `progress(done, total, path)`
/// is called after each one.
pub fn fetch_all(
    dir: &str,
    home: Option<&Path>,
    progress: impl Fn(usize, usize, &str) + Sync,
) -> Result<Summary, String> {
    let root = resolve_dir(dir, home)?;
    let repos = discover(Path::new(&root), MAX_DEPTH);
    let total = repos.len();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(total));
    std::thread::scope(|s| {
        for _ in 0..PARALLEL.min(total) {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    let Some(path) = repos.get(i) else { break };
                    let outcome =
                        fetch_in(path).unwrap_or_else(|reason| Outcome::Failed { reason });
                    results.lock().unwrap().push(RepoResult {
                        path: path.clone(),
                        outcome,
                    });
                    progress(done.fetch_add(1, Ordering::SeqCst) + 1, total, path);
                }
            });
        }
    });
    let mut results = results.into_inner().unwrap();
    results.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(summarize(results))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU32;

    static N: AtomicU32 = AtomicU32::new(0);

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = crate::gui_path::command("git")
            .arg("-C")
            .arg(dir)
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

    struct Tmp(PathBuf);
    impl Tmp {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "poimen-git-sync-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Self(p.canonicalize().unwrap())
        }
    }
    impl Drop for Tmp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn commit(dir: &Path, file: &str) {
        std::fs::write(dir.join(file), file).unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", file]);
    }

    fn configure(dir: &Path) {
        git(dir, &["config", "user.name", "t"]);
        git(dir, &["config", "user.email", "t@example.com"]);
        git(dir, &["config", "commit.gpgsign", "false"]);
    }

    /// Bare `origin.git`, clone `app` tracking `origin/main`, clone `other` to push from.
    fn setup(t: &Tmp) -> (PathBuf, PathBuf) {
        let b = &t.0;
        git(b, &["init", "--bare", "-b", "main", "origin.git"]);
        git(b, &["init", "-b", "main", "other"]);
        let other = b.join("other");
        configure(&other);
        git(
            &other,
            &[
                "remote",
                "add",
                "origin",
                b.join("origin.git").to_str().unwrap(),
            ],
        );
        commit(&other, "a.txt");
        git(&other, &["push", "-u", "origin", "main"]);
        git(b, &["clone", "origin.git", "app"]);
        let app = b.join("app");
        configure(&app);
        (app, other)
    }

    fn s(p: &Path) -> &str {
        p.to_str().unwrap()
    }

    #[test]
    fn discovers_repositories_with_limits() {
        let t = Tmp::new();
        let r = &t.0;
        for d in [
            "a/.git",
            "group/b/.git",
            "node_modules/x/.git",
            ".claude/worktrees/w/.git",
            "a/inner/.git",
            "d1/d2/d3/d4/d5/deep/.git",
            "d1/d2/ok/.git",
        ] {
            std::fs::create_dir_all(r.join(d)).unwrap();
        }
        // A linked worktree has a `.git` file.
        std::fs::create_dir_all(r.join("linked")).unwrap();
        std::fs::write(r.join("linked/.git"), "gitdir: /x").unwrap();
        let found: Vec<PathBuf> = discover(r, MAX_DEPTH)
            .iter()
            .map(|p| PathBuf::from(p).strip_prefix(r).unwrap().to_owned())
            .collect();
        assert_eq!(
            found,
            vec![
                PathBuf::from("a"),
                PathBuf::from("d1/d2/ok"),
                PathBuf::from("group/b")
            ]
        );
    }

    #[test]
    fn discover_of_nothing_is_empty_and_of_a_repository_is_itself() {
        let t = Tmp::new();
        assert!(discover(&t.0, MAX_DEPTH).is_empty());
        std::fs::create_dir_all(t.0.join(".git")).unwrap();
        assert_eq!(discover(&t.0, MAX_DEPTH), vec![s(&t.0).to_owned()]);
    }

    #[test]
    fn refusal_rules() {
        let ok = PullState {
            branch: Some("main".into()),
            has_upstream: true,
            behind: 2,
            ..Default::default()
        };
        assert_eq!(pull_refusal(&ok), None);
        let dirty = PullState {
            dirty: true,
            ..ok.clone()
        };
        assert!(pull_refusal(&dirty).unwrap().contains("uncommitted"));
        let detached = PullState {
            branch: None,
            ..ok.clone()
        };
        assert!(pull_refusal(&detached).unwrap().contains("detached"));
        let none = PullState {
            has_upstream: false,
            ..ok.clone()
        };
        assert!(pull_refusal(&none).unwrap().contains("no upstream"));
        let diverged = PullState {
            ahead: 1,
            ..ok.clone()
        };
        assert!(pull_refusal(&diverged).unwrap().contains("diverged"));
        let ahead = PullState {
            ahead: 3,
            behind: 0,
            ..ok
        };
        assert_eq!(pull_refusal(&ahead), None);
    }

    #[test]
    fn summary_counts_outcomes() {
        let r = |p: &str, outcome| RepoResult {
            path: p.into(),
            outcome,
        };
        let sum = summarize(vec![
            r("a", Outcome::Updated),
            r("b", Outcome::Current),
            r("c", Outcome::Failed { reason: "x".into() }),
            r("d", Outcome::Updated),
        ]);
        assert_eq!((sum.updated, sum.current, sum.failed), (2, 1, 1));
        assert_eq!(sum.results.len(), 4);
    }

    #[test]
    fn fetch_reports_updated_then_current() {
        let t = Tmp::new();
        let (app, other) = setup(&t);
        assert_eq!(fetch_in(s(&app)).unwrap(), Outcome::Current);
        commit(&other, "b.txt");
        git(&other, &["push"]);
        assert_eq!(fetch_in(s(&app)).unwrap(), Outcome::Updated);
        assert_eq!(fetch_in(s(&app)).unwrap(), Outcome::Current);
    }

    #[test]
    fn pull_fast_forwards() {
        let t = Tmp::new();
        let (app, other) = setup(&t);
        commit(&other, "b.txt");
        git(&other, &["push"]);
        assert_eq!(pull_in(s(&app)).unwrap(), Outcome::Updated);
        assert!(app.join("b.txt").exists());
        assert_eq!(pull_in(s(&app)).unwrap(), Outcome::Current);
    }

    #[test]
    fn pull_refuses_a_dirty_tree_and_a_diverged_branch() {
        let t = Tmp::new();
        let (app, other) = setup(&t);
        commit(&other, "b.txt");
        git(&other, &["push"]);
        std::fs::write(app.join("a.txt"), "changed").unwrap();
        let err = pull_in(s(&app)).unwrap_err();
        assert!(err.contains("uncommitted"), "{err}");
        assert!(!app.join("b.txt").exists());
        git(&app, &["checkout", "--", "a.txt"]);
        commit(&app, "c.txt");
        let err = pull_in(s(&app)).unwrap_err();
        assert!(err.contains("diverged"), "{err}");
    }

    #[test]
    fn fetch_all_runs_every_repository_and_reports_failures() {
        let t = Tmp::new();
        let (app, other) = setup(&t);
        commit(&other, "b.txt");
        git(&other, &["push"]);
        let broken = t.0.join("group").join("broken");
        std::fs::create_dir_all(&broken).unwrap();
        git(&broken, &["init", "-b", "main"]);
        git(
            &broken,
            &["remote", "add", "origin", "/nonexistent/repo.git"],
        );
        let calls = AtomicUsize::new(0);
        let sum = fetch_all(s(&t.0), None, |_, total, _| {
            assert_eq!(total, 3);
            calls.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
        // origin.git is bare (no `.git` directory), so app, other and broken are found.
        assert_eq!(sum.results.len(), 3);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(sum.failed, 1);
        assert_eq!(sum.updated + sum.current + sum.failed, 3);
        let failed = sum.results.iter().find(|r| r.path == s(&broken)).unwrap();
        assert!(matches!(failed.outcome, Outcome::Failed { .. }));
        assert!(
            sum.results
                .iter()
                .any(|r| r.path == s(&app) && r.outcome == Outcome::Updated)
        );
    }
}
