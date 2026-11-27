//! Live state of deliver runs. Primary source: the progress events of the runs of the repository,
//! `<run>.jsonl` in the shared event directory of the user (attributed to the repository, #1408)
//! and in `<git-common-dir>/.agents/events` (ADR 0001 of GravionLabs/skills), read by [`Tailer`]
//! and folded by `poimen_core::agent_events`; interactive `/deliver` sessions and loops alike.
//! Fallback during the migration: `<repo>/.agents/deliver-loop/status.json` (schema
//! `deliver-loop-status.schema.json`, `schemaVersion: 1`) when the repository has no events. Both
//! are watched with `notify`, no polling. The loop writes status.json atomically (temp file, then
//! rename), so a read never sees a partial file. Read-only: poimen never writes either.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::Value;

use poimen_core::agent_events::{self, RECENT_SECS, Run, SubjectKey};

mod tail;
pub use tail::{Context, Sources, Tailer, TerminalDir};

/// The status file relative to a repository root.
pub const STATUS_PATH: &str = ".agents/deliver-loop/status.json";

/// The schema version this build understands; others are reported, not parsed.
pub const SUPPORTED_SCHEMA: u64 = 1;

/// A larger file is not a status file.
const MAX_BYTES: u64 = 1 << 20;

/// What a repository's status file holds right now.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Read {
    /// No file: no loop was started here, or its data was removed.
    Missing,
    /// A JSON object of the supported schema version; the frontend validates its fields.
    Ok { status: Value },
    /// A JSON object with another `schemaVersion` (or none).
    Unsupported {
        #[serde(rename = "schemaVersion")]
        schema_version: Option<Value>,
    },
    /// Not readable, not JSON, not an object, not a regular file or too large.
    Invalid { error: String },
    /// The runs found in the progress events of the repository, newest first; `unknown_version`
    /// is the first event version this build does not know (those events are skipped).
    Events {
        runs: Vec<Run>,
        #[serde(rename = "unknownVersion")]
        unknown_version: Option<i64>,
    },
}

/// At most this many runs per repository inside the present limits ([`agent_events::RECENT_SECS`]
/// after their end); runs of a shown subject kept beyond them are not counted
/// ([`agent_events::SUBJECT_KEEP_SECS`], [`agent_events::MAX_SUBJECT_RUNS`]).
const MAX_RUNS: usize = 12;

/// `run` with the run id of its file when its events name none.
pub fn named(mut run: Run, file_run: &str) -> Run {
    if run.state.run.is_empty() {
        run.state.run = file_run.to_owned();
    }
    run
}

fn subject_of(run: &Run) -> Option<SubjectKey> {
    run.state
        .subject
        .as_ref()
        .and_then(agent_events::subject_key)
}

fn updated(run: &Run) -> Option<f64> {
    run.state
        .updated
        .as_deref()
        .and_then(agent_events::parse_ts)
}

/// The runs inside the present limits: not ended or abandoned more than a day ago, the newest
/// [`MAX_RUNS`] of them.
fn present(runs: &[Run], now: f64) -> Vec<&Run> {
    let mut within: Vec<&Run> = runs
        .iter()
        .filter(|run| {
            let over = run.state.status != "running";
            !(over && updated(run).is_some_and(|t| now - t > RECENT_SECS))
        })
        .collect();
    within.sort_by(|a, b| order(a, b));
    within.truncate(MAX_RUNS);
    within
}

/// The subjects of the runs inside the present limits.
fn shown_subjects(runs: &[Run], now: f64) -> HashSet<SubjectKey> {
    present(runs, now)
        .into_iter()
        .filter_map(subject_of)
        .collect()
}

