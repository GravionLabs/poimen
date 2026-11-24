use super::*;
use crate::test_dir::TestDir;
use std::io::Write;

fn temp_dir(name: &str) -> TestDir {
    TestDir::new(&format!("poimen-transcripts-{name}-"))
}

fn append(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

const PROMPT: &str = r#"{"type":"user","uuid":"u1","timestamp":"2026-11-02T04:07:52.158Z","isSidechain":true,"message":{"role":"user","content":"Review the diff."}}"#;
const CALL: &str = r#"{"type":"assistant","uuid":"a1","timestamp":"2026-11-02T04:07:54.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hm","signature":"x"},{"type":"tool_use","id":"toolu_1","name":"Bash","input":{"command":"git diff"}}]}}"#;
const RESULT: &str = r#"{"type":"user","uuid":"u2","timestamp":"2026-11-02T04:07:56.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_1","content":"1 file changed","is_error":false}]},"toolUseResult":{"stdout":"1 file changed"}}"#;
const REPORT: &str = r#"{"type":"assistant","uuid":"a2","timestamp":"2026-11-02T04:08:10.000Z","message":{"role":"assistant","content":[{"type":"text","text":"All good."}]}}"#;

#[test]
fn assistant_text_joins_text_blocks_and_skips_other_lines() {
    assert_eq!(assistant_text(REPORT).as_deref(), Some("All good."));
    assert_eq!(assistant_text(CALL), None);
    assert_eq!(assistant_text(PROMPT), None);
    assert_eq!(assistant_text("not json with \"assistant\" \"text\""), None);
    let two = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"A"},{"type":"tool_use","id":"t","name":"x","input":{}},{"type":"text","text":"B"}]}}"#;
    assert_eq!(assistant_text(two).as_deref(), Some("A\nB"));
}

#[test]
fn open_follows_a_claude_session_found_by_its_directory() {
    let home = temp_dir("open-claude");
    let file = projects_root(&home)
        .join(project_dir_name("/w/repo"))
        .join("s-1.jsonl");
    append(&file, &format!("{CALL}\n"));
    let Ok(mut follow) = open(&home, "claude", "s-1", "/w/repo") else {
        panic!("expected a follow");
    };
    assert!(
        follow
            .refresh()
            .iter()
            .any(|u| matches!(u, Update::Main { tools: 1, .. }))
    );
    assert!(follow.refresh().is_empty());
}

#[test]
fn open_follows_a_copilot_session_by_its_id() {
    let home = temp_dir("open-copilot");
    let file = copilot::sessions_root(&home)
        .join("0b7e-c0")
        .join(copilot::EVENTS);
    append(&file, include_str!("../../fixtures/copilot-events.jsonl"));
    let Ok(mut follow) = open(&home, "copilot", "0b7e-c0", "") else {
        panic!("expected a follow");
    };
    assert!(
        follow
            .refresh()
            .iter()
            .any(|u| matches!(u, Update::List { .. }))
    );
}

#[test]
fn open_tells_no_agent_from_missing_and_unreadable() {
    let home = temp_dir("open-errors");
    let no_agent = |agent, session, cwd| open(&home, agent, session, cwd).err();
    assert_eq!(no_agent("codex", "s-1", "/w"), Some(OpenError::NoAgent));
    assert_eq!(no_agent("claude", "../x", "/w"), Some(OpenError::NoAgent));
    assert_eq!(no_agent("claude", "s-1", ""), Some(OpenError::NoAgent));
    assert_eq!(no_agent("claude", "s-1", "/w"), Some(OpenError::Missing));
    assert_eq!(no_agent("copilot", "s-1", ""), Some(OpenError::Missing));
    // A directory where the file should be: located by `is_file` it is not, so it is missing.
    let dir = projects_root(&home)
        .join(project_dir_name("/w"))
        .join("s-2.jsonl");
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(no_agent("claude", "s-2", "/w"), Some(OpenError::Missing));
}

