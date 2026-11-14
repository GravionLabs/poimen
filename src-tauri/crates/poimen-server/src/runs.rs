//! Progress events of the runs in the repositories of the server's spaces (#692), for the app's
//! Deliver panel and Events tab of a remote machine. The files are those of the agent-events
//! `jsonl-file` binding, `<git-common-dir>/.agents/events/<run>.jsonl` (ADR 0002 of
//! GravionLabs/skills), read with the shared [`LineTail`] of `poimen_core::agent_events`: each file
//! from its start, then what it gained, up to the last complete line (a half-written last line
//! waits for the next read). The server does not fold the events (the app's reducer does); it
//! forwards each complete JSON object line unchanged as `run.events`.
//!
//! Only while a subscription asked for `run_events`: then the server reads the event directories
//! every [`POLL`] (a `stat` per directory and file; no watcher dependency, nothing runs without
//! such a client) and keeps the events of the recent files, so a new subscription gets them from
//! the start before it follows ([`Runs::replay`]). Without one the kept events are dropped.
//! Read-only: the server never writes an event file.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use poimen_core::agent_events::{self, EVENTS_PATH, LineTail, MAX_EVENTS_BYTES};
use poimen_protocol::Event;
use serde_json::Value;

/// How often the event directories are read while a client follows them.
pub(crate) const POLL: Duration = Duration::from_millis(500);

/// A file not modified for this long is not read (unless it is already followed); the app hides
/// runs that ended a day ago, so their events are of no use.
const RECENT: Duration = Duration::from_secs(25 * 3600);

/// At most this many events go into one `run.events` line.
const CHUNK: usize = 500;

/// How long a directory's repository is trusted before git is asked again (a space whose
/// repository is cloned later, a removed directory).
const RESOLVE_TTL: Duration = Duration::from_secs(30);

/// One followed run file: its run id, where reading stands and every event read so far.
struct RunFile {
    run: String,
    tail: LineTail,
    events: Vec<Value>,
}

/// One repository: the `root` the client sees (a toplevel) and its events directory.
struct Repo {
    root: String,
    files: BTreeMap<PathBuf, RunFile>,
}

/// The run files of the followed repositories, keyed by their events directory.
#[derive(Default)]
pub(crate) struct Runs {
    repos: BTreeMap<PathBuf, Repo>,
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

impl Runs {
    /// Follows `repos` (`(root, events dir)`; a directory listed twice is followed once, by the
    /// first root) and reads what their run files gained: the new events, per run in file order.
    /// A repository that is no longer listed and a file that is gone are forgotten.
    pub(crate) fn scan(&mut self, repos: &[(String, PathBuf)], now: SystemTime) -> Vec<Event> {
        let mut wanted: BTreeMap<&Path, &str> = BTreeMap::new();
        for (root, dir) in repos {
            wanted.entry(dir.as_path()).or_insert(root.as_str());
        }
        self.repos
            .retain(|dir, _| wanted.contains_key(dir.as_path()));
        let mut out = Vec::new();
        for (dir, root) in wanted {
            let repo = self.repos.entry(dir.to_path_buf()).or_insert_with(|| Repo {
                root: root.to_owned(),
                files: BTreeMap::new(),
            });
            out.extend(repo.scan(dir, now));
        }
        out
    }

    /// The events of every followed run from the start, for a new subscription.
    pub(crate) fn replay(&self) -> Vec<Event> {
        self.repos
            .values()
            .flat_map(|repo| {
                repo.files
                    .values()
                    .flat_map(|f| chunks(&repo.root, &f.run, &f.events))
            })
            .collect()
    }

    /// Forgets every file (no client follows the runs any more).
    pub(crate) fn clear(&mut self) {
        self.repos.clear();
    }
}

impl Repo {
    fn scan(&mut self, dir: &Path, now: SystemTime) -> Vec<Event> {
        let mut seen = Vec::new();
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
            let recent = meta
                .modified()
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .is_none_or(|age| age < RECENT);
            if !recent && !self.files.contains_key(&path) {
                continue;
            }
            seen.push(path.clone());
            let file = self.files.entry(path.clone()).or_insert_with(|| RunFile {
                run,
                tail: LineTail::default(),
                events: Vec::new(),
            });
            let Ok(tailed) = file.tail.read(&path, meta.len()) else {
                continue;
            };
            if tailed.reset {
                file.events.clear();
            }
            let new: Vec<Value> = tailed
                .lines
                .iter()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .filter(Value::is_object)
                .collect();
            if new.is_empty() {
                continue;
            }
            out.extend(chunks(&self.root, &file.run, &new));
            file.events.extend(new);
        }
        self.files.retain(|path, _| seen.contains(path));
        out
    }
}