/// The runs the views show, newest first: those inside the present limits (not ended or abandoned
/// more than a day ago, [`MAX_RUNS`] of them), and every other run of a subject one of them has,
/// when its last event is at most [`agent_events::SUBJECT_KEEP_SECS`] old, the newest
/// [`agent_events::MAX_SUBJECT_RUNS`] per subject. A run without a subject keeps the present limits.
pub fn recent(runs: Vec<Run>, now: f64) -> Vec<Run> {
    let shown_runs: HashSet<String> = present(&runs, now)
        .into_iter()
        .map(|r| r.state.run.clone())
        .collect();
    let shown = shown_subjects(&runs, now);
    let mut kept: Vec<Run> = runs
        .into_iter()
        .filter(|run| {
            shown_runs.contains(&run.state.run)
                || subject_of(run).is_some_and(|k| {
                    shown.contains(&k)
                        && updated(run).is_none_or(|t| now - t <= agent_events::SUBJECT_KEEP_SECS)
                })
        })
        .collect();
    kept.sort_by(order);
    let mut per_subject: HashMap<SubjectKey, usize> = HashMap::new();
    kept.retain(|run| match subject_of(run) {
        Some(k) => {
            let n = per_subject.entry(k).or_insert(0);
            *n += 1;
            *n <= agent_events::MAX_SUBJECT_RUNS
        }
        None => true,
    });
    kept
}

fn order(a: &Run, b: &Run) -> std::cmp::Ordering {
    b.state
        .updated
        .cmp(&a.state.updated)
        .then_with(|| a.state.run.cmp(&b.state.run))
}

pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// The current state of a repository: its events when there are runs (or events of an unknown
/// version), else the status file.
pub fn read_all(root: &Path, sources: &Sources, tailer: &mut Tailer) -> Read {
    let (runs, unknown_version) = tailer.read(sources, now_secs());
    if !runs.is_empty() || unknown_version.is_some() {
        return Read::Events {
            runs,
            unknown_version,
        };
    }
    read(root)
}

/// Reads the status file of the repository at `root`.
pub fn read(root: &Path) -> Read {
    let path = root.join(STATUS_PATH);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Read::Missing,
        Err(e) => {
            return Read::Invalid {
                error: e.to_string(),
            };
        }
    };
    if !meta.is_file() {
        return Read::Invalid {
            error: "status.json is not a regular file".into(),
        };
    }
    if meta.len() > MAX_BYTES {
        return Read::Invalid {
            error: "status.json is larger than 1 MiB".into(),
        };
    }
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // Removed between the check and the read.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Read::Missing,
        Err(e) => {
            return Read::Invalid {
                error: e.to_string(),
            };
        }
    };
    parse(&text)
}

/// Classifies the text of a status file.
pub fn parse(text: &str) -> Read {
    let value: Value = match serde_json::from_str(text) {
        Ok(value) => value,
        Err(e) => {
            return Read::Invalid {
                error: e.to_string(),
            };
        }
    };
    let Some(object) = value.as_object() else {
        return Read::Invalid {
            error: "status.json is not a JSON object".into(),
        };
    };
    match object.get("schemaVersion") {
        Some(v) if v.as_u64() == Some(SUPPORTED_SCHEMA) => Read::Ok { status: value },
        other => Read::Unsupported {
            schema_version: other.cloned(),
        },
    }
}

enum Msg {
    Changed,
    Stop,
}

/// A running watch of one repository; dropping it stops the thread and the file watcher.
pub struct Handle {
    stop: Sender<Msg>,
}

impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.stop.send(Msg::Stop);
    }
}

/// Whether a changed path can concern the status or event files: anything below a `.agents`
/// directory, below the shared event directory or on the way to it.
fn relevant(path: &Path, shared: &[PathBuf]) -> bool {
    path.components().any(|c| c.as_os_str() == ".agents")
        || shared
            .iter()
            .any(|dir| path.starts_with(dir) || dir.starts_with(path))
}

/// `dir` as it is and as a watcher may name it: with its deepest existing directory resolved.
fn spellings(dir: &Path) -> Vec<PathBuf> {
    let real = dir.ancestors().find_map(|a| {
        let real = std::fs::canonicalize(a).ok()?;
        Some(real.join(dir.strip_prefix(a).ok()?))
    });
    std::iter::once(dir.to_path_buf()).chain(real).collect()
}

