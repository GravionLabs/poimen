//! Reading the event files of one repository: its runs in the shared directory of the user and in
//! the legacy directory of the repository (jsonl-file binding of agent-events, #1408).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use poimen_core::agent_events::{
    self, Heads, LineTail, MAX_EVENTS_BYTES, RECENT_SECS, Reducer, RepoCache, RepoRef, Run,
    SubjectKey,
};

use super::{named, recent, shown_subjects};

/// The directory of the terminal of a run (`Terminal.run`).
pub type TerminalDir = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// What the watches of the app share.
#[derive(Clone, Default)]
pub struct Context {
    /// The watched repositories, so a run of the shared directory shows in one of them only.
    pub repos: Arc<Mutex<Vec<RepoRef>>>,
    pub terminal_dir: Option<TerminalDir>,
}

/// Where the runs of one repository are.
#[derive(Clone, Default)]
pub struct Sources {
    /// The shared event directory of the user.
    pub shared: Option<PathBuf>,
    /// The legacy directory of the repository, shared by its worktrees.
    pub legacy: Option<PathBuf>,
    /// The repository; runs of the shared directory are attributed to it. `None` outside one.
    pub repo: Option<RepoRef>,
    pub context: Context,
}

impl Sources {
    /// The sources of the repository at `root`, from the environment of this process.
    pub fn of(root: &Path, context: Context) -> Sources {
        let repo = RepoRef::of(&root.to_string_lossy());
        Sources {
            shared: agent_events::shared_dir_here(),
            legacy: repo.as_ref().map(|r| agent_events::legacy_dir(&r.common)),
            repo,
            context,
        }
    }
}

/// A run file: its path, length, modification time and age in seconds.
#[derive(Debug, Clone)]
struct Found {
    path: PathBuf,
    len: u64,
    modified: Option<SystemTime>,
    age: f64,
}

/// One followed run: its files (the shared one first) and the fold of them.
pub(super) struct FileState {
    files: Vec<(PathBuf, u64, Option<SystemTime>)>,
    reducer: Reducer,
    /// Where the reading of a single file stands.
    tail: LineTail,
}

/// The event files of one repository: each is replayed from the start once and then read from
/// where it ended, up to the last complete line. A run in both directories is replayed from
/// both, merged by `seq`, whenever one of them changed.
#[derive(Default)]
pub struct Tailer {
    pub(super) files: HashMap<String, FileState>,
    heads: Heads,
    repos: RepoCache,
}

/// The run files of `dir` (regular files up to the size limit, changed within
/// [`agent_events::SUBJECT_KEEP_SECS`]), by run id.
fn list(dir: &Path, now: f64) -> Vec<(String, Found)> {
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
        let modified = meta.modified().ok();
        let age = modified
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map_or(0.0, |m| now - m.as_secs_f64());
        if age < agent_events::SUBJECT_KEEP_SECS {
            let len = meta.len();
            out.push((
                run,
                Found {
                    path,
                    len,
                    modified,
                    age,
                },
            ));
        }
    }
    out
}

/// The events of `files` (the shared one first), each `seq` once and the first file's when
/// several have it, folded in `seq` order.
fn merged(files: &[Found]) -> Reducer {
    let mut events: BTreeMap<i64, Value> = BTreeMap::new();
    for file in files {
        let tailed = LineTail::default()
            .read(&file.path, file.len)
            .unwrap_or_default();
        for line in &tailed.lines {
            if let Ok(event) = serde_json::from_str::<Value>(line)
                && let Some(seq) = event.get("seq").and_then(Value::as_i64)
            {
                events.entry(seq).or_insert(event);
            }
        }
    }
    let mut reducer = Reducer::new();
    events.values().for_each(|e| reducer.push(e));
    reducer
}

impl Tailer {
    fn folded(&mut self, run: &str, files: &[Found]) {
        let key: Vec<_> = files
            .iter()
            .map(|f| (f.path.clone(), f.len, f.modified))
            .collect();
        let same_files = self.files.get(run).is_some_and(|s| {
            s.files.len() == key.len() && s.files.iter().zip(&key).all(|(a, b)| a.0 == b.0)
        });
        if !same_files {
            self.files.remove(run);
        }
        let state = self
            .files
            .entry(run.to_owned())
            .or_insert_with(|| FileState {
                files: Vec::new(),
                reducer: Reducer::new(),
                tail: LineTail::default(),
            });
        if let [file] = files {
            if let Ok(tailed) = state.tail.read(&file.path, file.len) {
                if tailed.reset {
                    // Truncated or replaced: replay.
                    state.reducer = Reducer::new();
                }
                for line in &tailed.lines {
                    state.reducer.push_line(line);
                }
            }
        } else if state.files != key {
            state.reducer = merged(files);
        }
        state.files = key;
    }

