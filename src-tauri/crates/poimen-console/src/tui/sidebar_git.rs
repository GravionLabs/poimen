//! The repository of each space for the sidebar: the worktree groups and the `branch`, `dirty`,
//! `sync` and `pr` tokens (#1174). Every read (`git`, `gh`) runs on a background thread and is
//! cached: the render thread only looks at what is there. A directory is read again after
//! [`GIT_TTL`], a pull request after [`PR_TTL`]; a directory that is no repository here (a remote
//! space) stays empty, so its tokens do.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

/// What the sidebar needs of a directory's repository.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RepoRef {
    pub common_dir: String,
    /// True for a linked worktree, false for the main checkout.
    pub worktree: bool,
    pub branch: Option<String>,
    /// Changed plus untracked files.
    pub dirty: Option<u32>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    /// The branch is the base branch (hidden unless `sidebar.showBaseBranch`).
    pub on_base: bool,
    /// The text of the `pr` token; empty without a pull request or `gh`.
    pub pr: Option<String>,
}

/// How long a read of a directory counts.
pub const GIT_TTL: Duration = Duration::from_secs(5);
/// How long a pull request lookup counts (it is a network call).
pub const PR_TTL: Duration = Duration::from_secs(60);

/// The two reads, replaceable in tests.
#[derive(Clone, Copy)]
pub struct Readers {
    pub repo: fn(&str) -> Option<RepoRef>,
    /// The `pr` token text of a branch in a directory.
    pub pr: fn(&str, &str) -> Option<String>,
}

impl Default for Readers {
    fn default() -> Self {
        Self {
            repo: git_lookup,
            pr: gh_pr_text,
        }
    }
}

fn git_lookup(dir: &str) -> Option<RepoRef> {
    poimen_core::git::info(dir).map(|i| RepoRef {
        common_dir: i.common_dir,
        worktree: i.worktree,
        branch: i.branch,
        dirty: i.dirty,
        ahead: i.ahead,
        behind: i.behind,
        on_base: i.on_base,
        pr: None,
    })
}

struct Entry {
    at: Instant,
    value: Option<RepoRef>,
}

#[derive(Default)]
struct Inner {
    repos: HashMap<String, Entry>,
    /// Pull request text per `(directory, branch)`.
    prs: HashMap<(String, String), (Instant, Option<String>)>,
    reading: bool,
}

/// Repository info per directory, read in the background; a failed read (no repository, or a
/// directory missing here) stays `None`.
pub struct RepoCache {
    inner: Arc<Mutex<Inner>>,
    readers: Readers,
    changed: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl Default for RepoCache {
    fn default() -> Self {
        Self::with_readers(Readers::default())
    }
}

fn lock(m: &Mutex<Inner>) -> std::sync::MutexGuard<'_, Inner> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl RepoCache {
    pub fn with_readers(readers: Readers) -> Self {
        Self {
            inner: Arc::default(),
            readers,
            changed: None,
        }
    }

    /// Calls `f` from the reading thread when a read changed what the cache holds.
    pub fn on_change(&mut self, f: impl Fn() + Send + Sync + 'static) {
        self.changed = Some(Arc::new(f));
    }

