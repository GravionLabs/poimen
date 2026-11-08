//! Live state of deliver runs. Primary source: the progress events of
//! `<git-common-dir>/.agents/events/*.jsonl` (ADR 0001 of GravionLabs/skills), each file replayed
//! from the start and then followed (`Tailer`), a half-written last line left for the next read,
//! folded by `progress_events.rs`; interactive `/deliver` sessions and loops alike. Fallback
//! during the migration: `<repo>/.agents/deliver-loop/status.json` (schema
//! `deliver-loop-status.schema.json`, `schemaVersion: 1`) when the repository has no events. Both
//! are watched with `notify`, no polling. The loop writes status.json atomically (temp file, then
//! rename), so a read never sees a partial file. Read-only: poimen never writes either.

use std::collections::{HashMap, HashSet};
use std::io::{Read as _, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use notify::{RecursiveMode, Watcher};
use serde::Serialize;
use serde_json::Value;

use crate::progress_events::{self, Reducer, Run};

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

/// The progress event files relative to the git common dir.
pub const EVENTS_PATH: &str = ".agents/events";

/// A larger event file is not read.
const MAX_EVENTS_BYTES: u64 = 16 << 20;

/// Runs that ended or were abandoned longer ago than this are not shown, nor are older files read.
const RECENT_SECS: f64 = 24.0 * 3600.0;

/// At most this many runs per repository.
const MAX_RUNS: usize = 12;

struct FileState {
    reducer: Reducer,
    offset: u64,
}

/// The event files of one repository: each is replayed from the start once and then read from
/// where it ended, up to the last complete line.
#[derive(Default)]
pub struct Tailer {
    files: HashMap<PathBuf, FileState>,
}

impl Tailer {
    fn advance(state: &mut FileState, path: &Path, len: u64) -> std::io::Result<()> {
        if len < state.offset {
            // Truncated or replaced: replay.
            *state = FileState {
                reducer: Reducer::new(),
                offset: 0,
            };
        }
        if len == state.offset {
            return Ok(());
        }
        let mut file = std::fs::File::open(path)?;
        file.seek(SeekFrom::Start(state.offset))?;
        let mut buf = Vec::new();
        file.take(len - state.offset).read_to_end(&mut buf)?;
        // An incomplete last line is still being written: leave it for the next read.
        let Some(end) = buf.iter().rposition(|b| *b == b'\n') else {
            return Ok(());
        };
        for line in String::from_utf8_lossy(&buf[..=end]).lines() {
            if !line.trim().is_empty() {
                state.reducer.push_line(line);
            }
        }
        state.offset += end as u64 + 1;
        Ok(())
    }

    /// Reads what is new in `dir` (`*.jsonl`, regular files only) and returns the recent runs,
    /// newest first, with the first unknown event version.
    pub fn read(&mut self, dir: &Path, now: f64) -> (Vec<Run>, Option<i64>) {
        let mut seen = HashSet::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                let Ok(meta) = std::fs::symlink_metadata(&path) else {
                    continue;
                };
                if !meta.is_file() || meta.len() > MAX_EVENTS_BYTES {
                    continue;
                }
                let recent = meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
                    .is_none_or(|m| now - m.as_secs_f64() < RECENT_SECS + 3600.0);
                if !recent && !self.files.contains_key(&path) {
                    continue;
                }
                seen.insert(path.clone());
                let state = self.files.entry(path.clone()).or_insert_with(|| FileState {
                    reducer: Reducer::new(),
                    offset: 0,
                });
                let _ = Self::advance(state, &path, meta.len());
            }
        }
        self.files.retain(|path, _| seen.contains(path));
        let mut unknown = None;
        let mut runs = Vec::new();
        for (path, state) in &self.files {
            unknown = unknown.or(state.reducer.unknown_v);
            if let Some(mut run) = state.reducer.snapshot(now, &progress_events::pid_alive) {
                if run.state.run.is_empty()
                    && let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                {
                    run.state.run = stem.to_owned();
                }
                let last = run
                    .state
                    .updated_at
                    .as_deref()
                    .and_then(progress_events::parse_ts);
                let over = run.state.result != "running";
                if over && last.is_some_and(|t| now - t > RECENT_SECS) {
                    continue;
                }
                runs.push(run);
            }
        }
        runs.sort_by(|a, b| {
            b.state
                .updated_at
                .cmp(&a.state.updated_at)
                .then_with(|| a.state.run.cmp(&b.state.run))
        });
        runs.truncate(MAX_RUNS);
        (runs, unknown)
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

/// The current state of a repository: its events when there are runs (or events of an unknown
/// version), else the status file.
pub fn read_all(root: &Path, events_dir: Option<&Path>, tailer: &mut Tailer) -> Read {
    if let Some(dir) = events_dir {
        let (runs, unknown_version) = tailer.read(dir, now_secs());
        if !runs.is_empty() || unknown_version.is_some() {
            return Read::Events {
                runs,
                unknown_version,
            };
        }
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
/// directory.
fn relevant(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == ".agents")
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

/// The directory of the event files of the repository at `root`: `.agents/events` in its common
/// dir, so every worktree of a repository shares it.
pub fn events_dir(root: &Path) -> Option<PathBuf> {
    crate::git::common_dir(&root.to_string_lossy()).map(|c| PathBuf::from(c).join(EVENTS_PATH))
}

/// Watches the deliver state of the repository at `root`: calls `emit` with the current content
/// at the start and again after every change that alters it (bursts coalesced).
pub fn watch(root: PathBuf, emit: impl Fn(Read) + Send + 'static) -> Handle {
    watch_with(root, None, emit)
}

/// Like [`watch`] with the events directory given (`None`: resolved from the common git dir).
pub fn watch_with(
    root: PathBuf,
    events: Option<PathBuf>,
    emit: impl Fn(Read) + Send + 'static,
) -> Handle {
    let (tx, rx) = mpsc::channel();
    let handle = Handle { stop: tx.clone() };
    std::thread::spawn(move || {
        let notifier = tx;
        let Ok(mut watcher) =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if let Ok(event) = event
                    && event.paths.iter().any(|p| relevant(p))
                {
                    let _ = notifier.send(Msg::Changed);
                }
            })
        else {
            return;
        };
        let events = events.or_else(|| events_dir(&root));
        let mut tailer = Tailer::default();
        let mut watched: Vec<PathBuf> = Vec::new();
        let mut last: Option<Read> = None;
        loop {
            // Arm before reading, so a change between the two is not lost.
            let mut wanted = vec![target(&root)];
            wanted.extend(events.as_deref().and_then(events_target));
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
            let current = read_all(&root, events.as_deref(), &mut tailer);
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

/// The running watches by repository root.
pub type Watches = HashMap<String, Handle>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Instant;

    fn temp_dir(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "poimen-deliver-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // Events carry resolved paths (`/private/var` for `/var` on macOS).
        dir.canonicalize().unwrap()
    }

    fn write_status(root: &Path, text: &str) {
        let dir = root.join(".agents/deliver-loop");
        std::fs::create_dir_all(&dir).unwrap();
        let tmp = dir.join("status.json.tmp");
        std::fs::write(&tmp, text).unwrap();
        std::fs::rename(&tmp, dir.join("status.json")).unwrap();
    }

    const RUNNING: &str = include_str!("../../src/app/deliver/fixtures/status-running.json");

    #[test]
    fn a_missing_file_is_missing() {
        let root = temp_dir("missing");
        assert_eq!(read(&root), Read::Missing);
    }

    #[test]
    fn a_status_of_the_supported_version_is_ok() {
        let root = temp_dir("ok");
        write_status(&root, RUNNING);
        let Read::Ok { status } = read(&root) else {
            panic!("not ok");
        };
        assert_eq!(status["milestone"]["key"], "M4");
    }

    #[test]
    fn another_schema_version_is_unsupported_not_an_error() {
        assert_eq!(
            parse(r#"{"schemaVersion":2,"run":"x"}"#),
            Read::Unsupported {
                schema_version: Some(Value::from(2))
            }
        );
        assert_eq!(
            parse(r#"{"run":"x"}"#),
            Read::Unsupported {
                schema_version: None
            }
        );
    }

    #[test]
    fn broken_content_is_invalid() {
        for text in ["", "{", "[]", "42", "null"] {
            assert!(matches!(parse(text), Read::Invalid { .. }), "{text:?}");
        }
    }

    #[test]
    fn a_directory_in_place_of_the_file_is_invalid() {
        let root = temp_dir("dir");
        std::fs::create_dir_all(root.join(STATUS_PATH)).unwrap();
        assert!(matches!(read(&root), Read::Invalid { .. }));
    }

    #[test]
    fn a_huge_file_is_invalid() {
        let root = temp_dir("huge");
        write_status(&root, &" ".repeat(MAX_BYTES as usize + 1));
        assert!(matches!(read(&root), Read::Invalid { .. }));
    }

    #[test]
    fn serializes_with_the_wire_names() {
        let json = serde_json::to_value(Read::Unsupported {
            schema_version: Some(Value::from(9)),
        })
        .unwrap();
        assert_eq!(json["kind"], "unsupported");
        assert_eq!(json["schemaVersion"], 9);
        assert_eq!(
            serde_json::to_value(Read::Missing).unwrap()["kind"],
            "missing"
        );
    }

    #[test]
    fn plan_stops_gone_roots_and_starts_new_ones_once() {
        let current: HashSet<String> = ["/a", "/b"].iter().map(|s| s.to_string()).collect();
        let wanted: Vec<String> = ["/b", "/c", "/c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(
            plan(&current, &wanted),
            (vec!["/a".to_string()], vec!["/c".to_string()])
        );
        assert_eq!(plan(&HashSet::new(), &[]), (vec![], vec![]));
    }

    #[test]
    fn the_watch_follows_the_file_from_missing_to_written_to_rewritten() {
        let root = temp_dir("watch");
        let (tx, rx) = mpsc::channel();
        let _handle = watch(root.clone(), move |r| {
            let _ = tx.send(r);
        });
        let next = |want: &dyn Fn(&Read) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok(r) if want(&r) => return r,
                    Ok(_) => {}
                    Err(_) => panic!("no matching update"),
                }
            }
        };
        next(&|r| *r == Read::Missing);
        // The directories do not exist yet: their creation is seen one level up.
        write_status(&root, RUNNING);
        next(&|r| matches!(r, Read::Ok { status } if status["result"] == "running"));
        write_status(&root, &RUNNING.replace("\"running\"", "\"done\""));
        next(&|r| matches!(r, Read::Ok { status } if status["result"] == "done"));
        std::fs::remove_file(root.join(STATUS_PATH)).unwrap();
        next(&|r| *r == Read::Missing);
    }

    fn event(seq: u32, ty: &str, extra: &str) -> String {
        let ts = format!("2026-11-07T11:{:02}:00Z", seq);
        format!(
            r#"{{"v":1,"seq":{seq},"ts":"{ts}","run":"r1","type":"{ty}","tone":"neutral"{extra}}}"#
        ) + "\n"
    }

    fn append(path: &Path, text: &str) {
        use std::io::Write;
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    fn now() -> f64 {
        progress_events::parse_ts("2026-11-07T12:00:00Z").unwrap()
    }

    #[test]
    fn the_tailer_replays_then_follows_and_waits_for_a_complete_line() {
        let dir = temp_dir("tail");
        let file = dir.join("r1.jsonl");
        append(
            &file,
            &(event(1, "run.started", r#","source":"session""#)
                + &event(2, "milestone.progress", r#","closed":1,"total":4"#)),
        );
        let mut tailer = Tailer::default();
        // The third event is half written.
        let half = event(3, "milestone.progress", r#","closed":2,"total":4"#);
        append(&file, &half[..half.len() / 2]);
        let (runs, unknown) = tailer.read(&dir, now());
        assert_eq!(unknown, None);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].state.closed, 1);
        assert_eq!(runs[0].source.as_deref(), Some("session"));
        // The rest arrives: it is folded once.
        append(&file, &half[half.len() / 2..]);
        let (runs, _) = tailer.read(&dir, now());
        assert_eq!(runs[0].state.closed, 2);
        // A truncated file is replayed.
        std::fs::write(&file, event(1, "run.started", "")).unwrap();
        let (runs, _) = tailer.read(&dir, now());
        assert_eq!(runs[0].state.closed, 0);
        // A removed file is forgotten.
        std::fs::remove_file(&file).unwrap();
        assert!(tailer.read(&dir, now()).0.is_empty());
    }

    #[test]
    fn the_tailer_lists_loops_and_sessions_and_hides_old_finished_runs() {
        let dir = temp_dir("runs");
        append(
            &dir.join("a.jsonl"),
            &(event(1, "run.started", r#","source":"loop""#)),
        );
        append(
            &dir.join("b.jsonl"),
            &(event(1, "run.started", r#","source":"session""#)
                + &event(2, "run.finished", r#","result":"done""#)),
        );
        let old = r#"{"v":1,"seq":1,"ts":"2026-11-01T10:00:00Z","run":"old","type":"run.finished","tone":"success","result":"done"}"#;
        append(&dir.join("old.jsonl"), &(old.to_owned() + "\n"));
        append(&dir.join("notes.txt"), "not an event file\n");
        let (runs, _) = Tailer::default().read(&dir, now());
        let sources: Vec<_> = runs.iter().map(|r| r.source.as_deref()).collect();
        assert_eq!(runs.len(), 2, "{sources:?}");
        assert!(sources.contains(&Some("loop")) && sources.contains(&Some("session")));
    }

    #[test]
    fn an_unknown_event_version_is_reported_and_keeps_the_events_source() {
        let root = temp_dir("v");
        let dir = root.join("events");
        append(
            &dir.join("x.jsonl"),
            "{\"v\":7,\"seq\":1,\"type\":\"run.started\"}\n",
        );
        let mut tailer = Tailer::default();
        // A status file does not hide the hint.
        write_status(&root, RUNNING);
        let read = read_all(&root, Some(&dir), &mut tailer);
        assert_eq!(
            read,
            Read::Events {
                runs: vec![],
                unknown_version: Some(7)
            }
        );
        let json = serde_json::to_value(&read).unwrap();
        assert_eq!(json["kind"], "events");
        assert_eq!(json["unknownVersion"], 7);
    }

    #[test]
    fn a_repository_with_a_status_file_but_no_events_falls_back_to_it() {
        let root = temp_dir("fallback");
        write_status(&root, RUNNING);
        let dir = root.join("events");
        let mut tailer = Tailer::default();
        assert!(matches!(
            read_all(&root, Some(&dir), &mut tailer),
            Read::Ok { .. }
        ));
        assert!(matches!(
            read_all(&root, None, &mut tailer),
            Read::Ok { .. }
        ));
        let empty = temp_dir("fallback-none");
        assert_eq!(
            read_all(&empty, Some(&empty.join("e")), &mut tailer),
            Read::Missing
        );
    }

    #[test]
    fn the_watch_replays_an_event_file_and_follows_it() {
        let root = temp_dir("events-watch");
        // The events directory does not exist yet: its creation is seen one level up.
        let dir = root.join("common").join(EVENTS_PATH);
        std::fs::create_dir_all(root.join("common")).unwrap();
        let (tx, rx) = mpsc::channel();
        let _handle = watch_with(root.clone(), Some(dir.clone()), move |r| {
            let _ = tx.send(r);
        });
        let next = |want: &dyn Fn(&Read) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok(r) if want(&r) => return r,
                    Ok(_) => {}
                    Err(_) => panic!("no matching update"),
                }
            }
        };
        next(&|r| *r == Read::Missing);
        let file = dir.join("r1.jsonl");
        std::fs::create_dir_all(&dir).unwrap();
        let fresh = |closed: u32| {
            // Timestamps of now, so the run is neither abandoned nor old.
            let secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            format!(
                r#"{{"v":1,"seq":1,"ts":"{0}","run":"r1","type":"milestone.progress","tone":"neutral","closed":{closed},"total":5}}"#,
                iso(secs)
            ) + "\n"
        };
        append(&file, &fresh(2));
        next(&|r| matches!(r, Read::Events { runs, .. } if runs[0].state.closed == 2));
        let more = fresh(3).replace("\"seq\":1", "\"seq\":2");
        append(&file, &more);
        next(&|r| matches!(r, Read::Events { runs, .. } if runs[0].state.closed == 3));
    }

    /// `YYYY-MM-DDTHH:MM:SSZ` of epoch seconds (civil from days).
    fn iso(secs: u64) -> String {
        let days = (secs / 86_400) as i64 + 719_468;
        let era = days.div_euclid(146_097);
        let doe = days - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + i64::from(month <= 2);
        let rem = secs % 86_400;
        format!(
            "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
            rem / 3600,
            rem % 3600 / 60,
            rem % 60
        )
    }
}