#[test]
fn project_dir_name_replaces_every_non_alphanumeric_character() {
    assert_eq!(
        project_dir_name("/Users/jk/Source/poimen"),
        "-Users-jk-Source-poimen"
    );
    assert_eq!(project_dir_name("/a/.wt_x"), "-a--wt-x");
    assert_eq!(project_dir_name("/tmp/ä"), "-tmp--");
}

#[test]
fn project_dir_is_found_by_cwd_then_by_scanning_for_the_session() {
    let root = temp_dir("locate");
    append(&root.join("-work-proj").join("s1.jsonl"), "{}\n");
    append(&root.join("-other").join("s2.jsonl"), "{}\n");
    assert_eq!(
        locate_project_dir(&root, "/work/proj", "s1"),
        Some(root.join("-work-proj"))
    );
    assert_eq!(
        locate_project_dir(&root, "/moved/away", "s2"),
        Some(root.join("-other"))
    );
    assert_eq!(locate_project_dir(&root, "/work/proj", "nope"), None);
    assert_eq!(locate_project_dir(&root.join("missing"), "/x", "s1"), None);
}

#[test]
fn messages_are_parsed_into_blocks() {
    let prompt = parse_line(PROMPT).unwrap().entry.unwrap();
    assert_eq!(prompt.role, "user");
    assert_eq!(
        prompt.blocks,
        vec![Block::Text {
            text: "Review the diff.".into()
        }]
    );
    let call = parse_line(CALL).unwrap();
    assert_eq!(call.timestamp.as_deref(), Some("2026-11-02T04:07:54.000Z"));
    assert_eq!(
        call.entry.unwrap().blocks,
        vec![
            Block::Thinking { text: "hm".into() },
            Block::ToolUse {
                id: "toolu_1".into(),
                name: "Bash".into(),
                input: r#"{"command":"git diff"}"#.into()
            }
        ]
    );
    let result = parse_line(RESULT).unwrap().entry.unwrap();
    assert_eq!(
        result.blocks,
        vec![Block::ToolResult {
            tool_use_id: "toolu_1".into(),
            text: "1 file changed".into(),
            is_error: false
        }]
    );
}

#[test]
fn tool_results_with_text_parts_are_joined() {
    let line = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"t","content":[{"type":"text","text":"a"},{"type":"image"},{"type":"text","text":"b"}],"is_error":true}]}}"#;
    assert_eq!(
        parse_line(line).unwrap().entry.unwrap().blocks,
        vec![Block::ToolResult {
            tool_use_id: "t".into(),
            text: "a\nb".into(),
            is_error: true
        }]
    );
}

#[test]
fn unknown_entry_types_blocks_and_fields_are_tolerated() {
    let attachment =
        r#"{"type":"attachment","timestamp":"t1","attachment":{"type":"env"},"newField":1}"#;
    let line = parse_line(attachment).unwrap();
    assert_eq!(line.timestamp.as_deref(), Some("t1"));
    assert_eq!(line.entry, None);
    let meta = r#"{"type":"user","isMeta":true,"message":{"content":"<system-reminder>"}}"#;
    assert_eq!(parse_line(meta).unwrap().entry, None);
    let odd = r#"{"type":"assistant","future":{"x":[]},"message":{"content":[{"type":"hologram","data":1},{"type":"text","text":"ok","extra":true}]}}"#;
    assert_eq!(
        parse_line(odd).unwrap().entry.unwrap().blocks,
        vec![Block::Text { text: "ok".into() }]
    );
    let no_message = r#"{"type":"assistant"}"#;
    assert_eq!(parse_line(no_message).unwrap().entry, None);
}

#[test]
fn broken_lines_are_not_lines() {
    assert_eq!(parse_line("not json"), None);
    assert_eq!(parse_line("[1,2]"), None);
    assert_eq!(parse_line(""), None);
}

#[test]
fn long_text_is_truncated_on_a_character_boundary() {
    let long = "ä".repeat(TEXT_LIMIT + 10);
    let line = format!(
        r#"{{"type":"assistant","message":{{"content":[{{"type":"text","text":"{long}"}}]}}}}"#
    );
    let Block::Text { text } = &parse_line(&line).unwrap().entry.unwrap().blocks[0] else {
        panic!("text block");
    };
    assert_eq!(text.chars().count(), TEXT_LIMIT + 1);
    assert!(text.ends_with('…'));
}

