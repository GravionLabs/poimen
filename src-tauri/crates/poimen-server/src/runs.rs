//! Progress events of the runs in the repositories of the server's spaces (#692), for the app's
//! Deliver panel and Events tab of a remote machine. The files are those of the agent-events
//! `jsonl-file` binding: `<run>.jsonl` in the shared directory of the user
//! (`agent_events::shared_dir`, #1408), attributed to a repository by the run's origin or its
//! terminal, and, for one minor version, in `<git-common-dir>/.agents/events` of each repository.
//! Read with the shared [`LineTail`] of `poimen_core::agent_events`: each file from its start, then
//! what it gained, up to the last complete line (a half-written last line waits for the next
//! read). The server does not fold the events (the app's reducer does); it forwards each complete
//! JSON object line unchanged as `run.events`, each `(run, seq)` once.
//!
//! Only while a subscription asked for `run_events`: then the server reads the event directories
//! every [`POLL`] (a `stat` per directory and file; no watcher dependency, nothing runs without
//! such a client) and keeps the events of the recent files, so a new subscription gets them from
//! the start before it follows ([`Runs::replay`]). Without one the kept events are dropped.
//! Read-only: the server never writes, moves or deletes an event file.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use poimen_core::agent_events::{
    self, Heads, LineTail, MAX_EVENTS_BYTES, Reducer, RepoRef, SubjectKey,
};
use poimen_protocol::{Event, TerminalStatus};
use serde_json::Value;

use crate::{Server, Shared, lock};

/// How often the event directories are read while a client follows them.
pub(crate) const POLL: Duration = Duration::from_millis(500);

/// A file not modified for this long is read only when a run of its subject is shown
/// ([`SUBJECT_KEEP`]); the app hides other runs that ended a day ago, so their events are of no
/// use. A file modified more recently is read (unless it is already followed).
const RECENT: Duration = Duration::from_secs(25 * 3600);

/// A file not modified for this long (14 days, `agent_events::SUBJECT_KEEP_SECS`) is not opened.
/// Between [`RECENT`] and this, a file is read (replayed) only while its subject, from its first
/// line, is the subject of a file modified within [`RECENT`]: the app keeps the earlier runs of a
/// shown subject (`MAX_SUBJECT_RUNS`, the newest 50 per subject, it applies the limit itself).
const SUBJECT_KEEP: Duration = Duration::from_secs(14 * 24 * 3600);

/// A run file untouched for longer is not a running run (the reducer abandons it after an hour).
const RUN_FRESH: Duration = Duration::from_secs(3600);

/// At most this many events go into one `run.events` line.
const CHUNK: usize = 500;

/// One run file of a followed run: where reading stands and every event read so far.
struct RunFile {
    path: PathBuf,
    tail: LineTail,
    events: Vec<Value>,
}

/// One followed run: the `root` the client sees and its files, the one of the shared directory
/// first.
struct Followed {
    root: String,
    files: Vec<RunFile>,
}

/// A run file found by a scan.
struct Found {
    path: PathBuf,
    len: u64,
    /// Time since the last change; `None` when unknown (or in the future).
    age: Option<Duration>,
}

/// The root and the files of each run, by run id.
type Collected = BTreeMap<String, (String, Vec<Found>)>;

/// Where the run files of a scan come from.
#[derive(Debug, Default)]
pub(crate) struct Sources {
    /// The shared directory of the user ([`agent_events::shared_dir`]).
    pub shared: Option<PathBuf>,
    /// The repositories of the spaces and terminals; a linked worktree counts as its main
    /// repository.
    pub repos: Vec<RepoRef>,
    /// The directory of the terminal of each run (`Terminal.run`).
    pub terminals: HashMap<String, String>,
}

/// The followed runs by run id: a run is sent once, under one root, whether the shared
/// directory, a legacy one or both hold it.
#[derive(Default)]
pub(crate) struct Runs {
    runs: BTreeMap<String, Followed>,
    heads: Heads,
}

fn chunks(root: &str, run: &str, events: &[Value]) -> Vec<Event> {
    events
        .chunks(CHUNK)
        .map(|c| Event::RunEvents {
            root: root.to_owned(),
            run: run.to_owned(),
            events: c.to_vec(),
        })
        .collect()
}