    /// What is known of `dirs` now; a directory never read, or read longer than [`GIT_TTL`] ago,
    /// is read on a background thread (one at a time) and shows with the next call.
    pub fn resolve<'a>(&self, dirs: impl Iterator<Item = &'a str>) -> HashMap<String, RepoRef> {
        let mut out = HashMap::new();
        let mut stale = Vec::new();
        let mut inner = lock(&self.inner);
        for dir in dirs {
            match inner.repos.get(dir) {
                Some(e) => {
                    if e.at.elapsed() >= GIT_TTL {
                        stale.push(dir.to_owned());
                    }
                    if let Some(r) = &e.value {
                        let mut r = r.clone();
                        if let Some(b) = &r.branch {
                            r.pr = inner
                                .prs
                                .get(&(dir.to_owned(), b.clone()))
                                .and_then(|(_, t)| t.clone());
                        }
                        out.insert(dir.to_owned(), r);
                    }
                }
                None => stale.push(dir.to_owned()),
            }
        }
        if !stale.is_empty() && !inner.reading {
            inner.reading = true;
            drop(inner);
            self.spawn_read(stale);
        }
        out
    }

    fn spawn_read(&self, dirs: Vec<String>) {
        let inner = Arc::clone(&self.inner);
        let readers = self.readers;
        let changed = self.changed.clone();
        std::thread::spawn(move || {
            let any = read_dirs(&inner, readers, dirs);
            lock(&inner).reading = false;
            if any && let Some(f) = changed {
                f();
            }
        });
    }

    /// [`RepoCache::resolve`] that reads what is missing or old on this thread first.
    pub fn resolve_now<'a>(
        &self,
        dirs: impl Iterator<Item = &'a str> + Clone,
    ) -> HashMap<String, RepoRef> {
        read_dirs(
            &self.inner,
            self.readers,
            dirs.clone().map(str::to_owned).collect(),
        );
        self.resolve(dirs)
    }
}

/// Reads `dirs` (and the pull requests of their branches when due) into the cache; true when
/// something changed.
fn read_dirs(inner: &Mutex<Inner>, readers: Readers, dirs: Vec<String>) -> bool {
    let mut any = false;
    let mut seen = HashSet::new();
    for dir in dirs.into_iter().filter(|d| seen.insert(d.clone())) {
        let value = (readers.repo)(&dir);
        let branch = value.as_ref().and_then(|r| {
            let b = r.branch.clone()?;
            (!r.on_base && !skipped(&b)).then_some(b)
        });
        let due = branch.filter(|b| {
            lock(inner)
                .prs
                .get(&(dir.clone(), b.clone()))
                .is_none_or(|(at, _)| at.elapsed() >= PR_TTL)
        });
        let pr = due.map(|b| {
            let text = (readers.pr)(&dir, &b);
            (b, text)
        });
        let mut g = lock(inner);
        let before = g.repos.get(&dir).map(|e| e.value.clone());
        // Absent to `None` is no change: a remote space stays as it was.
        any |= match &before {
            Some(old) => *old != value,
            None => value.is_some(),
        };
        g.repos.insert(
            dir.clone(),
            Entry {
                at: Instant::now(),
                value,
            },
        );
        if let Some((b, text)) = pr {
            let key = (dir.clone(), b);
            any |= g.prs.get(&key).map(|(_, t)| t) != Some(&text);
            g.prs.insert(key, (Instant::now(), text));
        }
    }
    any
}

/// Branches that never get a pull request lookup: the trunk and release branches (as the app).
pub fn skipped(branch: &str) -> bool {
    matches!(branch, "main" | "master") || branch.starts_with("release/")
}

/// `●3` for three changed or untracked files; empty when clean or unknown.
pub fn dirty_text(dirty: Option<u32>) -> String {
    dirty
        .filter(|d| *d > 0)
        .map_or_else(String::new, |d| format!("●{d}"))
}