#[test]
fn meta_is_read_with_defaults_for_missing_fields() {
    let meta = parse_meta(
        r#"{"agentType":"general-purpose","description":"Review PBI #21","toolUseId":"toolu_9","spawnDepth":2,"worktreeBranch":"fix-1","finished":true,"model":"x"}"#,
    );
    assert_eq!(
        meta,
        Meta {
            agent_type: "general-purpose".into(),
            description: "Review PBI #21".into(),
            tool_use_id: "toolu_9".into(),
            spawn_depth: 2,
            worktree_branch: Some("fix-1".into()),
            finished: true
        }
    );
    assert_eq!(parse_meta("{}"), Meta::default());
    assert_eq!(parse_meta("garbage"), Meta::default());
}

#[test]
fn a_final_tool_result_completes_the_tool_use() {
    let done = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_9","content":"report"}]},"toolUseResult":{"status":"completed"}}"#;
    assert_eq!(final_results(done), vec![("toolu_9".to_owned(), false)]);
}

#[test]
fn a_background_launch_acknowledgement_does_not_complete_the_tool_use() {
    let launched = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_9","content":"Async agent launched"}]},"toolUseResult":{"isAsync":true,"status":"async_launched","agentId":"a1"}}"#;
    assert!(final_results(launched).is_empty());
    assert!(final_results("nope").is_empty());
    assert!(final_results(CALL).is_empty());
}

#[test]
fn tailer_returns_only_new_complete_lines() {
    let dir = temp_dir("tail");
    let path = dir.join("a.jsonl");
    let mut tailer = Tailer::new(path.clone());
    assert!(tailer.read_new().is_empty());
    append(&path, "one\ntwo\nthr");
    assert_eq!(tailer.read_new(), vec!["one", "two"]);
    assert!(tailer.read_new().is_empty());
    append(&path, "ee\n\nfour\n");
    assert_eq!(tailer.read_new(), vec!["three", "four"]);
}

