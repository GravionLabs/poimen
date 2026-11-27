use super::*;
use serde_json::json;

/// A fresh legacy event directory (`<common>/.agents/events`); keep the guard, it removes the
/// directory on drop.
fn temp_dir(name: &str) -> (tempfile::TempDir, PathBuf) {
    let guard = tempfile::Builder::new()
        .prefix(&format!("poimen-server-runs-{name}-"))
        .tempdir()
        .unwrap();
    let dir = agent_events::legacy_dir(guard.path());
    std::fs::create_dir_all(&dir).unwrap();
    (guard, dir)
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

/// The one repository `/repo` with the legacy directory `dir`.
fn sources(dir: &Path) -> Sources {
    let common = dir.parent().and_then(Path::parent).unwrap();
    Sources {
        repos: vec![repo("/repo", common, "o/r")],
        ..Sources::default()
    }
}

/// A repository with the toplevel `top` and the common git dir `common`.
fn repo(top: &str, common: &Path, name: &str) -> RepoRef {
    RepoRef {
        top: top.to_owned(),
        common: common.to_path_buf(),
        name: name.to_owned(),
    }
}

fn scan(runs: &mut Runs, sources: &Sources) -> Vec<Event> {
    runs.scan(sources, SystemTime::now(), |_| None)
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
    let (_guard, dir) = temp_dir("new");
    let repos = sources(&dir);
    let mut runs = Runs::default();
    assert!(scan(&mut runs, &repos).is_empty());
    append(
        &dir.join("r1.jsonl"),
        &(line(1, "run.started") + &line(2, "progress")),
    );
    let first = scan(&mut runs, &repos);
    assert_eq!(seqs(&first), [pair("r1", 1), pair("r1", 2)]);
    let Event::RunEvents { root, events, .. } = &first[0] else {
        unreachable!()
    };
    assert_eq!(root, "/repo");
    assert_eq!(events[0]["type"], "run.started");
    // Nothing new, nothing sent.
    assert!(scan(&mut runs, &repos).is_empty());
    append(&dir.join("r1.jsonl"), &line(3, "progress"));
    assert_eq!(seqs(&scan(&mut runs, &repos)), [pair("r1", 3)]);
}

#[test]
fn an_incomplete_last_line_waits_until_it_is_complete() {
    let (_guard, dir) = temp_dir("half");
    let repos = sources(&dir);
    let file = dir.join("r1.jsonl");
    let half = line(2, "progress");
    append(&file, &(line(1, "run.started") + &half[..10]));
    let mut runs = Runs::default();
    assert_eq!(seqs(&scan(&mut runs, &repos)), [pair("r1", 1)]);
    assert!(scan(&mut runs, &repos).is_empty());
    append(&file, &half[10..]);
    assert_eq!(seqs(&scan(&mut runs, &repos)), [pair("r1", 2)]);
}

#[test]
fn the_replay_holds_every_event_read_so_far_per_run() {
    let (_guard, dir) = temp_dir("replay");
    let repos = sources(&dir);
    append(&dir.join("a.jsonl"), &line(1, "run.started"));
    append(
        &dir.join("b.jsonl"),
        &(line(1, "run.started") + "not json\n" + "[1]\n" + &line(2, "run.finished")),
    );
    let mut runs = Runs::default();
    scan(&mut runs, &repos);
    append(&dir.join("a.jsonl"), &line(2, "progress"));
    scan(&mut runs, &repos);
    assert_eq!(
        seqs(&runs.replay()),
        [pair("a", 1), pair("a", 2), pair("b", 1), pair("b", 2)]
    );
    runs.clear();
    assert!(runs.replay().is_empty());
}

#[test]
fn foreign_old_and_removed_files_and_dropped_repositories_are_not_followed() {
    let (_guard, dir) = temp_dir("foreign");
    let repos = sources(&dir);
    append(&dir.join("notes.txt"), &line(1, "run.started"));
    append(&dir.join(".hidden.jsonl"), &line(1, "run.started"));
    append(&dir.join("old.jsonl"), &line(1, "run.started"));
    std::fs::create_dir(dir.join("d.jsonl")).unwrap();
    let mut runs = Runs::default();
    // Two days on, the file is old.
    let later = SystemTime::now() + Duration::from_secs(48 * 3600);
    assert!(runs.scan(&repos, later, |_| None).is_empty());
    append(&dir.join("r1.jsonl"), &line(1, "run.started"));
    assert_eq!(seqs(&scan(&mut runs, &repos)).len(), 2);
    std::fs::remove_file(dir.join("r1.jsonl")).unwrap();
    scan(&mut runs, &repos);
    assert_eq!(seqs(&runs.replay()), [pair("old", 1)]);
    scan(&mut runs, &Sources::default());
    assert!(runs.replay().is_empty());
}

/// A run file whose first line names the subject `M16`, last changed `days` days ago.
fn subject_file(dir: &Path, run: &str, subject: bool, days: u64) {
    let subject = if subject {
        r#","subject":{"kind":"github.milestone","id":"M16"}"#
    } else {
        ""
    };
    let path = dir.join(format!("{run}.jsonl"));
    append(
        &path,
        &(format!(r#"{{"v":1,"seq":1,"run":"{run}","type":"run.started"{subject}}}"#) + "\n"),
    );
    append(&path, &line(2, "progress"));
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(SystemTime::now() - Duration::from_secs(days * 86_400))
        .unwrap();
}

#[test]
fn older_files_follow_only_while_their_subject_is_shown_and_within_fourteen_days() {
    let (_guard, dir) = temp_dir("subject");
    let repos = sources(&dir);
    subject_file(&dir, "earlier", true, 5);
    subject_file(&dir, "bare", false, 5);
    subject_file(&dir, "ancient", true, 20);
    let mut runs = Runs::default();
    // No file of the subject is recent: none is read.
    assert!(scan(&mut runs, &repos).is_empty());
    subject_file(&dir, "current", true, 0);
    let mut got = seqs(&scan(&mut runs, &repos));
    got.sort();
    assert_eq!(
        got,
        [
            pair("current", 1),
            pair("current", 2),
            pair("earlier", 1),
            pair("earlier", 2)
        ]
    );
    // The replay holds them; once the recent file is gone the earlier one is forgotten.
    assert_eq!(runs.replay().len(), 2);
    std::fs::remove_file(dir.join("current.jsonl")).unwrap();
    assert!(scan(&mut runs, &repos).is_empty());
    assert!(runs.replay().is_empty());
}

#[test]
fn a_truncated_file_is_read_again_from_its_start() {
    let (_guard, dir) = temp_dir("trunc");
    let repos = sources(&dir);
    let file = dir.join("r1.jsonl");
    append(&file, &(line(1, "run.started") + &line(2, "progress")));
    let mut runs = Runs::default();
    scan(&mut runs, &repos);
    std::fs::write(&file, line(1, "run.started")).unwrap();
    assert_eq!(seqs(&scan(&mut runs, &repos)), [pair("r1", 1)]);
    assert_eq!(seqs(&runs.replay()), [pair("r1", 1)]);
}

#[test]
fn a_long_run_goes_out_in_chunks() {
    let events: Vec<Value> = (1..=CHUNK as i64 + 1).map(|n| json!({"seq": n})).collect();
    let out = chunks("/r", "x", &events);
    assert_eq!(out.len(), 2);
    assert_eq!(seqs(&out).len(), CHUNK + 1);
}

/// A repository `/repo` named `example/a` with a shared and a legacy directory.
fn both(guard: &Path) -> (Sources, PathBuf, PathBuf) {
    let shared = guard.join("shared");
    let common = guard.join("common");
    let legacy = agent_events::legacy_dir(&common);
    for dir in [&shared, &legacy] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let sources = Sources {
        shared: Some(shared.clone()),
        repos: vec![repo("/repo", &common, "example/a")],
        ..Sources::default()
    };
    (sources, shared, legacy)
}

fn started(run: &str) -> String {
    format!(
        r#"{{"v":1,"seq":1,"run":"{run}","type":"run.started","origin":{{"repo":"example/a"}}}}"#
    ) + "\n"
}

#[test]
fn a_run_in_both_directories_is_forwarded_once_per_seq_and_in_order() {
    let guard = tempfile::tempdir().unwrap();
    let (sources, shared, legacy) = both(guard.path());
    let (shared, legacy) = (shared.join("r1.jsonl"), legacy.join("r1.jsonl"));
    // The shared file lacks seq 2, which only the legacy one has.
    append(&shared, &(started("r1") + &line(3, "progress")));
    append(&legacy, &(started("r1") + &line(2, "progress")));
    let mut runs = Runs::default();
    let want = [pair("r1", 1), pair("r1", 2), pair("r1", 3)];
    assert_eq!(seqs(&scan(&mut runs, &sources)), want);
    assert_eq!(seqs(&runs.replay()), want);
    append(&legacy, &line(3, "progress"));
    assert!(scan(&mut runs, &sources).is_empty());
    append(&legacy, &line(4, "progress"));
    assert_eq!(seqs(&scan(&mut runs, &sources)), [pair("r1", 4)]);
    append(&shared, &line(4, "progress"));
    assert!(scan(&mut runs, &sources).is_empty());
    // A replaced shared file does not send again what the legacy one holds.
    std::fs::write(&shared, started("r1")).unwrap();
    assert!(scan(&mut runs, &sources).is_empty());
    // The shared file goes away: nothing new, the run stays.
    std::fs::remove_file(&shared).unwrap();
    assert!(scan(&mut runs, &sources).is_empty());
    assert_eq!(seqs(&runs.replay()).len(), 4);
}

#[test]
fn a_shared_run_stays_followed_when_its_worktree_is_removed() {
    let guard = tempfile::tempdir().unwrap();
    let (mut sources, shared, _) = both(guard.path());
    sources
        .repos
        .push(repo("/other", Path::new("/other"), "o/x"));
    let file = shared.join("r1.jsonl");
    append(
        &file,
        "{\"v\":1,\"seq\":1,\"run\":\"r1\",\"type\":\"run.started\",\"origin\":{\"worktree\":\"/wt\"}}\n",
    );
    let mut runs = Runs::default();
    let there = runs.scan(&sources, SystemTime::now(), |_| {
        Some(repo("/wt", Path::new("/other"), "o/x"))
    });
    append(&file, &line(2, "progress"));
    let gone = scan(&mut runs, &sources);
    for events in [there, gone] {
        let Event::RunEvents { root, .. } = &events[0] else {
            unreachable!()
        };
        assert_eq!(root, "/other");
    }
}

/// The reader cases of the directory fixtures of agent-events: each listed run goes out once,
/// with the events of `seqs`.
#[test]
fn the_directory_fixtures_of_agent_events_are_followed_as_expected() {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../poimen-core/fixtures/agent-events/directory");
    for case in ["two-repositories", "shared-and-legacy"] {
        let dir = fixtures.join(case);
        let expected: Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("expected.json")).unwrap())
                .unwrap();
        // The legacy directory belongs to `example/a`; the fixtures are only read.
        let guard = tempfile::tempdir().unwrap();
        let legacy = agent_events::legacy_dir(guard.path());
        std::fs::create_dir_all(&legacy).unwrap();
        for entry in std::fs::read_dir(dir.join("legacy"))
            .into_iter()
            .flatten()
            .flatten()
        {
            std::fs::copy(entry.path(), legacy.join(entry.file_name())).unwrap();
        }
        let sources = Sources {
            shared: Some(dir.join("shared")),
            repos: vec![
                repo("/a", guard.path(), "example/a"),
                repo("/b", Path::new("/nowhere"), "example/b"),
            ],
            ..Sources::default()
        };
        // The files of a checkout are fresh, whatever the fixture's clock says.
        let mut got = seqs(&scan(&mut Runs::default(), &sources));
        got.sort();
        let want: Vec<_> = expected["runs"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|r| {
                let run = r["run"].as_str().unwrap();
                let seqs = r["seqs"].as_array().unwrap().iter();
                seqs.map(|s| pair(run, s.as_i64().unwrap()))
            })
            .collect();
        assert_eq!(got, want, "{case}");
    }
}