    /// The files of each run of the repository: those of the shared directory attributed to it
    /// ([`agent_events::attribute`]) and those of its legacy directory, the shared one first; and
    /// every run file seen, whose first line stays cached.
    fn collect(
        &mut self,
        sources: &Sources,
        now: f64,
    ) -> (BTreeMap<String, Vec<Found>>, HashSet<PathBuf>) {
        let mut listed = HashSet::new();
        let mut out: BTreeMap<String, Vec<Found>> = BTreeMap::new();
        let mut unattributed = Vec::new();
        if let (Some(dir), Some(own)) = (&sources.shared, &sources.repo) {
            let mut repos = sources
                .context
                .repos
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            if !repos.iter().any(|r| r.common == own.common) {
                repos.push(own.clone());
            }
            let at = Instant::now();
            self.repos.prune(at);
            for (run, file) in list(dir, now) {
                listed.insert(file.path.clone());
                // A run shown already stays, also once its worktree is gone.
                let shown = self
                    .files
                    .get(&run)
                    .is_some_and(|s| s.files.iter().any(|f| f.0 == file.path));
                let ours = shown
                    || self.heads.get(&file.path, file.len).is_some_and(|head| {
                        let cache = &mut self.repos;
                        agent_events::attribute(
                            head.origin.as_ref(),
                            &repos,
                            |dir| cache.get(dir, at, RepoRef::of),
                            || sources.context.terminal_dir.as_ref()?(&run),
                        )
                        .is_some_and(|i| repos[i].common == own.common)
                    });
                if ours {
                    out.insert(run, vec![file]);
                } else {
                    unattributed.push((run, file));
                }
            }
        }
        if let Some(dir) = &sources.legacy {
            for (run, file) in list(dir, now) {
                listed.insert(file.path.clone());
                let files = out.entry(run).or_default();
                if files.iter().all(|f| f.path != file.path) {
                    files.push(file);
                }
            }
        }
        // A shared file nothing attributes still belongs to the run the legacy directory lists.
        for (run, file) in unattributed {
            if let Some(files) = out.get_mut(&run)
                && files.iter().all(|f| f.path != file.path)
            {
                files.insert(0, file);
            }
        }
        (out, listed)
    }

    /// Reads what is new in the event files of `sources` (`*.jsonl`, regular files only) and
    /// returns the runs to show, newest first, with the first unknown event version. Runs changed
    /// within the last day are read; an older one (up to [`agent_events::SUBJECT_KEEP_SECS`])
    /// only has its first line read and is replayed when a run of its subject is shown
    /// ([`recent`]); an older one is not opened.
    pub fn read(&mut self, sources: &Sources, now: f64) -> (Vec<Run>, Option<i64>) {
        let (found, listed) = self.collect(sources, now);
        let mut seen = HashSet::new();
        let mut older = Vec::new();
        for (run, files) in found {
            // The age of a run is that of its newest file.
            if files.iter().all(|f| f.age >= RECENT_SECS + 3600.0) {
                older.push((run, files));
            } else {
                self.folded(&run, &files);
                seen.insert(run);
            }
        }
        // The subjects shown by the runs read in full.
        let present: Vec<Run> = self
            .files
            .iter()
            .filter(|(run, _)| seen.contains(*run))
            .filter_map(|(_, state)| state.reducer.snapshot(now, &agent_events::pid_alive))
            .collect();
        let shown = shown_subjects(&present, now);
        for (run, files) in older {
            let key: Option<SubjectKey> = files
                .iter()
                .find_map(|f| self.heads.get(&f.path, f.len)?.subject);
            if key.is_some_and(|k| shown.contains(&k)) {
                self.folded(&run, &files);
                seen.insert(run);
            }
        }
        self.files.retain(|run, _| seen.contains(run));
        self.heads.retain(|path| listed.contains(path));
        let mut unknown = None;
        let mut runs = Vec::new();
        for (run, state) in &self.files {
            unknown = unknown.or(state.reducer.unknown_v);
            let snapshot = state.reducer.snapshot(now, &agent_events::pid_alive);
            runs.extend(snapshot.map(|r| named(r, run)));
        }
        (recent(runs, now), unknown)
    }
}