#[test]
fn tailer_reads_a_large_file_in_linear_time() {
    let dir = temp_dir("large");
    let path = dir.join("a.jsonl");
    let mut text = String::new();
    for i in 0..50_000 {
        text.push_str(&format!("{{\"n\":{i}}}\n"));
    }
    text.push_str("partial");
    append(&path, &text);
    let started = std::time::Instant::now();
    let lines = Tailer::new(path.clone()).read_new();
    assert_eq!(lines.len(), 50_000);
    assert_eq!(lines[49_999], "{\"n\":49999}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn unsafe_session_ids_find_no_transcripts() {
    let root = temp_dir("unsafe");
    std::fs::write(root.join("x.jsonl"), "").unwrap();
    let project = root.join("p");
    std::fs::create_dir_all(&project).unwrap();
    for bad in ["../x", "/abs", "", "a/b", "..", "a.b"] {
        assert_eq!(locate_project_dir(&root, "/p", bad), None, "{bad}");
        assert!(Tracker::new(&project, bad).is_none(), "{bad}");
        assert!(watch(&project, bad, |_| {}).is_err(), "{bad}");
    }
    assert!(Tracker::new(&project, "0b7e-A_1").is_some());
}

#[test]
fn tailer_starts_over_when_the_file_shrinks() {
    let dir = temp_dir("shrink");
    let path = dir.join("a.jsonl");
    let mut tailer = Tailer::new(path.clone());
    append(&path, "long first line\n");
    assert_eq!(tailer.read_new().len(), 1);
    std::fs::write(&path, "new\n").unwrap();
    assert_eq!(tailer.read_new(), vec!["new"]);
}

fn session_fixture(name: &str) -> (TestDir, PathBuf) {
    let project = temp_dir(name);
    let sub = project.join("S").join("subagents");
    append(
        &sub.join("agent-abc.meta.json"),
        r#"{"agentType":"general-purpose","description":"Review","toolUseId":"toolu_9","spawnDepth":1}"#,
    );
    append(&sub.join("agent-abc.jsonl"), &format!("{PROMPT}\n{CALL}\n"));
    (project, sub)
}

#[test]
fn tracker_lists_a_running_subagent_with_its_entries() {
    let (project, _) = session_fixture("running");
    let mut tracker = Tracker::new(&project, "S").unwrap();
    let updates = tracker.refresh();
    let Update::List { agents } = &updates[0] else {
        panic!("list first");
    };
    assert_eq!(agents.len(), 1);
    assert_eq!(agents[0].agent_id, "abc");
    assert_eq!(agents[0].status, "running");
    assert_eq!(agents[0].description, "Review");
    assert_eq!(
        agents[0].started.as_deref(),
        Some("2026-11-02T04:07:52.158Z")
    );
    assert_eq!(agents[0].last.as_deref(), Some("2026-11-02T04:07:54.000Z"));
    let Update::Entries { agent_id, entries } = &updates[1] else {
        panic!("entries");
    };
    assert_eq!((agent_id.as_str(), entries.len()), ("abc", 2));
    assert!(tracker.refresh().is_empty());
}

#[test]
fn tracker_streams_later_lines_and_marks_done_by_the_parent_result() {
    let (project, sub) = session_fixture("done");
    let mut tracker = Tracker::new(&project, "S").unwrap();
    tracker.refresh();
    append(
        &sub.join("agent-abc.jsonl"),
        &format!("{RESULT}\n{REPORT}\n"),
    );
    let updates = tracker.refresh();
    assert!(matches!(&updates[1], Update::Entries { entries, .. } if entries.len() == 2));
    assert!(matches!(&updates[0], Update::List { agents } if agents[0].status == "running"));

    let launched = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_9","content":"launched"}]},"toolUseResult":{"isAsync":true}}"#;
    append(&project.join("S.jsonl"), &format!("{launched}\n"));
    assert!(tracker.refresh().is_empty(), "a launch is not a result");

    let final_result = r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"toolu_9","content":"report"}]}}"#;
    append(&project.join("S.jsonl"), &format!("{final_result}\n"));
    let updates = tracker.refresh();
    assert!(matches!(&updates[..], [Update::List { agents }] if agents[0].status == "done"));
}

#[test]
fn tracker_marks_done_by_the_finished_flag_of_the_meta() {
    let (project, sub) = session_fixture("finished");
    let mut tracker = Tracker::new(&project, "S").unwrap();
    tracker.refresh();
    std::fs::write(
        sub.join("agent-abc.meta.json"),
        r#"{"agentType":"general-purpose","description":"Review","toolUseId":"toolu_9","finished":true}"#,
    )
    .unwrap();
    let updates = tracker.refresh();
    assert!(matches!(&updates[..], [Update::List { agents }] if agents[0].status == "done"));
}

const MAIN_CALLS: &str = r#"{"type":"assistant","uuid":"m1","timestamp":"2026-11-02T04:00:00.000Z","message":{"role":"assistant","content":[{"type":"tool_use","id":"toolu_s","name":"Skill","input":{"skill":"implementation","args":"104"}},{"type":"tool_use","id":"toolu_b","name":"Bash","input":{"command":"ls"}},{"type":"tool_use","id":"toolu_9","name":"Agent","input":{"description":"Review","subagent_type":"general-purpose","prompt":"long"}}]}}"#;
const SKILL_RESULT: &str = r#"{"type":"user","uuid":"m2","timestamp":"2026-11-02T04:00:05.000Z","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_s","content":"Launching skill","is_error":true}]}}"#;

#[test]
fn main_calls_split_skill_and_agent_from_plain_tools() {
    let (calls, plain) = parse_main_calls(MAIN_CALLS);
    assert_eq!(plain, vec!["toolu_b".to_owned()]);
    assert_eq!(calls.len(), 2);
    assert_eq!(
        (
            calls[0].name.as_str(),
            calls[0].target.as_str(),
            calls[0].detail.as_str()
        ),
        ("Skill", "implementation", "104")
    );
    assert_eq!(
        (
            calls[1].name.as_str(),
            calls[1].target.as_str(),
            calls[1].detail.as_str()
        ),
        ("Agent", "Review", "general-purpose")
    );
    assert_eq!(
        calls[0].started.as_deref(),
        Some("2026-11-02T04:00:00.000Z")
    );
    assert_eq!(parse_main_calls("not json"), (Vec::new(), Vec::new()));
    assert_eq!(parse_main_calls(PROMPT), (Vec::new(), Vec::new()));
}