fn seq_of(event: &Value) -> Option<i64> {
    event.get("seq").and_then(Value::as_i64)
}

/// The events of `per_file` (the shared file first) that are not in `seen`, each `(run, seq)`
/// once: the first file that has it wins. An event without `seq` counts from the first file only.
/// In `seq` order when more than one file takes part, else in file order.
fn merge<'a>(
    per_file: impl Iterator<Item = (usize, &'a [Value])>,
    seen: &mut HashSet<i64>,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut files = 0;
    for (index, events) in per_file {
        let before = out.len();
        out.extend(
            events
                .iter()
                .filter(|e| seq_of(e).map_or(index == 0, |s| seen.insert(s)))
                .cloned(),
        );
        files += usize::from(out.len() > before);
    }
    if files > 1 {
        out.sort_by_key(|e| seq_of(e).unwrap_or(i64::MAX));
    }
    out
}

/// The run files of `dir` (regular files up to the size limit, changed within [`SUBJECT_KEEP`]),
/// by run id.
fn list(dir: &Path, now: SystemTime) -> Vec<(String, Found)> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let Some(run) = agent_events::run_of(&path).map(str::to_owned) else {
            continue;
        };
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.is_file() || meta.len() > MAX_EVENTS_BYTES {
            continue;
        }
        let age = meta
            .modified()
            .ok()
            .and_then(|m| now.duration_since(m).ok());
        if age.is_some_and(|a| a >= SUBJECT_KEEP) {
            continue;
        }
        let len = meta.len();
        out.push((run, Found { path, len, age }));
    }
    out
}

impl Runs {
    /// Reads what the run files of the followed repositories gained: the new events per run. The
    /// files are those of the shared directory attributed to a repository
    /// ([`agent_events::attribute`], asking `repo_of` for a directory) and those of the legacy
    /// directories. A run that is no longer listed is forgotten.
    pub(crate) fn scan(
        &mut self,
        sources: &Sources,
        now: SystemTime,
        mut repo_of: impl FnMut(&str) -> Option<RepoRef>,
    ) -> Vec<Event> {
        let (found, listed) = self.collect(sources, now, &mut repo_of);
        // The subjects of the recent runs per root; an older run follows when it shares one.
        let recent = |files: &[Found]| files.iter().any(|f| f.age.is_none_or(|a| a < RECENT));
        let mut shown: HashSet<(String, SubjectKey)> = HashSet::new();
        for (root, files) in found.values().filter(|(_, files)| recent(files)) {
            for f in files {
                let subject = self.heads.get(&f.path, f.len).and_then(|h| h.subject);
                shown.extend(subject.map(|s| (root.clone(), s)));
            }
        }
        let mut wanted = Vec::new();
        for (run, (root, files)) in found {
            let follow = recent(&files)
                || files.iter().any(|f| {
                    let subject = self.heads.get(&f.path, f.len).and_then(|h| h.subject);
                    subject.is_some_and(|s| shown.contains(&(root.clone(), s)))
                });
            if follow {
                wanted.push((run, root, files));
            }
        }
        self.heads.retain(|path| listed.contains(path));
        let mut before = std::mem::take(&mut self.runs);
        let mut out = Vec::new();
        for (run, root, files) in wanted {
            let mut followed = before
                .remove(&run)
                .filter(|f| f.root == root)
                .unwrap_or_else(|| Followed {
                    root,
                    files: Vec::new(),
                });
            out.extend(followed.read(&run, files));
            self.runs.insert(run, followed);
        }
        out
    }

