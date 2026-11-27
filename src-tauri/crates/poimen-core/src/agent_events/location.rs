//! Where run files are and whose they are (jsonl-file binding of agent-events 0.3, ADR 0006 of
//! GravionLabs/agent-events): the per-user shared directory ([`shared_dir`]), the legacy
//! directory of a repository ([`legacy_dir`], read for one minor version), the origin a run names
//! in `run.started` and the attribution of a run of the shared directory to a repository
//! ([`attribute`]). The environment, git and the clock are parameters.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use super::{EVENTS_PATH, SubjectKey, subject_key};

/// The environment variable that replaces the shared directory.
pub const DIR_ENV: &str = "AGENT_EVENTS_DIR";

/// The name of the shared directory below the user's state directory.
const DIR_NAME: &str = "agent-events";

/// How long a directory's repository is trusted before git is asked again (a repository cloned
/// later, a removed worktree).
pub const RESOLVE_TTL: Duration = Duration::from_secs(30);

/// How much of a file [`first_head`] reads to find its first line.
const FIRST_LINE_BYTES: u64 = 64 << 10;

/// The shared event directory of the user: [`DIR_ENV`] when set and not empty, else
/// `%LOCALAPPDATA%\agent-events` on Windows and `$XDG_STATE_HOME/agent-events` (only an absolute
/// `XDG_STATE_HOME`), else `~/.local/state/agent-events` elsewhere, macOS included. `env` reads one
/// environment variable; `None` when it names no home.
pub fn shared_dir(env: impl Fn(&str) -> Option<String>, windows: bool) -> Option<PathBuf> {
    let var = |name: &str| env(name).filter(|v| !v.is_empty());
    if let Some(dir) = var(DIR_ENV) {
        return Some(PathBuf::from(dir));
    }
    if windows {
        return var("LOCALAPPDATA").map(|base| Path::new(&base).join(DIR_NAME));
    }
    // A Unix path is absolute when it starts with `/`, whatever the platform of the caller.
    let state = var("XDG_STATE_HOME")
        .filter(|v| v.starts_with('/'))
        .map(PathBuf::from)
        .or_else(|| var("HOME").map(|home| Path::new(&home).join(".local").join("state")))?;
    Some(state.join(DIR_NAME))
}

/// [`shared_dir`] of this process.
pub fn shared_dir_here() -> Option<PathBuf> {
    shared_dir(|name| std::env::var(name).ok(), cfg!(windows))
}

/// The legacy event directory of the repository with the common git dir `common_dir`; linked
/// worktrees share their main checkout's.
pub fn legacy_dir(common_dir: &Path) -> PathBuf {
    common_dir.join(EVENTS_PATH)
}

/// `origin` of `run.started`; every field is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Origin {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worktree: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
}

impl Origin {
    /// The `origin` of a `run.started` event; `None` when it has no `origin` object.
    pub fn of(event: &Value) -> Option<Origin> {
        let origin = event.get("origin")?.as_object()?;
        let field = |name: &str| origin.get(name).and_then(super::text);
        Some(Origin {
            repo: field("repo"),
            branch: field("branch"),
            worktree: field("worktree"),
            host: field("host"),
        })
    }
}

/// What the first line of a run file tells about the run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Head {
    /// `(kind, id)` of the `subject` of `run.started`.
    pub subject: Option<SubjectKey>,
    pub origin: Option<Origin>,
}

/// The head of the run in `path`, from its first line only, without replaying the file. `None`
/// while the first line is incomplete; an empty head for a first line that is no `run.started`.
pub fn first_head(path: &Path) -> Option<Head> {
    let file = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    file.take(FIRST_LINE_BYTES).read_to_end(&mut buf).ok()?;
    let end = buf.iter().position(|b| *b == b'\n')?;
    let started = serde_json::from_slice::<Value>(&buf[..end])
        .ok()
        .filter(|e| e.get("type").and_then(Value::as_str) == Some("run.started"));
    Some(started.map_or_else(Head::default, |event| Head {
        subject: event.get("subject").and_then(subject_key),
        origin: Origin::of(&event),
    }))
}

/// The heads of run files, so a first line is read once.
#[derive(Debug, Default)]
pub struct Heads {
    /// Per file its head and, while the first line is incomplete, the length it was read at.
    cache: HashMap<PathBuf, (u64, Option<Head>)>,
}

impl Heads {
    /// [`first_head`] of `path`, of length `len`: read again only while the first line was
    /// incomplete and the file grew, or after the file shrank (replaced).
    pub fn get(&mut self, path: &Path, len: u64) -> Option<Head> {
        match self.cache.get(path) {
            Some((seen, Some(head))) if len >= *seen => return Some(head.clone()),
            Some((seen, None)) if *seen == len => return None,
            _ => {}
        }
        let head = first_head(path);
        self.cache.insert(path.to_path_buf(), (len, head.clone()));
        head
    }