#[test]
fn tracker_reports_main_calls_with_results_and_the_plain_tool_count() {
    let (project, _) = session_fixture("main");
    let mut tracker = Tracker::new(&project, "S").unwrap();
    tracker.refresh();
    append(&project.join("S.jsonl"), &format!("{MAIN_CALLS}\n"));
    let updates = tracker.refresh();
    let [Update::Main { calls, tools, .. }] = &updates[..] else {
        panic!("one main update: {updates:?}");
    };
    assert_eq!((calls.len(), *tools), (2, 1));
    assert_eq!(calls[0].ended, None);

    append(&project.join("S.jsonl"), &format!("{SKILL_RESULT}\n"));
    let updates = tracker.refresh();
    let Update::Main { calls, .. } = updates.last().unwrap() else {
        panic!("main last");
    };
    assert_eq!(calls[0].ended.as_deref(), Some("2026-11-02T04:00:05.000Z"));
    assert!(calls[0].is_error);
    assert!(tracker.refresh().is_empty());
}

#[test]
fn tracker_without_transcripts_is_empty() {
    let project = temp_dir("empty");
    let mut tracker = Tracker::new(&project, "S").unwrap();
    assert!(tracker.refresh().is_empty());
}

#[test]
fn tracker_ignores_other_files_of_the_directory() {
    let (project, sub) = session_fixture("other");
    append(&sub.join("agent-x.forked-skill.json"), "{}");
    append(&sub.join("notes.txt"), "hi");
    let mut tracker = Tracker::new(&project, "S").unwrap();
    let Update::List { agents } = &tracker.refresh()[0] else {
        panic!("list");
    };
    assert_eq!(agents.len(), 1);
}

#[test]
fn watch_reports_the_files_that_appear_afterwards() {
    let project = temp_dir("watch");
    let (tx, rx) = mpsc::channel();
    let handle = watch(&project, "S", move |u| {
        let _ = tx.send(u);
    })
    .unwrap();
    let sub = project.join("S").join("subagents");
    append(&sub.join("agent-abc.jsonl"), &format!("{PROMPT}\n"));
    let update = rx.recv_timeout(Duration::from_secs(10)).expect("an update");
    assert!(matches!(update, Update::List { .. }));
    drop(handle);
}

fn assistant(id: &str, ts: &str, usage: &str) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"u-{id}-{ts}","timestamp":"2026-11-02T04:00:{ts}.000Z","message":{{"id":"{id}","model":"claude-opus-4-7","content":[{{"type":"text","text":"x"}}]{usage}}}}}"#
    )
}

#[test]
fn usage_reads_the_four_counts_and_the_model() {
    let line = assistant(
        "msg_1",
        "01",
        r#","usage":{"input_tokens":3,"output_tokens":120,"cache_read_input_tokens":40000,"cache_creation_input_tokens":500,"service_tier":"standard"}"#,
    );
    let entry = parse_line(&line).unwrap().entry.unwrap();
    let usage = entry.usage.unwrap();
    assert_eq!(
        (
            usage.input,
            usage.output,
            usage.cache_read,
            usage.cache_creation
        ),
        (3, 120, 40000, 500)
    );
    assert_eq!(usage.context(), 40503);
    assert_eq!(entry.model.as_deref(), Some("claude-opus-4-7"));
    assert_eq!(entry.message_id.as_deref(), Some("msg_1"));
}

#[test]
fn missing_usage_is_unknown_not_an_error() {
    let entry = parse_line(&assistant("msg_1", "01", ""))
        .unwrap()
        .entry
        .unwrap();
    assert_eq!(entry.usage, None);
    assert!(parse_usage_sample(&assistant("msg_1", "01", "")).is_none());
}