    /// The files of each run of the followed repositories with its root, the shared file first,
    /// and every run file seen (also of runs of other repositories, whose first line stays
    /// cached). A run keeps the root it has.
    fn collect(
        &mut self,
        sources: &Sources,
        now: SystemTime,
        repo_of: &mut impl FnMut(&str) -> Option<RepoRef>,
    ) -> (Collected, HashSet<PathBuf>) {
        let mut listed = HashSet::new();
        let mut out = Collected::new();
        let mut unattributed = Vec::new();
        let shared = sources.shared.as_deref().map(|dir| list(dir, now));
        for (run, file) in shared.into_iter().flatten() {
            listed.insert(file.path.clone());
            let kept = self
                .runs
                .get(&run)
                .filter(|f| sources.repos.iter().any(|r| r.top == f.root))
                .map(|f| f.root.clone());
            let root = kept.or_else(|| {
                // An incomplete first line: asked again once it is complete.
                let head = self.heads.get(&file.path, file.len)?;
                let at = agent_events::attribute(
                    head.origin.as_ref(),
                    &sources.repos,
                    &mut *repo_of,
                    || sources.terminals.get(&run).cloned(),
                )?;
                Some(sources.repos[at].top.clone())
            });
            match root {
                Some(root) => {
                    out.insert(run, (root, vec![file]));
                }
                None => unattributed.push((run, file)),
            }
        }
        for repo in &sources.repos {
            for (run, file) in list(&agent_events::legacy_dir(&repo.common), now) {
                listed.insert(file.path.clone());
                let (root, files) = out
                    .entry(run)
                    .or_insert_with(|| (repo.top.clone(), Vec::new()));
                // The same run id in another repository's directory is not sent twice.
                if *root == repo.top && files.iter().all(|f| f.path != file.path) {
                    files.push(file);
                }
            }
        }
        // A shared file nothing attributes still belongs to the run a legacy directory lists.
        for (run, file) in unattributed {
            if let Some((_, files)) = out.get_mut(&run)
                && files.iter().all(|f| f.path != file.path)
            {
                files.insert(0, file);
            }
        }
        (out, listed)
    }

    /// Whether a run of the followed repositories is running: fresh, readable, no
    /// `run.finished`, its writer alive.
    pub(crate) fn running(
        &mut self,
        sources: &Sources,
        now: SystemTime,
        mut repo_of: impl FnMut(&str) -> Option<RepoRef>,
    ) -> bool {
        let secs = now
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let (found, _) = self.collect(sources, now, &mut repo_of);
        found.into_iter().any(|(run, (root, files))| {
            if !files.iter().any(|f| f.age.is_some_and(|a| a < RUN_FRESH)) {
                return false;
            }
            let mut followed = Followed {
                root,
                files: Vec::new(),
            };
            let mut reducer = Reducer::new();
            for event in followed.read(&run, files) {
                if let Event::RunEvents { events, .. } = event {
                    events.iter().for_each(|e| reducer.push(e));
                }
            }
            reducer
                .snapshot(secs, &agent_events::pid_alive)
                .is_some_and(|run| run.state.status == "running")
        })
    }

    /// The events of every followed run from the start, for a new subscription.
    pub(crate) fn replay(&self) -> Vec<Event> {
        self.runs
            .iter()
            .flat_map(|(run, followed)| {
                let files = followed.files.iter().map(|f| f.events.as_slice());
                let events = merge(files.enumerate(), &mut HashSet::new());
                chunks(&followed.root, run, &events)
            })
            .collect()
    }

    /// Forgets every file (no client follows the runs any more).
    pub(crate) fn clear(&mut self) {
        self.runs.clear();
        self.heads = Heads::default();
    }
}

impl Followed {
    /// Takes `found` as the files of the run and returns the events they gained that were not
    /// sent before.
    fn read(&mut self, run: &str, found: Vec<Found>) -> Vec<Event> {
        let mut before = std::mem::take(&mut self.files);
        let mut new: Vec<Vec<Value>> = Vec::new();
        for file in found {
            let at = before.iter().position(|f| f.path == file.path);
            let mut state = match at {
                Some(at) => before.swap_remove(at),
                None => RunFile {
                    path: file.path.clone(),
                    tail: LineTail::default(),
                    events: Vec::new(),
                },
            };
            let tailed = state.tail.read(&file.path, file.len).unwrap_or_default();
            if tailed.reset {
                state.events.clear();
            }
            new.push(
                tailed
                    .lines
                    .iter()
                    .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                    .filter(Value::is_object)
                    .collect(),
            );
            self.files.push(state);
        }
        if new.iter().all(Vec::is_empty) {
            return Vec::new();
        }
        // What was sent: everything the files held before this read.
        let mut seen: HashSet<i64> = self
            .files
            .iter()
            .flat_map(|f| f.events.iter().filter_map(seq_of))
            .collect();
        let files = new.iter().map(Vec::as_slice);
        let out = merge(files.enumerate(), &mut seen);
        for (file, events) in self.files.iter_mut().zip(new) {
            file.events.extend(events);
        }
        chunks(&self.root, run, &out)
    }
}

