use super::*;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/agent-events");

fn fixture(name: &str, ext: &str) -> String {
    std::fs::read_to_string(format!("{DIR}/{name}.{ext}")).unwrap()
}

fn now() -> f64 {
    parse_ts("2026-11-08T12:00:00Z").unwrap()
}

/// The tag recorded in `SOURCE.txt`, the one place that pins it.
fn pinned_tag() -> String {
    fixture("SOURCE", "txt")
        .lines()
        .find_map(|l| l.strip_prefix("tag: "))
        .unwrap()
        .trim()
        .to_owned()
}

#[test]
fn the_pinned_tag_is_recorded_in_source_txt() {
    assert_eq!(pinned_tag(), "v0.1.0");
}

/// Every reader fixture of agent-events: events in, run state out. `<case>.now` fixes the
/// clock (default 2026-11-08T10:30:00.000Z), no pid check.
#[test]
fn every_reader_fixture_of_agent_events_folds_to_its_state() {
    let mut names: Vec<String> = std::fs::read_dir(DIR)
        .unwrap()
        .filter_map(|e| {
            let name = e.unwrap().file_name().into_string().unwrap();
            name.strip_suffix(".jsonl").map(str::to_owned)
        })
        .collect();
    names.sort();
    assert_eq!(names.len(), 10, "{names:?}");
    for name in names {
        let clock = std::fs::read_to_string(format!("{DIR}/{name}.now"))
            .unwrap_or_else(|_| "2026-11-08T10:30:00.000Z".into());
        let at = parse_ts(clock.trim()).unwrap();
        let run = reduce(&fixture(&name, "jsonl"), at, &|_| true)
            .unwrap_or_else(|| panic!("{name}: no run"));
        let got = serde_json::to_value(&run.state).unwrap();
        let want: Value = serde_json::from_str(&fixture(&name, "state.json")).unwrap();
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn tone_host_and_rows_come_from_the_events() {
    let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:00:00Z","run":"r","type":"run.started","tone":"neutral","host":{"poimen.terminal":"t7","herdr.pane":"w1:p2","n":3},"agent":{"harness":"claude"}}
{"v":1,"seq":2,"ts":"2026-11-08T11:01:00Z","run":"r","type":"task.started","tone":"info","task":"t","subject":{"kind":"x","id":"1","title":"T"}}
{"v":1,"seq":3,"ts":"2026-11-08T11:02:00Z","run":"r","type":"step.started","tone":"info","task":"t","step":{"name":"implement","text":"green"}}
{"v":1,"seq":4,"ts":"2026-11-08T11:03:00Z","run":"r","type":"deliver.new","tone":"danger"}
{"v":1,"seq":5,"ts":"2026-11-08T11:04:00Z","run":"r","type":"attention","tone":"warning","id":"a","reason":"CI","needsYou":true}
"#;
    let run = reduce(text, now(), &|_| true).unwrap();
    assert_eq!(run.tone, "warning");
    assert!(run.state.needs_you);
    assert_eq!(run.host["poimen.terminal"], "t7");
    assert_eq!(run.host.len(), 2);
    assert_eq!(run.agent.unwrap()["harness"], "claude");
    let kinds: Vec<&str> = run.events.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["run.started", "task.started", "step.started", "attention"]
    );
    assert_eq!(run.events[2].step.as_deref(), Some("implement: green"));
    assert_eq!(run.events[2].task.as_deref(), Some("t"));
}

#[test]
fn the_state_keeps_text_of_attention_finished_task_and_run() {
    let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:00:00Z","run":"r","type":"run.started","tone":"neutral"}
{"v":1,"seq":2,"ts":"2026-11-08T11:00:01Z","run":"r","type":"task.started","tone":"neutral","task":"t","subject":{"kind":"k","id":"1","title":"T"}}
{"v":1,"seq":3,"ts":"2026-11-08T11:00:02Z","run":"r","type":"task.started","tone":"neutral","task":"u","subject":{"kind":"k","id":"2","title":"U"}}
{"v":1,"seq":4,"ts":"2026-11-08T11:00:03Z","run":"r","type":"attention","tone":"warning","id":"a","reason":"ci.red","text":"CI is red","needsYou":true}
{"v":1,"seq":5,"ts":"2026-11-08T11:00:04Z","run":"r","type":"attention","tone":"warning","id":"b","reason":"x","needsYou":true}
{"v":1,"seq":6,"ts":"2026-11-08T11:00:05Z","run":"r","type":"task.finished","tone":"danger","task":"t","outcome":"failed","reason":"ci.red","text":"CI stayed red"}
{"v":1,"seq":7,"ts":"2026-11-08T11:00:06Z","run":"r","type":"task.finished","tone":"neutral","task":"u","outcome":"skipped","reason":"blocked"}
"#;
    let mid = reduce(
        &text[..text.find("{\"v\":1,\"seq\":6").unwrap()],
        now(),
        &|_| true,
    )
    .unwrap();
    assert_eq!(mid.state.attentions[0].text.as_deref(), Some("CI is red"));
    assert_eq!(mid.state.attentions[1].text, None);
    let run = reduce(text, now(), &|_| true).unwrap();
    let f = &run.state.finished;
    assert_eq!(f.failed[0].text.as_deref(), Some("CI stayed red"));
    assert_eq!(f.skipped[0].text, None);
    assert_eq!(f.skipped[0].reason.as_deref(), Some("blocked"));
    let done = format!(
        "{text}{}\n",
        r#"{"v":1,"seq":8,"ts":"2026-11-08T11:00:07Z","run":"r","type":"run.finished","tone":"danger","outcome":"failed","reason":"deliver.partial","text":"2 of 4 delivered"}"#
    );
    let run = reduce(&done, now(), &|_| true).unwrap();
    assert_eq!(run.state.text.as_deref(), Some("2 of 4 delivered"));
    assert_eq!(run.state.reason.as_deref(), Some("deliver.partial"));
}

#[test]
fn the_run_tone_is_the_tone_of_run_finished() {
    let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:00:00Z","run":"r","type":"run.started","tone":"neutral"}
{"v":1,"seq":2,"ts":"2026-11-08T11:01:00Z","run":"r","type":"run.finished","tone":"danger","outcome":"failed","reason":"boom"}
{"v":1,"seq":3,"ts":"2026-11-08T11:02:00Z","run":"r","type":"progress","tone":"success","done":1,"total":2}
"#;
    let run = reduce(text, now(), &|_| true).unwrap();
    assert_eq!(run.tone, "danger");
    assert_eq!(run.state.status, "finished");
    assert_eq!(run.state.reason.as_deref(), Some("boom"));
    assert!(run.state.progress.is_none());
    assert_eq!(run.state.last_seq, 3);
}

#[test]
fn a_dead_writer_pid_abandons_a_fresh_run_without_run_finished() {
    let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:59:00Z","run":"r","type":"run.started","tone":"info","host":{"jsonl-file.pid":"4242"}}
"#;
    let alive = reduce(text, now(), &|p| p == 4242).unwrap();
    assert_eq!(alive.state.status, "running");
    let dead = reduce(text, now(), &|_| false).unwrap();
    assert_eq!(dead.state.status, "abandoned");
    assert_eq!(dead.tone, "neutral");
}

#[test]
fn events_of_another_version_are_skipped_and_reported() {
    let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:00:00Z","run":"r","type":"run.started","tone":"neutral"}
{"v":2,"seq":2,"ts":"2026-11-08T11:01:00Z","run":"r","type":"run.finished","tone":"success","outcome":"succeeded"}
"#;
    let mut reducer = Reducer::new();
    for line in text.lines() {
        reducer.push_line(line);
    }
    assert_eq!(reducer.unknown_v, Some(2));
    let run = reducer.snapshot(now(), &|_| true).unwrap();
    assert_eq!(run.state.status, "running");
    let mut only = Reducer::new();
    only.push_line(r#"{"v":9,"type":"run.started"}"#);
    assert!(only.is_empty());
    assert_eq!(only.unknown_v, Some(9));
}

#[test]
fn unknown_fields_and_lines_that_are_not_json_are_ignored() {
    let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:59:00Z","run":"r","type":"run.started","tone":"neutral","future":{"a":1}}
not json
{"v":1,"seq":2,"ts":"2026-11-08T11:59:10Z","run":"r","type":"something.new","tone":"danger"}
"#;
    let run = reduce(text, now(), &|_| true).unwrap();
    assert_eq!(run.state.status, "running");
    assert_eq!(run.tone, "neutral");
    assert_eq!(run.state.last_seq, 2);
}

#[test]
fn an_empty_or_incomplete_file_has_no_run() {
    assert!(reduce("", now(), &|_| true).is_none());
    assert!(reduce("{\"v\":1,\"ty", now(), &|_| true).is_none());
    let whole_but_unterminated = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:59:00Z","run":"r","type":"run.started","tone":"neutral"}"#;
    assert!(reduce(whole_but_unterminated, now(), &|_| true).is_none());
}

#[test]
fn timestamps_parse_with_fraction_and_offset() {
    assert_eq!(parse_ts("1970-01-01T00:00:00Z"), Some(0.0));
    assert_eq!(parse_ts("1970-01-01T01:00:00+01:00"), Some(0.0));
    assert_eq!(parse_ts("1970-01-01T00:00:01.5Z"), Some(1.5));
    assert_eq!(parse_ts("2026-11-07T12:00:00Z"), Some(1_794_052_800.0));
    for bad in [
        "",
        "2026-11-07",
        "2026-11-07T12:00:00",
        "2026-13-07T12:00:00Z",
    ] {
        assert_eq!(parse_ts(bad), None, "{bad}");
    }
}

/// A file path in a fresh directory; keep the guard, it removes the directory on drop.
fn temp_file(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::Builder::new()
        .prefix("poimen-core-events-")
        .tempdir()
        .unwrap();
    let path = dir.path().join(name);
    (dir, path)
}

fn read(tail: &mut LineTail, path: &Path) -> Tailed {
    let len = std::fs::metadata(path).unwrap().len();
    tail.read(path, len).unwrap()
}

#[test]
fn first_subject_reads_only_the_first_line() {
    let (_dir, path) = temp_file("r1.jsonl");
    std::fs::write(
        &path,
        "{\"v\":1,\"type\":\"run.started\",\"subject\":{\"kind\":\"github.milestone\",\"id\":\"M16\"}}\n{\"type\":\"x\"}\n",
    )
    .unwrap();
    assert_eq!(
        first_subject(&path),
        Some(("github.milestone".to_owned(), "M16".to_owned()))
    );
    // Incomplete first line, other first event, no subject, missing file: none.
    for text in [
        "{\"type\":\"run.started\",\"subject\":{\"kind\":\"a\",\"id\":\"1\"}}",
        "{\"type\":\"progress\",\"subject\":{\"kind\":\"a\",\"id\":\"1\"}}\n",
        "{\"type\":\"run.started\"}\n",
    ] {
        std::fs::write(&path, text).unwrap();
        assert_eq!(first_subject(&path), None, "{text}");
    }
    assert_eq!(first_subject(&path.with_extension("none")), None);
}

#[test]
fn the_line_tail_gives_complete_lines_once_and_waits_for_a_half_written_one() {
    let (_dir, path) = temp_file("r1.jsonl");
    std::fs::write(&path, "{\"seq\":1}\n\n{\"seq\":2}\n{\"se").unwrap();
    let mut tail = LineTail::default();
    assert_eq!(
        read(&mut tail, &path),
        Tailed {
            reset: false,
            lines: vec!["{\"seq\":1}".into(), "{\"seq\":2}".into()]
        }
    );
    // Nothing new: nothing again.
    assert_eq!(read(&mut tail, &path), Tailed::default());
    // The rest of the line arrives.
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("q\":3}\n");
    std::fs::write(&path, &text).unwrap();
    assert_eq!(read(&mut tail, &path).lines, ["{\"seq\":3}"]);
    // Truncated: read again from the start, marked as a reset.
    std::fs::write(&path, "{\"seq\":1}\n").unwrap();
    assert_eq!(
        read(&mut tail, &path),
        Tailed {
            reset: true,
            lines: vec!["{\"seq\":1}".into()]
        }
    );
}

#[test]
fn a_missing_file_is_an_error_of_the_tail() {
    let mut tail = LineTail::default();
    assert!(
        tail.read(Path::new("/nonexistent/poimen/r.jsonl"), 3)
            .is_err()
    );
}

#[test]
fn run_files_are_named_by_a_run_id_of_the_binding() {
    assert_eq!(run_of(Path::new("/x/r1.jsonl")), Some("r1"));
    assert_eq!(
        run_of(Path::new("2026-11-08.run_1.jsonl")),
        Some("2026-11-08.run_1")
    );
    for bad in [".x.jsonl", "a b.jsonl", "r1.txt", "r1", "-x.jsonl"] {
        assert!(!is_run_file(Path::new(bad)), "{bad}");
    }
    assert!(!is_run_file(Path::new(&format!(
        "{}.jsonl",
        "a".repeat(129)
    ))));
    assert!(is_run_file(Path::new(&format!(
        "{}.jsonl",
        "a".repeat(128)
    ))));
}

#[test]
fn the_records_of_a_lead_run_fold_into_its_lead_state() {
    let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:00:00Z","run":"lead-1","type":"run.started","tone":"neutral","host":{"poimen.terminal":"t1"}}
{"v":1,"seq":2,"ts":"2026-11-08T11:01:00Z","run":"lead-1","profile":"orchestrate@1","type":"orchestrate.member","task":"a#1","repository":"/r/a","terminal":"t2","deliver_run":"d-1"}
{"v":1,"seq":3,"ts":"2026-11-08T11:02:00Z","run":"lead-1","profile":"orchestrate@1","type":"orchestrate.member","task":"a#1","repository":"/r/a","terminal":"t3","deliver_run":"d-1"}
{"v":1,"seq":4,"ts":"2026-11-08T11:03:00Z","run":"lead-1","profile":"orchestrate@1","type":"orchestrate.dependency","task":"b#2","blocked_by":"a#1"}
{"v":1,"seq":5,"ts":"2026-11-08T11:03:00Z","run":"lead-1","profile":"orchestrate@1","type":"orchestrate.dependency","task":"b#2","blocked_by":"a#1"}
{"v":1,"seq":6,"ts":"2026-11-08T11:04:00Z","run":"lead-1","profile":"orchestrate@1","type":"orchestrate.grant","grant":"g1","spaces":["s1"],"tools":["start_agent"],"max_agents":3}
{"v":1,"seq":7,"ts":"2026-11-08T11:05:00Z","run":"lead-1","profile":"orchestrate@1","type":"orchestrate.resume","terminal":"t9"}
{"v":1,"seq":8,"ts":"2026-11-08T11:06:00Z","run":"lead-1","profile":"orchestrate@1","type":"orchestrate.future"}
"#;
    let run = reduce(text, now(), &|_| true).unwrap();
    let lead = run.lead.clone().expect("a lead state");
    assert_eq!(lead.members.len(), 1, "a member is replaced by its task");
    assert_eq!(lead.members[0].terminal.as_deref(), Some("t3"));
    assert_eq!(lead.dependencies.len(), 1);
    assert_eq!(lead.grant.as_ref().map(|g| g.id.as_str()), Some("g1"));
    assert_eq!(lead.resumed_in.as_deref(), Some("t9"));
    // The records are no rows of the Events tab, the core events are.
    assert_eq!(run.events.len(), 1);
    let json = serde_json::to_value(&run).unwrap();
    assert_eq!(json["lead"]["members"][0]["deliverRun"], "d-1");
    assert_eq!(json["lead"]["grant"]["maxAgents"], 3);
}

#[test]
fn a_run_without_lead_records_has_no_lead_state() {
    let text = r#"{"v":1,"seq":1,"ts":"2026-11-08T11:00:00Z","run":"r","type":"run.started","tone":"neutral"}
{"v":1,"seq":2,"ts":"2026-11-08T11:01:00Z","run":"r","profile":"deliver@1","type":"deliver.new"}
"#;
    let run = reduce(text, now(), &|_| true).unwrap();
    assert!(run.lead.is_none());
    assert!(serde_json::to_value(&run).unwrap().get("lead").is_none());
}