#[test]
fn malformed_usage_is_unknown_not_an_error() {
    for usage in [
        r#","usage":"many""#,
        r#","usage":null"#,
        r#","usage":{}"#,
        r#","usage":{"input_tokens":"3"}"#,
        r#","usage":{"input_tokens":-1,"output_tokens":2}"#,
        r#","usage":{"input_tokens":1.5}"#,
    ] {
        let line = assistant("msg_1", "01", usage);
        let parsed = parse_line(&line).expect("the line itself still parses");
        assert_eq!(parsed.entry.expect("entry").usage, None, "{usage}");
        assert!(parse_usage_sample(&line).is_none(), "{usage}");
    }
}

#[test]
fn partial_usage_counts_absent_fields_as_zero() {
    let line = assistant("m", "01", r#","usage":{"output_tokens":7}"#);
    let sample = parse_usage_sample(&line).unwrap();
    assert_eq!((sample.usage.input, sample.usage.output), (0, 7));
}

#[test]
fn main_call_carries_the_context_fill_of_its_message() {
    let line = MAIN_CALLS.replace(
        r#""content":["#,
        r#""usage":{"input_tokens":2,"output_tokens":9,"cache_read_input_tokens":1000,"cache_creation_input_tokens":30},"content":["#,
    );
    let (calls, _) = parse_main_calls(&line);
    assert_eq!(calls[0].context, Some(1032));
    assert_eq!(parse_main_calls(MAIN_CALLS).0[0].context, None);
}

#[test]
fn tracker_samples_usage_once_per_message_and_keeps_the_latest_line() {
    let (project, _) = session_fixture("usage");
    let mut tracker = Tracker::new(&project, "S").unwrap();
    tracker.refresh();
    let u = |out: u32| format!(r#","usage":{{"input_tokens":1,"output_tokens":{out}}}"#);
    append(
        &project.join("S.jsonl"),
        &format!(
            "{}\n{}\n{}\n",
            assistant("msg_1", "01", &u(5)),
            assistant("msg_1", "02", &u(50)),
            assistant("msg_2", "03", "")
        ),
    );
    let updates = tracker.refresh();
    let Some(Update::Main { usage, .. }) = updates.last() else {
        panic!("no main update: {updates:?}");
    };
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].usage.output, 50);
    assert_eq!(usage[0].model.as_deref(), Some("claude-opus-4-7"));
}

fn msg(id: &str, input: u64, output: u64) -> String {
    format!(
        r#"{{"type":"assistant","uuid":"u-{id}","message":{{"id":"{id}","usage":{{"input_tokens":{input},"output_tokens":{output},"cache_read_input_tokens":10,"cache_creation_input_tokens":5}}}}}}"#
    )
}

#[test]
fn claude_totals_sum_main_and_subagent_files_with_duplicates_once() {
    let dir = temp_dir("totals");
    let main = dir.join("s1.jsonl");
    // m1 is written in two lines (the later replaces), a malformed line is skipped.
    append(
        &main,
        &format!(
            "{}\n{}\nnot json\n{}\n",
            msg("m1", 1, 1),
            msg("m1", 1, 7),
            msg("m2", 100, 20)
        ),
    );
    append(
        &dir.join("s1").join("subagents").join("agent-a.jsonl"),
        &format!("{}\n{}\n", msg("m3", 1000, 200), msg("m2", 100, 20)),
    );
    let mut reader = UsageReader::claude(&dir, "s1").unwrap();
    assert_eq!(
        reader.refresh(),
        Some(Totals {
            input: 1101,
            output: 227,
            cache_read: 30,
            cache_creation: 15
        })
    );
    assert_eq!(reader.refresh(), None);
    append(&main, &format!("{}\n", msg("m4", 2, 3)));
    assert_eq!(reader.refresh().unwrap().input, 1103);
}

