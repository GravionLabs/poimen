use super::*;
use crate::test_dir::TestDir;
use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

const FIXTURE: &str = include_str!("../../fixtures/copilot-events.jsonl");

fn temp_dir(name: &str) -> TestDir {
    TestDir::new(&format!("poimen-copilot-{name}-"))
}

fn append(path: &Path, text: &str) {
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

fn session(name: &str, text: &str) -> TestDir {
    let dir = temp_dir(name);
    append(&dir.join(EVENTS), text);
    dir
}

fn list(updates: &[Update]) -> &[SubagentInfo] {
    updates
        .iter()
        .find_map(|u| match u {
            Update::List { agents } => Some(agents.as_slice()),
            _ => None,
        })
        .expect("a list update")
}

fn entries<'a>(updates: &'a [Update], id: &str) -> &'a [Entry] {
    updates
        .iter()
        .find_map(|u| match u {
            Update::Entries { agent_id, entries } if agent_id == id => Some(entries.as_slice()),
            _ => None,
        })
        .unwrap_or_default()
}

fn main(updates: &[Update]) -> (&[MainCall], u32) {
    updates
        .iter()
        .find_map(|u| match u {
            Update::Main { calls, tools, .. } => Some((calls.as_slice(), *tools)),
            _ => None,
        })
        .expect("a main update")
}

#[test]
fn session_dir_is_found_by_the_session_id() {
    let root = temp_dir("locate");
    std::fs::create_dir_all(root.join("c0")).unwrap();
    append(&root.join("c0").join(EVENTS), "{}\n");
    std::fs::create_dir_all(root.join("c1")).unwrap();
    assert_eq!(locate_session_dir(&root, "c0"), Some(root.join("c0")));
    assert_eq!(locate_session_dir(&root, "c1"), None, "no events.jsonl");
    assert_eq!(locate_session_dir(&root, "nope"), None);
    for bad in ["../c0", "", "a/b", "..", "c0.x"] {
        assert_eq!(locate_session_dir(&root, bad), None, "{bad}");
    }
    assert_eq!(
        sessions_root(Path::new("/h")),
        Path::new("/h").join(".copilot").join("session-state")
    );
}

