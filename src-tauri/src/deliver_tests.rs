use super::*;
use crate::test_dir::TestDir;
use agent_events::{EVENTS_PATH, Reducer, RepoRef};
use std::time::Instant;

/// The sources of a repository with the legacy directory `dir` and no shared one: no test reads
/// the directory of the user.
fn legacy(dir: &Path) -> Sources {
    Sources {
        legacy: Some(dir.to_path_buf()),
        ..Sources::default()
    }
}

fn temp_dir(name: &str) -> TestDir {
    TestDir::new(&format!("poimen-deliver-{name}-"))
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
    let _handle = watch_with(
        root.to_path_buf(),
        |_| Sources::default(),
        move |r| {
            let _ = tx.send(r);
        },
    );
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

/// A `progress` event of the run `r1` with `done` of `total` for the run subject.
fn progress(seq: u32, done: u32) -> String {
    event(seq, "progress", &format!(r#","done":{done},"total":4"#))
}

fn event(seq: u32, ty: &str, extra: &str) -> String {
    let ts = format!("2026-11-07T11:{:02}:00Z", seq);
    format!(r#"{{"v":1,"seq":{seq},"ts":"{ts}","run":"r1","type":"{ty}","tone":"neutral"{extra}}}"#)
        + "\n"
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
    agent_events::parse_ts("2026-11-07T12:00:00Z").unwrap()
}

#[test]
fn the_tailer_replays_then_follows_and_waits_for_a_complete_line() {
    let dir = temp_dir("tail");
    let file = dir.join("r1.jsonl");
    append(&file, &(event(1, "run.started", "") + &progress(2, 1)));
    let mut tailer = Tailer::default();
    // The third event is half written.
    let half = progress(3, 2);
    append(&file, &half[..half.len() / 2]);
    let (runs, unknown) = tailer.read(&legacy(&dir), now());
    assert_eq!(unknown, None);
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].state.progress.as_ref().unwrap()["done"], 1);
    // The rest arrives: it is folded once.
    append(&file, &half[half.len() / 2..]);
    let (runs, _) = tailer.read(&legacy(&dir), now());
    assert_eq!(runs[0].state.progress.as_ref().unwrap()["done"], 2);
    // A truncated file is replayed.
    std::fs::write(&file, event(1, "run.started", "")).unwrap();
    let (runs, _) = tailer.read(&legacy(&dir), now());
    assert!(runs[0].state.progress.is_none());
    // A removed file is forgotten.
    std::fs::remove_file(&file).unwrap();
    assert!(tailer.read(&legacy(&dir), now()).0.is_empty());
}

#[test]
fn the_tailer_lists_runs_and_hides_old_finished_runs_and_foreign_files() {
    let dir = temp_dir("runs");
    append(&dir.join("a.jsonl"), &event(1, "run.started", ""));
    append(
        &dir.join("b.jsonl"),
        &(event(1, "run.started", "") + &event(2, "run.finished", r#","outcome":"succeeded""#)),
    );
    let old = r#"{"v":1,"seq":1,"ts":"2026-11-01T10:00:00Z","run":"old","type":"run.finished","tone":"success","outcome":"succeeded"}"#;
    append(&dir.join("old.jsonl"), &(old.to_owned() + "\n"));
    append(&dir.join("notes.txt"), "not an event file\n");
    append(&dir.join(".hidden.jsonl"), &event(1, "run.started", ""));
    append(&dir.join("has space.jsonl"), &event(1, "run.started", ""));
    let (runs, _) = Tailer::default().read(&legacy(&dir), now());
    let mut status: Vec<_> = runs.iter().map(|r| r.state.status.as_str()).collect();
    status.sort_unstable();
    assert_eq!(status, ["finished", "running"]);
}

const DAY: u64 = 86_400;

fn real_now() -> u64 {
    now_secs() as u64
}

/// A run file of `run` with the subject `(kind, id)`; its last event `age` seconds ago, and
/// finished unless `running`. The file's modification time is that of the last event.
fn write_run(dir: &Path, run: &str, subject: Option<(&str, &str)>, age: u64, running: bool) {
    let at = real_now();
    let subject = subject.map_or(String::new(), |(k, i)| {
        format!(r#","subject":{{"kind":"{k}","id":"{i}"}}"#)
    });
    let mut text = format!(
        r#"{{"v":1,"seq":1,"ts":"{}","run":"{run}","type":"run.started","tone":"neutral"{subject}}}"#,
        iso(at - age - 60)
    ) + "\n";
    if !running {
        text += &format!(
            r#"{{"v":1,"seq":2,"ts":"{}","run":"{run}","type":"run.finished","tone":"success","outcome":"succeeded"}}"#,
            iso(at - age)
        );
        text += "\n";
    }
    let path = dir.join(format!("{run}.jsonl"));
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(&path, text).unwrap();
    let file = std::fs::File::options().write(true).open(&path).unwrap();
    file.set_modified(UNIX_EPOCH + Duration::from_secs(at - age))
        .unwrap();
}

fn ids(runs: &[Run]) -> Vec<&str> {
    let mut ids: Vec<_> = runs.iter().map(|r| r.state.run.as_str()).collect();
    ids.sort_unstable();
    ids
}

#[test]
fn earlier_runs_of_a_shown_subject_are_kept_and_the_others_dropped() {
    let dir = temp_dir("subject");
    let ms = Some(("github.milestone", "M16"));
    write_run(&dir, "cur", ms, 3600, true);
    write_run(&dir, "old1", ms, 3 * DAY, false);
    write_run(&dir, "old2", ms, 10 * DAY, false);
    write_run(&dir, "ancient", ms, 20 * DAY, false);
    write_run(
        &dir,
        "other",
        Some(("github.milestone", "M15")),
        3 * DAY,
        false,
    );
    write_run(&dir, "bare", None, 3 * DAY, false);
    let mut tailer = Tailer::default();
    let (runs, _) = tailer.read(&legacy(&dir), now_secs());
    assert_eq!(ids(&runs), ["cur", "old1", "old2"]);
    // Only the files of the subject are replayed; the others had their first line read.
    assert_eq!(tailer.files.len(), 3);
    // The same again reads nothing new and gives the same answer.
    assert_eq!(
        ids(&tailer.read(&legacy(&dir), now_secs()).0),
        ["cur", "old1", "old2"]
    );
}

#[test]
fn nothing_of_a_subject_is_kept_when_none_of_its_runs_is_shown() {
    let dir = temp_dir("subject-gone");
    let ms = Some(("github.milestone", "M16"));
    write_run(&dir, "old1", ms, 3 * DAY, false);
    write_run(&dir, "old2", ms, 5 * DAY, false);
    let mut tailer = Tailer::default();
    let (runs, _) = tailer.read(&legacy(&dir), now_secs());
    assert!(runs.is_empty());
    assert!(tailer.files.is_empty());
    // A run of the subject starts: the earlier ones come back.
    write_run(&dir, "new", ms, 60, true);
    assert_eq!(
        ids(&tailer.read(&legacy(&dir), now_secs()).0),
        ["new", "old1", "old2"]
    );
    // It ends more than a day ago: they go again.
    std::fs::remove_file(dir.join("new.jsonl")).unwrap();
    assert!(tailer.read(&legacy(&dir), now_secs()).0.is_empty());
    assert!(tailer.files.is_empty());
}

fn run_at(i: usize, subject: Option<&str>, age: u64) -> Run {
    let at = real_now();
    let subject = subject.map_or(String::new(), |id| {
        format!(r#","subject":{{"kind":"github.milestone","id":"{id}"}}"#)
    });
    let mut r = Reducer::new();
    r.push_line(&format!(
        r#"{{"v":1,"seq":1,"ts":"{}","run":"r{i}","type":"run.started","tone":"neutral"{subject}}}"#,
        iso(at - age)
    ));
    r.push_line(&format!(
        r#"{{"v":1,"seq":2,"ts":"{}","run":"r{i}","type":"run.finished","tone":"success","outcome":"succeeded"}}"#,
        iso(at - age)
    ));
    r.snapshot(now_secs(), &|_| true).unwrap()
}

#[test]
fn a_subject_keeps_its_newest_fifty_runs_within_fourteen_days() {
    // One run inside the present limits shows the subject; 70 earlier runs, one a minute apart
    // and the oldest 15 days back.
    let mut runs = vec![run_at(0, Some("M1"), 60)];
    runs.extend((1..=70).map(|i| run_at(i, Some("M1"), 2 * DAY + i as u64 * 60)));
    runs.push(run_at(71, Some("M1"), 15 * DAY));
    // Another subject with only old runs, and runs without a subject: the present limits,
    // twelve of them.
    runs.push(run_at(72, Some("M2"), 3 * DAY));
    runs.extend((100..130).map(|i| run_at(i, None, 60 + i as u64)));
    let kept = recent(runs, now_secs());
    let m1: Vec<_> = kept.iter().filter(|r| subject_of(r).is_some()).collect();
    assert_eq!(m1.len(), agent_events::MAX_SUBJECT_RUNS);
    assert!(m1.iter().any(|r| r.state.run == "r0"));
    assert!(m1.iter().any(|r| r.state.run == "r49"));
    assert!(
        m1.iter()
            .all(|r| r.state.run != "r50" && r.state.run != "r71")
    );
    assert!(kept.iter().all(|r| r.state.run != "r72"));
    assert_eq!(kept.len() - m1.len(), MAX_RUNS - 1);
}

#[test]
fn the_fourteen_day_limit_counts_from_the_last_event() {
    let runs = vec![
        run_at(0, Some("M1"), 60),
        run_at(1, Some("M1"), 13 * DAY),
        run_at(2, Some("M1"), 15 * DAY),
    ];
    assert_eq!(ids(&recent(runs, now_secs())), ["r0", "r1"]);
}

#[test]
fn a_kept_older_run_is_not_running() {
    let dir = temp_dir("subject-status");
    let ms = Some(("github.milestone", "M16"));
    write_run(&dir, "cur", ms, 60, true);
    // Its writer never finished: abandoned, never `running`.
    write_run(&dir, "left", ms, 4 * DAY, true);
    let (runs, _) = Tailer::default().read(&legacy(&dir), now_secs());
    let left = runs.iter().find(|r| r.state.run == "left").unwrap();
    assert_ne!(left.state.status, "running");
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
    let read = read_all(&root, &legacy(&dir), &mut tailer);
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
        read_all(&root, &legacy(&dir), &mut tailer),
        Read::Ok { .. }
    ));
    assert!(matches!(
        read_all(&root, &Sources::default(), &mut tailer),
        Read::Ok { .. }
    ));
    let empty = temp_dir("fallback-none");
    assert_eq!(
        read_all(&empty, &legacy(&empty.join("e")), &mut tailer),
        Read::Missing
    );
}

#[test]
fn the_watch_replays_an_event_file_and_follows_it() {
    let root = temp_dir("events-watch");
    // The events directory does not exist yet: its creation is seen one level up.
    let dir = root.join("common").join(EVENTS_PATH);
    std::fs::create_dir_all(root.join("common")).unwrap();
    let sources = legacy(&dir);
    let (tx, rx) = mpsc::channel();
    let _handle = watch_with(
        root.to_path_buf(),
        |_| sources,
        move |r| {
            let _ = tx.send(r);
        },
    );
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
            r#"{{"v":1,"seq":1,"ts":"{0}","run":"r1","type":"progress","tone":"neutral","done":{closed},"total":5}}"#,
            iso(secs)
        ) + "\n"
    };
    append(&file, &fresh(2));
    next(
        &|r| matches!(r, Read::Events { runs, .. } if runs[0].state.progress.as_ref().is_some_and(|p| p["done"] == 2)),
    );
    let more = fresh(3).replace("\"seq\":1", "\"seq\":2");
    append(&file, &more);
    next(
        &|r| matches!(r, Read::Events { runs, .. } if runs[0].state.progress.as_ref().is_some_and(|p| p["done"] == 3)),
    );
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

/// Sources with a shared and a legacy directory of the repository `o/r` below `root`.
fn both(root: &Path) -> Sources {
    let common = root.join("repo/.git");
    Sources {
        shared: Some(root.join("shared")),
        legacy: Some(agent_events::legacy_dir(&common)),
        repo: Some(RepoRef {
            top: root.join("repo").to_string_lossy().into_owned(),
            common,
            name: "o/r".into(),
        }),
        context: Context::default(),
    }
}

/// A running run started now, with `extra` fields of `run.started`.
fn started(run: &str, extra: &str) -> String {
    format!(
        r#"{{"v":1,"seq":1,"ts":"{}","run":"{run}","type":"run.started","tone":"neutral"{extra}}}"#,
        iso(real_now())
    ) + "\n"
}

#[test]
fn the_shared_directory_shows_the_runs_of_this_repository_only() {
    let root = temp_dir("shared");
    let sources = both(&root);
    let shared = sources.shared.clone().unwrap();
    let legacy = sources.legacy.clone().unwrap();
    append(
        &shared.join("ours.jsonl"),
        &started("ours", r#","origin":{"repo":"o/r","worktree":"/gone"}"#),
    );
    append(
        &shared.join("theirs.jsonl"),
        &started("theirs", r#","origin":{"repo":"o/other"}"#),
    );
    append(&shared.join("nobody.jsonl"), &started("nobody", ""));
    let mut tailer = Tailer::default();
    let Read::Events { runs, .. } = read_all(&root, &sources, &mut tailer) else {
        panic!("no events");
    };
    assert_eq!(ids(&runs), ["ours"]);
    // The legacy directory still works, with or without an origin.
    append(&legacy.join("old.jsonl"), &started("old", ""));
    assert_eq!(ids(&tailer.read(&sources, now_secs()).0), ["old", "ours"]);
}

#[test]
fn a_run_in_both_directories_is_one_run_folded_by_seq() {
    let root = temp_dir("both");
    let sources = both(&root);
    let first = started("r1", r#","origin":{"repo":"o/r"}"#);
    let step = |seq: u32, done: u32| {
        format!(
            r#"{{"v":1,"seq":{seq},"ts":"{}","run":"r1","type":"progress","tone":"neutral","done":{done},"total":9}}"#,
            iso(real_now())
        ) + "\n"
    };
    let shared = sources.shared.clone().unwrap().join("r1.jsonl");
    let legacy = sources.legacy.clone().unwrap().join("r1.jsonl");
    // The shared file wins a seq both have; the legacy one fills the gap.
    append(&shared, &(first.clone() + &step(3, 3)));
    append(&legacy, &(first + &step(2, 2) + &step(3, 1)));
    let mut tailer = Tailer::default();
    let (runs, _) = tailer.read(&sources, now_secs());
    assert_eq!(runs.len(), 1);
    let seqs: Vec<i64> = runs[0].events.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, [1, 2, 3]);
    assert_eq!(runs[0].state.progress.as_ref().unwrap()["done"], 3);
    append(&legacy, &step(4, 4));
    let (runs, _) = tailer.read(&sources, now_secs());
    assert_eq!(runs[0].state.progress.as_ref().unwrap()["done"], 4);
    assert!(runs[0].state.gaps.is_empty());
}

#[test]
fn a_run_without_origin_belongs_to_the_repository_of_its_terminal() {
    let root = temp_dir("terminal");
    let top = root.join("repo");
    std::fs::create_dir_all(&top).unwrap();
    crate::git::run_bytes(&top.to_string_lossy(), &["init", "-q"]).unwrap();
    let mut sources = Sources {
        shared: Some(root.join("shared")),
        repo: RepoRef::of(&top.to_string_lossy()),
        ..Sources::default()
    };
    let cwd = top.to_string_lossy().into_owned();
    sources.context.terminal_dir = Some(std::sync::Arc::new(move |run: &str| {
        (run == "mine").then(|| cwd.clone())
    }));
    let shared = sources.shared.clone().unwrap();
    append(&shared.join("mine.jsonl"), &started("mine", ""));
    append(&shared.join("nobody.jsonl"), &started("nobody", ""));
    let (runs, _) = Tailer::default().read(&sources, now_secs());
    assert_eq!(ids(&runs), ["mine"]);
}

#[test]
fn the_watch_follows_the_shared_directory() {
    let root = temp_dir("shared-watch");
    // Neither directory exists yet.
    let sources = both(&root);
    let shared = sources.shared.clone().unwrap();
    let (tx, rx) = mpsc::channel();
    let _handle = watch_with(
        root.to_path_buf(),
        |_| sources,
        move |r| {
            let _ = tx.send(r);
        },
    );
    let wait = || rx.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(wait(), Read::Missing);
    append(
        &shared.join("r1.jsonl"),
        &started("r1", r#","origin":{"repo":"o/r"}"#),
    );
    assert!(matches!(wait(), Read::Events { runs, .. } if runs[0].state.run == "r1"));
}