/// The repositories of the server's directories (space and terminal cwds), asked of git at most
/// every [`RESOLVE_TTL`] per directory.
#[derive(Default)]
pub(crate) struct Repos {
    cache: HashMap<String, (Option<(String, String)>, Instant)>,
}

impl Repos {
    /// `(root, events dir)` of each distinct repository among `dirs` (a linked worktree counts as
    /// its main repository, the root is the toplevel of the first directory of it). `roots` gives
    /// the toplevel and the common git dir of a directory (`poimen_core::git::roots`).
    pub(crate) fn resolve(
        &mut self,
        dirs: &[String],
        now: Instant,
        roots: impl Fn(&str) -> Option<(String, String)>,
    ) -> Vec<(String, PathBuf)> {
        self.cache.retain(|dir, _| dirs.contains(dir));
        let mut out: Vec<(String, PathBuf)> = Vec::new();
        for dir in dirs {
            let fresh = self
                .cache
                .get(dir)
                .is_some_and(|(_, at)| now.duration_since(*at) < RESOLVE_TTL);
            if !fresh {
                self.cache.insert(dir.clone(), (roots(dir), now));
            }
            if let Some((Some((top, common)), _)) = self.cache.get(dir) {
                let events = Path::new(common).join(EVENTS_PATH);
                if !out.iter().any(|(_, d)| *d == events) {
                    out.push((top.clone(), events));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "poimen-server-runs-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn line(seq: u32, ty: &str) -> String {
        format!(r#"{{"v":1,"seq":{seq},"run":"r1","type":"{ty}"}}"#) + "\n"
    }

    fn append(path: &Path, text: &str) {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        f.write_all(text.as_bytes()).unwrap();
    }

    /// The `(run, seq)` of each forwarded event, in order.
    fn seqs(events: &[Event]) -> Vec<(String, i64)> {
        events
            .iter()
            .flat_map(|e| match e {
                Event::RunEvents { run, events, .. } => events
                    .iter()
                    .map(|v| (run.clone(), v["seq"].as_i64().unwrap()))
                    .collect::<Vec<_>>(),
                other => panic!("not a run event: {other:?}"),
            })
            .collect()
    }

    fn pair(run: &str, seq: i64) -> (String, i64) {
        (run.to_owned(), seq)
    }

    #[test]
    fn a_new_file_is_read_from_the_start_and_an_appended_line_once() {
        let dir = temp_dir("new");
        let repos = vec![("/repo".to_owned(), dir.clone())];
        let mut runs = Runs::default();
        assert!(runs.scan(&repos, SystemTime::now()).is_empty());
        append(
            &dir.join("r1.jsonl"),
            &(line(1, "run.started") + &line(2, "progress")),
        );
        let first = runs.scan(&repos, SystemTime::now());
        assert_eq!(seqs(&first), [pair("r1", 1), pair("r1", 2)]);
        let Event::RunEvents { root, events, .. } = &first[0] else {
            unreachable!()
        };
        assert_eq!(root, "/repo");
        assert_eq!(events[0]["type"], "run.started");
        // Nothing new, nothing sent.
        assert!(runs.scan(&repos, SystemTime::now()).is_empty());
        append(&dir.join("r1.jsonl"), &line(3, "progress"));
        assert_eq!(seqs(&runs.scan(&repos, SystemTime::now())), [pair("r1", 3)]);
    }

    #[test]
    fn an_incomplete_last_line_waits_until_it_is_complete() {
        let dir = temp_dir("half");
        let repos = vec![("/repo".to_owned(), dir.clone())];
        let file = dir.join("r1.jsonl");
        let half = line(2, "progress");
        append(&file, &(line(1, "run.started") + &half[..10]));
        let mut runs = Runs::default();
        assert_eq!(seqs(&runs.scan(&repos, SystemTime::now())), [pair("r1", 1)]);
        assert!(runs.scan(&repos, SystemTime::now()).is_empty());
        append(&file, &half[10..]);
        assert_eq!(seqs(&runs.scan(&repos, SystemTime::now())), [pair("r1", 2)]);
    }

    #[test]
    fn the_replay_holds_every_event_read_so_far_per_run() {
        let dir = temp_dir("replay");
        let repos = vec![("/repo".to_owned(), dir.clone())];
        append(&dir.join("a.jsonl"), &line(1, "run.started"));
        append(
            &dir.join("b.jsonl"),
            &(line(1, "run.started") + "not json\n" + "[1]\n" + &line(2, "run.finished")),
        );
        let mut runs = Runs::default();
        runs.scan(&repos, SystemTime::now());
        append(&dir.join("a.jsonl"), &line(2, "progress"));
        runs.scan(&repos, SystemTime::now());
        assert_eq!(
            seqs(&runs.replay()),
            [pair("a", 1), pair("a", 2), pair("b", 1), pair("b", 2)]
        );
        runs.clear();
        assert!(runs.replay().is_empty());
    }

    #[test]
    fn foreign_old_and_removed_files_and_dropped_repositories_are_not_followed() {
        let dir = temp_dir("foreign");
        let repos = vec![("/repo".to_owned(), dir.clone())];
        append(&dir.join("notes.txt"), &line(1, "run.started"));
        append(&dir.join(".hidden.jsonl"), &line(1, "run.started"));
        append(&dir.join("old.jsonl"), &line(1, "run.started"));
        std::fs::create_dir(dir.join("d.jsonl")).unwrap();
        let mut runs = Runs::default();
        // Two days on, the file is old.
        let later = SystemTime::now() + Duration::from_secs(48 * 3600);
        assert!(runs.scan(&repos, later).is_empty());
        append(&dir.join("r1.jsonl"), &line(1, "run.started"));
        assert_eq!(seqs(&runs.scan(&repos, SystemTime::now())).len(), 2);
        std::fs::remove_file(dir.join("r1.jsonl")).unwrap();
        runs.scan(&repos, SystemTime::now());
        assert_eq!(seqs(&runs.replay()), [pair("old", 1)]);
        runs.scan(&[], SystemTime::now());
        assert!(runs.replay().is_empty());
    }

    #[test]
    fn a_truncated_file_is_read_again_from_its_start() {
        let dir = temp_dir("trunc");
        let repos = vec![("/repo".to_owned(), dir.clone())];
        let file = dir.join("r1.jsonl");
        append(&file, &(line(1, "run.started") + &line(2, "progress")));
        let mut runs = Runs::default();
        runs.scan(&repos, SystemTime::now());
        std::fs::write(&file, line(1, "run.started")).unwrap();
        assert_eq!(seqs(&runs.scan(&repos, SystemTime::now())), [pair("r1", 1)]);
        assert_eq!(seqs(&runs.replay()), [pair("r1", 1)]);
    }

    #[test]
    fn a_long_run_goes_out_in_chunks() {
        let events: Vec<Value> = (1..=CHUNK as i64 + 1).map(|n| json!({"seq": n})).collect();
        let out = chunks("/r", "x", &events);
        assert_eq!(out.len(), 2);
        assert_eq!(seqs(&out).len(), CHUNK + 1);
    }

    #[test]
    fn repositories_are_resolved_once_per_common_dir_and_cached() {
        let calls = Cell::new(0);
        let roots = |dir: &str| {
            calls.set(calls.get() + 1);
            match dir {
                "/src/api" | "/src/api/sub" => Some(("/src/api".into(), "/src/api/.git".into())),
                "/src/api-pr1" => Some(("/src/api-pr1".into(), "/src/api/.git".into())),
                _ => None,
            }
        };
        let mut repos = Repos::default();
        let dirs: Vec<String> = ["/src/api-pr1", "/src/api", "/tmp", "/src/api/sub"]
            .map(String::from)
            .to_vec();
        let t0 = Instant::now();
        let want = vec![(
            "/src/api-pr1".to_owned(),
            Path::new("/src/api/.git").join(EVENTS_PATH),
        )];
        assert_eq!(repos.resolve(&dirs, t0, roots), want);
        assert_eq!(calls.get(), 4);
        assert_eq!(
            repos.resolve(&dirs, t0 + Duration::from_secs(1), roots),
            want
        );
        assert_eq!(calls.get(), 4, "cached");
        repos.resolve(&dirs, t0 + RESOLVE_TTL, roots);
        assert_eq!(calls.get(), 8, "asked again after the ttl");
    }
}