#[test]
fn usage_samples_carry_the_timestamp_of_each_message_once() {
    let dir = temp_dir("samples");
    let main = dir.join("s1.jsonl");
    let stamped = |id: &str, input: u64, at: &str| {
        msg(id, input, 1).replacen(
            "{\"type\"",
            &format!("{{\"timestamp\":\"{at}\",\"type\""),
            1,
        )
    };
    // m1 twice (the later line wins), m2 once, m3 has no timestamp.
    append(
        &main,
        &format!(
            "{}\n{}\n{}\n{}\n",
            stamped("m1", 1, "2026-11-02T04:00:01.000Z"),
            stamped("m1", 2, "2026-11-02T04:00:01.000Z"),
            stamped("m2", 100, "2026-11-02T04:00:09.000Z"),
            msg("m3", 5000, 1)
        ),
    );
    let mut reader = UsageReader::claude(&dir, "s1").unwrap();
    reader.refresh().unwrap();
    let mut samples = reader.samples();
    samples.sort_by(|a, b| a.at.cmp(&b.at));
    assert_eq!(
        samples
            .iter()
            .map(|s| (s.at.as_str(), s.input))
            .collect::<Vec<_>>(),
        [
            ("2026-11-02T04:00:01.000Z", 2),
            ("2026-11-02T04:00:09.000Z", 100)
        ]
    );
}

#[test]
fn a_session_without_usage_has_no_totals() {
    let dir = temp_dir("nousage");
    append(&dir.join("s1.jsonl"), "{\"type\":\"user\"}\n");
    assert_eq!(UsageReader::claude(&dir, "s1").unwrap().refresh(), None);
    assert!(UsageReader::claude(&dir, "../x").is_none());
}

#[test]
fn copilot_usage_records_total_once_per_event_id() {
    let dir = temp_dir("cop");
    let rec = |id: &str, i: u64| {
        format!(
            r#"{{"type":"session.usage_record","data":{{"usage":{{"inputTokens":{i},"outputTokens":2,"cacheReadTokens":3,"cacheWriteTokens":4}}}},"id":"{id}"}}"#
        )
    };
    append(
        &dir.join(copilot::EVENTS),
        &format!(
            "{}\n{}\n{}\n{{oops\n",
            rec("a", 10),
            rec("a", 10),
            rec("b", 5)
        ),
    );
    let totals = UsageReader::copilot(&dir).refresh().unwrap();
    assert_eq!((totals.input, totals.output), (15, 4));
    assert_eq!((totals.cache_read, totals.cache_creation), (6, 8));
}

#[test]
fn updates_of_a_remote_machine_fold_into_the_same_totals() {
    let usage = |i| Usage {
        input: i,
        output: 1,
        cache_read: 0,
        cache_creation: 0,
    };
    let sample = |id: &str, i| UsageSample {
        id: id.into(),
        timestamp: None,
        model: None,
        usage: usage(i),
    };
    let mut sum = UsageSum::default();
    let main = Update::Main {
        calls: vec![],
        tools: 0,
        usage: vec![sample("m1", 4)],
        last_message: None,
    };
    assert!(sum.apply(&main));
    assert!(!sum.apply(&main));
    let entry = Entry {
        uuid: "x".into(),
        role: "assistant".into(),
        timestamp: None,
        blocks: vec![],
        message_id: Some("m9".into()),
        model: None,
        usage: Some(usage(6)),
    };
    assert!(sum.apply(&Update::Entries {
        agent_id: "a".into(),
        entries: vec![entry]
    }));
    assert_eq!(sum.totals().input, 10);
    let mut json = UsageSum::default();
    let u = serde_json::json!({"input": 4, "output": 1, "cache_read": 0, "cache_creation": 0});
    let main = serde_json::json!({"kind": "main", "calls": [], "tools": 0, "usage": [{"id": "m1", "usage": u}]});
    assert!(json.apply_json(&main));
    assert!(!json.apply_json(&main));
    assert!(!json.apply_json(&serde_json::json!({"kind": "list", "agents": []})));
    let entries = serde_json::json!({"kind": "entries", "agent_id": "a", "entries": [{"message_id": "m9", "usage": {"input": 6, "output": 1, "cache_read": 0, "cache_creation": 0}}, {"message_id": "m8", "usage": null}]});
    assert!(json.apply_json(&entries));
    assert_eq!(json.totals().input, 10);
}