/// `↑2 ↓1`; empty when even, without upstream or unknown.
pub fn sync_text(ahead: Option<u32>, behind: Option<u32>) -> String {
    [
        ahead.filter(|n| *n > 0).map(|n| format!("↑{n}")),
        behind.filter(|n| *n > 0).map(|n| format!("↓{n}")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
}

/// The repositories a row's tokens read from.
#[derive(Clone, Copy, Default)]
pub struct GitView<'a> {
    /// The repository of the row's directory.
    pub own: Option<&'a RepoRef>,
    /// On a terminal row: the space's, which the row only shows what differs from.
    pub space: Option<&'a RepoRef>,
    pub show_base_branch: bool,
}

impl GitView<'_> {
    /// The text of `branch`, `dirty`, `sync` or `pr`; `None` for other tokens and for no value.
    pub fn token(&self, token: &str) -> Option<String> {
        let own = self.own?;
        let text = match token {
            "branch" => {
                let hidden = self.space.is_none() && own.on_base && !self.show_base_branch;
                let b = own.branch.as_deref().filter(|_| !hidden)?;
                format!("{}{b}", if own.worktree { "wt " } else { "" })
            }
            "dirty" => dirty_text(own.dirty),
            "sync" => sync_text(own.ahead, own.behind),
            "pr" => own.pr.clone().unwrap_or_default(),
            _ => return None,
        };
        let shared = self.space.map(|s| match token {
            "branch" => s
                .branch
                .clone()
                .map(|b| format!("{}{b}", if s.worktree { "wt " } else { "" })),
            "dirty" => Some(dirty_text(s.dirty)),
            "sync" => Some(sync_text(s.ahead, s.behind)),
            _ => s.pr.clone(),
        });
        // A terminal row shows what differs from its space.
        (shared.flatten().as_deref() != Some(text.as_str()) || self.space.is_none()).then_some(text)
    }
}

/// `#123 ✓`, `#123 ✗`, `#123 …` (no symbol without checks); `merged` for a merged pull request.
pub fn pr_text(pulls: &Value) -> Option<String> {
    let list = pulls.as_array()?;
    let pick = |state: &str| list.iter().find(|p| p["state"] == state);
    if pick("OPEN").is_none() && pick("MERGED").is_some() {
        return Some("merged".into());
    }
    let pr = pick("OPEN")?;
    let number = pr["number"].as_u64()?;
    let list = pr["statusCheckRollup"]
        .as_array()
        .map_or(&[][..], Vec::as_slice);
    let symbol = if list.iter().any(is_failing) {
        " ✗"
    } else if list.iter().any(is_pending) {
        " …"
    } else if list.is_empty() {
        ""
    } else {
        " ✓"
    };
    Some(format!("#{number}{symbol}"))
}

fn is_pending(c: &Value) -> bool {
    match c["__typename"].as_str() {
        Some("StatusContext") => matches!(c["state"].as_str(), Some("PENDING" | "EXPECTED")),
        _ => c["status"].as_str().is_some_and(|s| s != "COMPLETED"),
    }
}

fn is_failing(c: &Value) -> bool {
    match c["__typename"].as_str() {
        Some("StatusContext") => matches!(c["state"].as_str(), Some("FAILURE" | "ERROR")),
        _ => matches!(
            c["conclusion"].as_str(),
            Some("FAILURE" | "TIMED_OUT" | "CANCELLED" | "ACTION_REQUIRED" | "STARTUP_FAILURE")
        ),
    }
}

const GH_TIMEOUT: Duration = Duration::from_secs(20);

/// The `pr` token text of `branch` in `dir` through `gh`; `None` without `gh`, without a sign-in,
/// without a pull request or on any failure.
fn gh_pr_text(dir: &str, branch: &str) -> Option<String> {
    let path = poimen_core::gui_path::child_path();
    let program = poimen_core::gui_path::resolve_program("gh", &path)?;
    let mut child = poimen_core::gui_path::command(program)
        .args([
            "pr", "list", "--head", branch, "--state", "all", "--limit", "5",
        ])
        .args(["--json", "number,state,statusCheckRollup"])
        .current_dir(dir)
        .env("PATH", &path)
        .env("GH_PROMPT_DISABLED", "1")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });
    let deadline = Instant::now() + GH_TIMEOUT;
    let status = loop {
        if let Some(s) = child.try_wait().ok()? {
            break s;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let body = reader.join().ok()?;
    if !status.success() {
        return None;
    }
    pr_text(&serde_json::from_str(&body).ok()?)
}

#[cfg(test)]
#[path = "sidebar_git_tests.rs"]
mod tests;