#[test]
fn main_calls_are_skills_and_tasks_with_the_other_tools_counted() {
    let updates = Tracker::new(&session("main", FIXTURE)).refresh();
    let (calls, tools) = main(&updates);
    assert_eq!(tools, 1, "view");
    let summary: Vec<_> = calls
        .iter()
        .map(|c| {
            (
                c.tool_use_id.as_str(),
                c.name.as_str(),
                c.target.as_str(),
                c.detail.as_str(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("call_s", "Skill", "implementation", ""),
            ("call_t1", "Agent", "Research the API", "research"),
            (
                "call_t2",
                "Agent",
                "Implement the change",
                "general-purpose"
            ),
        ]
    );
    assert_eq!(
        calls[0].started.as_deref(),
        Some("2026-11-03T10:00:02.100Z")
    );
    assert_eq!(calls[0].ended.as_deref(), Some("2026-11-03T10:00:03.000Z"));
    // A sync task ends with its subagent; a background one is still running after the ack.
    assert_eq!(calls[1].ended.as_deref(), Some("2026-11-03T10:00:14.500Z"));
    assert_eq!(calls[2].ended, None);
    assert!(calls.iter().all(|c| !c.is_error));
}

fn usage(updates: &[Update]) -> Vec<UsageSample> {
    updates
        .iter()
        .find_map(|u| match u {
            Update::Main { usage, .. } => Some(usage.clone()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn usage_records_become_main_usage_samples() {
    let line = r#"{"type":"session.usage_record","data":{"usage":{"model":"gpt-6-luna","inputTokens":100,"outputTokens":7,"cacheReadTokens":50,"cacheWriteTokens":3}},"id":"u1","timestamp":"2026-11-03T10:00:05.000Z","parentId":null}"#;
    let updates = Tracker::new(&session("usage", &format!("{line}\n"))).refresh();
    let samples = usage(&updates);
    assert_eq!(samples.len(), 1);
    let s = &samples[0];
    assert_eq!(
        (s.id.as_str(), s.model.as_deref()),
        ("u1", Some("gpt-6-luna"))
    );
    assert_eq!(
        (
            s.usage.input,
            s.usage.output,
            s.usage.cache_read,
            s.usage.cache_creation
        ),
        (100, 7, 50, 3)
    );
}

#[test]
fn missing_or_malformed_usage_stays_unknown() {
    assert!(usage(&Tracker::new(&session("nousage", FIXTURE)).refresh()).is_empty());
    let bad = [
        r#"{"type":"session.usage_record","data":{"usage":{}},"id":"a"}"#,
        r#"{"type":"session.usage_record","data":{"usage":{"inputTokens":"9"}},"id":"b"}"#,
        r#"{"type":"session.usage_record","data":{},"id":"c"}"#,
    ];
    let updates = Tracker::new(&session("badusage", &bad.join("\n"))).refresh();
    assert!(usage(&updates).is_empty());
}

#[test]
fn subagents_are_listed_with_their_spawning_call_and_depth() {
    let updates = Tracker::new(&session("list", FIXTURE)).refresh();
    assert!(matches!(updates[0], Update::List { .. }), "list first");
    let agents = list(&updates);
    let summary: Vec<_> = agents
        .iter()
        .map(|a| {
            (
                a.agent_id.as_str(),
                a.agent_type.as_str(),
                a.description.as_str(),
                a.tool_use_id.as_str(),
                a.spawn_depth,
                a.status,
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            ("a1", "research", "Research the API", "call_t1", 1, "done"),
            (
                "a2",
                "general-purpose",
                "Implement the change",
                "call_t2",
                1,
                "running"
            ),
            (
                "a3",
                "test-writer",
                "Write the tests",
                "call_t3",
                2,
                "running"
            ),
        ]
    );
    assert_eq!(
        agents[0].started.as_deref(),
        Some("2026-11-03T10:00:10.500Z")
    );
    assert_eq!(agents[0].last.as_deref(), Some("2026-11-03T10:00:14.500Z"));
    assert_eq!(
        agents[2].last.as_deref(),
        Some("2026-11-03T10:01:03.000Z"),
        "unknown events still count as activity"
    );
}

#[test]
fn subagent_events_become_messages() {
    let updates = Tracker::new(&session("entries", FIXTURE)).refresh();
    let a1 = entries(&updates, "a1");
    let blocks: Vec<_> = a1.iter().map(|e| (e.role.as_str(), &e.blocks)).collect();
    assert_eq!(
        blocks,
        vec![
            (
                "user",
                &vec![Block::Text {
                    text: "Find the endpoints.".into()
                }]
            ),
            (
                "assistant",
                &vec![
                    Block::Thinking {
                        text: "Search the routes.".into()
                    },
                    Block::ToolUse {
                        id: "call_g".into(),
                        name: "grep".into(),
                        input: r#"{"pattern":"route"}"#.into()
                    }
                ]
            ),
            (
                "user",
                &vec![Block::ToolResult {
                    tool_use_id: "call_g".into(),
                    text: "src/routes.ts:3".into(),
                    is_error: false
                }]
            ),
            (
                "assistant",
                &vec![Block::Text {
                    text: "Two endpoints.".into()
                }]
            ),
        ]
    );
    assert_eq!(a1[0].uuid, "e13");
    assert_eq!(a1[0].timestamp.as_deref(), Some("2026-11-03T10:00:11.000Z"));
    // The nested task is an `Agent` call with the keys the flow reads.
    let a2 = entries(&updates, "a2");
    let Block::ToolUse { name, input, .. } = &a2[1].blocks[1] else {
        panic!("tool use: {:?}", a2[1].blocks);
    };
    assert_eq!(name, "Agent");
    let input: Value = serde_json::from_str(input).unwrap();
    assert_eq!(input["description"], "Write the tests");
    assert_eq!(input["subagent_type"], "test-writer");
    assert_eq!(input["prompt"], "Tests.");
    assert!(input.get("agent_type").is_none());
    assert!(entries(&updates, "a3").is_empty());
}

#[test]
fn later_events_stream_and_completion_marks_done() {
    let dir = session("stream", FIXTURE);
    let mut tracker = Tracker::new(&dir);
    tracker.refresh();
    assert!(tracker.refresh().is_empty());
    append(
        &dir.join(EVENTS),
        concat!(
            r#"{"type":"tool.execution_complete","data":{"toolCallId":"call_x","success":false,"error":{"message":"denied"}},"agentId":"a3","id":"e28","timestamp":"2026-11-03T10:01:04.000Z"}"#,
            "\n",
            r#"{"type":"subagent.failed","data":{"toolCallId":"call_t3"},"agentId":"a3","id":"e29","timestamp":"2026-11-03T10:01:05.000Z"}"#,
            "\n",
            r#"{"type":"subagent.completed","data":{"toolCallId":"call_t2"},"agentId":"a2","id":"e30","timestamp":"2026-11-03T10:02:00.000Z"}"#,
            "\n",
            r#"{"type":"subagent.compl"#,
        ),
    );
    let updates = tracker.refresh();
    let status: Vec<_> = list(&updates).iter().map(|a| a.status).collect();
    assert_eq!(status, vec!["done", "done", "done"]);
    assert_eq!(
        entries(&updates, "a3")[0].blocks,
        vec![Block::ToolResult {
            tool_use_id: "call_x".into(),
            text: "denied".into(),
            is_error: true
        }]
    );
    let (calls, _) = main(&updates);
    assert_eq!(calls[2].ended.as_deref(), Some("2026-11-03T10:02:00.000Z"));
    assert!(!calls[2].is_error);
}

#[test]
fn a_failed_main_task_ends_its_call_as_an_error() {
    let start = r#"{"type":"tool.execution_start","data":{"toolCallId":"t","toolName":"task","arguments":{"description":"D"}},"id":"1","timestamp":"t1"}"#;
    let done = r#"{"type":"tool.execution_complete","data":{"toolCallId":"t","success":false},"id":"2","timestamp":"t2"}"#;
    let updates = Tracker::new(&session("failed", &format!("{start}\n{done}\n"))).refresh();
    let (calls, _) = main(&updates);
    assert_eq!(
        (calls[0].ended.as_deref(), calls[0].is_error),
        (Some("t2"), true)
    );
}

#[test]
fn the_last_main_assistant_message_with_text_is_reported() {
    let text = concat!(
        r#"{"type":"assistant.message","data":{"messageId":"m1","content":"First."},"id":"1","timestamp":"t1"}"#,
        "\n",
        r#"{"type":"assistant.message","data":{"messageId":"m2","content":"Final answer."},"id":"2","timestamp":"t2"}"#,
        "\n",
        r#"{"type":"assistant.message","data":{"messageId":"m3","content":"","toolRequests":[]},"id":"3","timestamp":"t3"}"#,
        "\n",
        r#"{"type":"assistant.message","data":{"messageId":"m4","content":"Subagent."},"agentId":"a1","id":"4","timestamp":"t4"}"#,
        "\n",
    );
    let dir = session("last-message", text);
    let mut tracker = Tracker::new(&dir);
    let updates = tracker.refresh();
    let last = updates.iter().find_map(|u| match u {
        Update::Main { last_message, .. } => Some(last_message.clone()),
        _ => None,
    });
    assert_eq!(last, Some(Some("Final answer.".to_owned())));
    assert!(
        tracker
            .refresh()
            .iter()
            .all(|u| !matches!(u, Update::Main { .. }))
    );
}

#[test]
fn unknown_and_broken_lines_are_skipped() {
    let text = concat!(
        "not json\n",
        "[1,2]\n",
        r#"{"type":"tool.execution_start"}"#,
        "\n",
        r#"{"type":"tool.execution_start","data":{"toolCallId":7}}"#,
        "\n",
        r#"{"type":"assistant.message","data":{"toolRequests":[{"name":"x"},{"toolCallId":"y"}]},"agentId":"a"}"#,
        "\n",
        r#"{"type":"hologram","agentId":"","data":{}}"#,
        "\n",
    );
    let updates = Tracker::new(&session("broken", text)).refresh();
    // One subagent seen (by its events), with no messages and no spawning call.
    let agents = list(&updates);
    assert_eq!(agents.len(), 1);
    assert_eq!(
        (agents[0].agent_id.as_str(), agents[0].tool_use_id.as_str()),
        ("a", "")
    );
    assert!(entries(&updates, "a").is_empty());
    assert!(updates.iter().all(|u| !matches!(u, Update::Main { .. })));
}

#[test]
fn a_session_without_events_is_empty() {
    let dir = temp_dir("empty");
    assert!(Tracker::new(&dir).refresh().is_empty());
}

#[test]
fn watch_reports_events_written_afterwards() {
    let dir = session("watch", "");
    let (tx, rx) = mpsc::channel();
    let handle = watch(&dir, move |u| {
        let _ = tx.send(u);
    })
    .unwrap();
    append(
        &dir.join(EVENTS),
        concat!(
            r#"{"type":"subagent.started","data":{"toolCallId":"t"},"agentId":"a","id":"1","timestamp":"t1"}"#,
            "\n"
        ),
    );
    let update = rx.recv_timeout(Duration::from_secs(10)).expect("an update");
    assert!(matches!(update, Update::List { .. }));
    drop(handle);
}