/// The deepest existing directory on the way to the status file; the one to watch, since a
/// directory that does not exist cannot be watched and its creation is seen one level up.
fn target(root: &Path) -> PathBuf {
    let loop_dir = root.join(".agents").join("deliver-loop");
    let agents = root.join(".agents");
    if loop_dir.is_dir() {
        loop_dir
    } else if agents.is_dir() {
        agents
    } else {
        root.to_path_buf()
    }
}

/// The deepest existing directory on the way to the event files of `events_dir`.
fn events_target(events_dir: &Path) -> Option<PathBuf> {
    events_dir
        .ancestors()
        .find(|p| p.is_dir())
        .map(Path::to_path_buf)
}

/// Watches the deliver state of the repository at `root`: calls `emit` with the current content
/// at the start and again after every change that alters it (bursts coalesced).
pub fn watch(root: PathBuf, context: Context, emit: impl Fn(Read) + Send + 'static) -> Handle {
    watch_with(root, move |root| Sources::of(root, context), emit)
}

/// Like [`watch`] with the event sources of the repository, asked for on the watching thread.
pub fn watch_with(
    root: PathBuf,
    sources: impl FnOnce(&Path) -> Sources + Send + 'static,
    emit: impl Fn(Read) + Send + 'static,
) -> Handle {
    let (tx, rx) = mpsc::channel();
    let handle = Handle { stop: tx.clone() };
    std::thread::spawn(move || {
        let notifier = tx;
        let sources = sources(&root);
        let shared = sources.shared.as_deref().map_or_else(Vec::new, spellings);
        let Ok(mut watcher) =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event
                    // Reading the files is not a change.
                    && !event.kind.is_access()
                    && event.paths.iter().any(|p| relevant(p, &shared))
                {
                    let _ = notifier.send(Msg::Changed);
                }
            })
        else {
            return;
        };
        let mut tailer = Tailer::default();
        let mut watched: Vec<PathBuf> = Vec::new();
        let mut last: Option<Read> = None;
        loop {
            // Arm before reading, so a change between the two is not lost.
            let events = [&sources.legacy, &sources.shared];
            let mut wanted = vec![target(&root)];
            wanted.extend(events.iter().filter_map(|d| events_target(d.as_deref()?)));
            wanted.sort();
            wanted.dedup();
            if watched != wanted {
                for old in watched.drain(..) {
                    let _ = watcher.unwatch(&old);
                }
                for dir in wanted {
                    if watcher.watch(&dir, RecursiveMode::NonRecursive).is_ok() {
                        watched.push(dir);
                    }
                }
            }
            let current = read_all(&root, &sources, &mut tailer);
            if last.as_ref() != Some(&current) {
                emit(current.clone());
                last = Some(current);
            }
            match rx.recv() {
                Ok(Msg::Changed) => {
                    // A burst of writes becomes one read.
                    std::thread::sleep(Duration::from_millis(40));
                    let mut stopped = false;
                    while let Ok(msg) = rx.try_recv() {
                        stopped |= matches!(msg, Msg::Stop);
                    }
                    if stopped {
                        break;
                    }
                }
                _ => break,
            }
        }
    });
    handle
}

/// What to do to go from the `current` watched roots to the `wanted` ones: `(stop, start)`.
pub fn plan(current: &HashSet<String>, wanted: &[String]) -> (Vec<String>, Vec<String>) {
    let wanted_set: HashSet<&String> = wanted.iter().collect();
    let mut stop: Vec<String> = current
        .iter()
        .filter(|r| !wanted_set.contains(r))
        .cloned()
        .collect();
    stop.sort();
    let mut start = Vec::new();
    for root in wanted {
        if !current.contains(root) && !start.contains(root) {
            start.push(root.clone());
        }
    }
    (stop, start)
}

/// The running watches by repository root, and what they share.
#[derive(Default)]
pub struct Watches {
    pub handles: HashMap<String, Handle>,
    pub context: Context,
}

#[cfg(test)]
#[path = "deliver_tests.rs"]
mod tests;