impl Shared {
    /// Reads what the run files of the spaces' repositories gained and sends it to the
    /// subscriptions with `run_events`; without one, the followed files are forgotten (#692).
    pub(crate) fn follow_runs(&self) {
        let wanted = || lock(&self.subscribers).iter().any(|s| s.runs);
        {
            // Under the lock, so a subscription that comes meanwhile is never cleared away.
            let mut followed = lock(&self.runs);
            if !wanted() {
                followed.clear();
                return;
            }
        }
        let (dirs, terminals) = self.run_dirs();
        let mut repo_of = self.repo_lookup();
        let sources = self.sources(&dirs, terminals, &mut repo_of);
        let mut followed = lock(&self.runs);
        // Checked again under the lock: a subscription that came meanwhile got the replay of what
        // is kept, so it is only dropped while there is none.
        if !wanted() {
            followed.clear();
            return;
        }
        let events = followed.scan(&sources, SystemTime::now(), repo_of);
        let mut subs = lock(&self.subscribers);
        for event in events {
            subs.retain(|s| !s.runs || s.tx.send(event.clone()).is_ok());
        }
    }
}

impl Shared {
    /// The directories of the spaces and running terminals, and the directory of the terminal of
    /// each run.
    fn run_dirs(&self) -> (Vec<String>, HashMap<String, String>) {
        let inner = lock(&self.inner);
        let mut dirs: Vec<String> = inner.layout.spaces.iter().map(|s| s.cwd.clone()).collect();
        let mut terminals = HashMap::new();
        for t in &inner.terminals {
            if t.info.status == TerminalStatus::Running && !dirs.contains(&t.info.cwd) {
                dirs.push(t.info.cwd.clone());
            }
            if let Some(run) = &t.info.run {
                terminals.insert(run.clone(), t.info.cwd.clone());
            }
        }
        (dirs, terminals)
    }

    /// The repository of a directory, from the cache. The cache is locked per question only: no
    /// other lock is ever taken while it is held, so it may be asked under `runs` or `awake`.
    fn repo_lookup(&self) -> impl FnMut(&str) -> Option<RepoRef> + '_ {
        let now = Instant::now();
        lock(&self.run_repos).prune(now);
        move |dir| lock(&self.run_repos).get(dir, now, RepoRef::of)
    }

    fn sources(
        &self,
        dirs: &[String],
        terminals: HashMap<String, String>,
        repo_of: impl FnMut(&str) -> Option<RepoRef>,
    ) -> Sources {
        Sources {
            shared: lock(&self.events_dir).clone(),
            repos: distinct(dirs, repo_of),
            terminals,
        }
    }

    /// A run of a followed repository is running, whether or not a client follows the runs.
    pub(crate) fn any_run_running(&self) -> bool {
        let (dirs, terminals) = self.run_dirs();
        let mut repo_of = self.repo_lookup();
        let sources = self.sources(&dirs, terminals, &mut repo_of);
        Runs::default().running(&sources, SystemTime::now(), repo_of)
    }
}

impl Server {
    /// Uses `dir` instead of the shared event directory of this process's environment (tests
    /// pass a temp directory). Take it before `serve`.
    pub fn set_events_dir(&self, dir: Option<PathBuf>) {
        *lock(&self.shared.events_dir) = dir;
    }
}

/// Each distinct repository among `dirs`, by the toplevel of the first directory of it.
fn distinct(dirs: &[String], mut repo_of: impl FnMut(&str) -> Option<RepoRef>) -> Vec<RepoRef> {
    let mut out: Vec<RepoRef> = Vec::new();
    for repo in dirs.iter().filter_map(|dir| repo_of(dir)) {
        if !out.iter().any(|r| r.common == repo.common) {
            out.push(repo);
        }
    }
    out
}

#[cfg(test)]
#[path = "runs_tests.rs"]
mod tests;