    /// Forgets the files `keep` says no to.
    pub fn retain(&mut self, keep: impl Fn(&Path) -> bool) {
        self.cache.retain(|path, _| keep(path));
    }
}

/// `origin.repo` of the repository with the common git dir `common_dir` and the URL `remote` of
/// its remote `origin`: `owner/repo`, the URL's path without the leading `/` and `.git`; without
/// such a remote, or when its URL names no host, the directory of the repository.
pub fn repo_name(remote: Option<&str>, common_dir: &Path) -> String {
    if let Some(path) = remote.and_then(url_path) {
        return path;
    }
    let dir = match common_dir.parent() {
        Some(parent) if common_dir.file_name().is_some_and(|n| n == ".git") => parent,
        _ => common_dir,
    };
    dir.to_string_lossy().into_owned()
}

/// The path of a remote URL that names a host (`scheme://host/path` or `[user@]host:path`).
fn url_path(url: &str) -> Option<String> {
    let url = url.trim();
    let path = if let Some((scheme, rest)) = url.split_once("://") {
        let (authority, path) = rest.split_once('/')?;
        if scheme.eq_ignore_ascii_case("file") || authority.is_empty() {
            return None;
        }
        path
    } else {
        let (host, path) = url.split_once(':')?;
        // `C:\repo` and `./a:b` are local paths.
        if host.len() < 2 || host.contains(['/', '\\']) {
            return None;
        }
        path
    };
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    (!path.is_empty()).then(|| path.to_owned())
}

/// A repository a reader follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoRef {
    /// The toplevel of the directory it was found from.
    pub top: String,
    /// The common git dir, canonical when it exists.
    pub common: PathBuf,
    /// Its `origin.repo` ([`repo_name`]).
    pub name: String,
}

impl RepoRef {
    pub fn new(top: &str, common_dir: &str, remote: Option<&str>) -> RepoRef {
        let common = std::fs::canonicalize(common_dir).unwrap_or_else(|_| common_dir.into());
        RepoRef {
            top: top.to_owned(),
            // From the path as git gives it: a writer does not canonicalize it either.
            name: repo_name(remote, Path::new(common_dir)),
            common,
        }
    }

    /// The repository containing `dir`, asked of git; `None` outside one.
    pub fn of(dir: &str) -> Option<RepoRef> {
        // Not asked for a directory that is gone (a removed worktree of an old run).
        if !Path::new(dir).is_dir() {
            return None;
        }
        let (top, common) = crate::git::roots(dir)?;
        let remote = crate::git::origin_url(dir);
        Some(RepoRef::new(&top, &common, remote.as_deref()))
    }
}

/// The repositories of directories, each asked at most every [`RESOLVE_TTL`].
#[derive(Debug, Default)]
pub struct RepoCache {
    cache: HashMap<String, (Option<RepoRef>, Instant)>,
}

impl RepoCache {
    pub fn get(
        &mut self,
        dir: &str,
        now: Instant,
        lookup: impl FnOnce(&str) -> Option<RepoRef>,
    ) -> Option<RepoRef> {
        if let Some((repo, at)) = self.cache.get(dir)
            && now.duration_since(*at) < RESOLVE_TTL
        {
            return repo.clone();
        }
        let repo = lookup(dir);
        self.cache.insert(dir.to_owned(), (repo.clone(), now));
        repo
    }

    /// Forgets the answers older than [`RESOLVE_TTL`].
    pub fn prune(&mut self, now: Instant) {
        self.cache
            .retain(|_, (_, at)| now.duration_since(*at) < RESOLVE_TTL);
    }
}

/// The repository among `repos` a run of the shared directory belongs to, as its index: the one of
/// `origin.worktree` (the same common git dir), else the first named `origin.repo`. A run without
/// `origin` belongs to the repository of the directory of its terminal, `terminal_dir`.
/// `repo_of` answers for a directory; it is only asked when needed.
pub fn attribute(
    origin: Option<&Origin>,
    repos: &[RepoRef],
    mut repo_of: impl FnMut(&str) -> Option<RepoRef>,
    terminal_dir: impl FnOnce() -> Option<String>,
) -> Option<usize> {
    let mut by_dir = |dir: &str| {
        let common = repo_of(dir)?.common;
        repos.iter().position(|r| r.common == common)
    };
    match origin {
        Some(origin) => origin
            .worktree
            .as_deref()
            .and_then(&mut by_dir)
            .or_else(|| {
                let name = origin.repo.as_deref()?;
                repos.iter().position(|r| r.name == name)
            }),
        None => by_dir(&terminal_dir()?),
    }
}

#[cfg(test)]
#[path = "location_tests.rs"]
mod tests;
