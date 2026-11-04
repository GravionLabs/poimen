//! Live subagents of GitHub Copilot CLI agents.
//!
//! Copilot CLI writes one event log per session, read-only here:
//! `~/.copilot/session-state/<session>/events.jsonl`, one JSON object per line
//! (`{type, data, id, parentId, timestamp, agentId?}`). Subagents live in the same file: their
//! events carry the `agentId` of the subagent. The format is internal to Copilot CLI and may
//! change, so unknown event types and fields are skipped and a broken line is ignored.
//!
//! The events are mapped onto the wire types of [`transcripts`](crate::transcripts), so the
//! frontend needs no Copilot-specific code:
//!
//! - `tool.execution_start` of the main session: `skill` becomes a `Skill` call, `task` an `Agent`
//!   call, every other tool counts as a plain tool call;
//! - `subagent.started` (`data.toolCallId` is the spawning `task` call) lists a subagent,
//!   `subagent.completed` / `subagent.failed` marks it done (the result of a background `task`
//!   call is only the launch acknowledgement and does not count);
//! - `user.message`, `assistant.message` (text, reasoning, tool requests) and
//!   `tool.execution_complete` of a subagent become its messages; `task` and `skill` requests are
//!   named `Agent` and `Skill` there, so nested subagents are found by their tool call id.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::herdr::is_safe_session_id;
use crate::transcripts::{
    self, Block, Entry, INPUT_LIMIT, MainCall, SubagentInfo, TEXT_LIMIT, Tailer, Update,
    WatchHandle, truncate,
};

/// `~/.copilot/session-state`.
pub fn sessions_root(home: &Path) -> PathBuf {
    home.join(".copilot").join("session-state")
}

/// The directory of `session` below `root` when it holds an `events.jsonl`; `None` for a session
/// id that is not a plain file name.
pub fn locate_session_dir(root: &Path, session: &str) -> Option<PathBuf> {
    if !is_safe_session_id(session) {
        return None;
    }
    let dir = root.join(session);
    dir.join(EVENTS).is_file().then_some(dir)
}

const EVENTS: &str = "events.jsonl";

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// The name and input a tool call gets on the wire: `task` and `skill` take the names and input
/// keys of Claude Code's `Agent` and `Skill` calls, other tools keep theirs.
fn tool_call(name: &str, arguments: &Value) -> (String, String) {
    let input = match name {
        "task" => {
            // The keys the flow reads come first, so a truncated prompt cannot cut them off.
            let mut object = Map::new();
            object.insert("description".into(), text(arguments, "description").into());
            object.insert("subagent_type".into(), text(arguments, "agent_type").into());
            let mut input = Value::Object(object).to_string();
            input.pop();
            if let Some(rest) = arguments.as_object() {
                for (key, value) in rest {
                    if !matches!(key.as_str(), "description" | "agent_type") {
                        input.push_str(&format!(",{}:{value}", Value::from(key.as_str())));
                    }
                }
            }
            input.push('}');
            input
        }
        _ => arguments.to_string(),
    };
    let name = match name {
        "task" => "Agent",
        "skill" => "Skill",
        other => other,
    };
    (name.to_owned(), truncate(&input, INPUT_LIMIT))
}

/// The text of a `tool.execution_complete`: its result, else its error message.
fn result_text(data: &Value) -> String {
    let content = data.get("result").and_then(|r| r.get("content"));
    let text = match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => self::text(data.get("error").unwrap_or(&Value::Null), "message").to_owned(),
    };
    truncate(&text, TEXT_LIMIT)
}

#[derive(Debug, Default)]
struct Agent {
    agent_type: String,
    description: String,
    tool_use_id: String,
    spawn_depth: u32,
    started: Option<String>,
    last: Option<String>,
    done: bool,
}

/// Follows the event log of one session.
pub struct Tracker {
    events: Tailer,
    agents: BTreeMap<String, Agent>,
    /// `task` calls of subagents: tool call id to the spawning subagent.
    spawned_by: HashMap<String, String>,
    listed: Vec<SubagentInfo>,
    calls: Vec<MainCall>,
    plain: HashSet<String>,
    sent_main: (Vec<MainCall>, u32),
}

/// Event lines worth parsing: subagent events and the main session's messages and tool calls.
fn relevant(line: &str) -> bool {
    line.contains("\"agentId\"")
        || line.contains("\"tool.execution_")
        || line.contains("\"subagent.")
}

impl Tracker {
    /// `session_dir` holds `events.jsonl`.
    pub fn new(session_dir: &Path) -> Self {
        Self {
            events: Tailer::new(session_dir.join(EVENTS)),
            agents: BTreeMap::new(),
            spawned_by: HashMap::new(),
            listed: Vec::new(),
            calls: Vec::new(),
            plain: HashSet::new(),
            sent_main: (Vec::new(), 0),
        }
    }

    /// Reads what the log gained; the updates are empty when nothing changed.
    pub fn refresh(&mut self) -> Vec<Update> {
        let mut entries: BTreeMap<String, Vec<Entry>> = BTreeMap::new();
        for line in self.events.read_new() {
            if !relevant(&line) {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<Value>(&line)
                && event.is_object()
            {
                self.apply(&event, &mut entries);
            }
        }
        let mut updates = Vec::new();
        let mut agents: Vec<SubagentInfo> = self
            .agents
            .iter()
            .map(|(id, a)| SubagentInfo {
                agent_id: id.clone(),
                agent_type: a.agent_type.clone(),
                description: a.description.clone(),
                tool_use_id: a.tool_use_id.clone(),
                spawn_depth: a.spawn_depth,
                worktree_branch: None,
                status: if a.done { "done" } else { "running" },
                started: a.started.clone(),
                last: a.last.clone(),
            })
            .collect();
        agents.sort_by(|a, b| (&a.started, &a.agent_id).cmp(&(&b.started, &b.agent_id)));
        if agents != self.listed {
            self.listed = agents.clone();
            updates.push(Update::List { agents });
        }
        for (agent_id, entries) in entries {
            updates.push(Update::Entries { agent_id, entries });
        }
        let tools = u32::try_from(self.plain.len()).unwrap_or(u32::MAX);
        if (&self.calls, tools) != (&self.sent_main.0, self.sent_main.1) {
            self.sent_main = (self.calls.clone(), tools);
            updates.push(Update::Main {
                calls: self.calls.clone(),
                tools,
            });
        }
        updates
    }

    fn apply(&mut self, event: &Value, entries: &mut BTreeMap<String, Vec<Entry>>) {
        let kind = text(event, "type");
        let data = event.get("data").unwrap_or(&Value::Null);
        let timestamp = event
            .get("timestamp")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let call_id = text(data, "toolCallId");
        let agent_id = event
            .get("agentId")
            .and_then(Value::as_str)
            .filter(|a| !a.is_empty());

        // Completion is read for every subagent, wherever the event sits.
        if matches!(kind, "subagent.completed" | "subagent.failed") {
            if let Some(call) = self.calls.iter_mut().find(|c| c.tool_use_id == call_id) {
                call.ended = timestamp.clone();
                call.is_error = kind == "subagent.failed";
            }
            if let Some(id) = agent_id {
                self.agents.entry(id.to_owned()).or_default().done = true;
            }
        }

        let Some(agent_id) = agent_id else {
            self.apply_main(kind, data, call_id, timestamp);
            return;
        };
        if kind == "subagent.started" {
            let depth = self
                .spawned_by
                .get(call_id)
                .and_then(|parent| self.agents.get(parent))
                .map_or(1, |parent| parent.spawn_depth + 1);
            let agent = self.agents.entry(agent_id.to_owned()).or_default();
            agent.agent_type = text(data, "agentName").to_owned();
            agent.description = [
                text(data, "agentDescription"),
                text(data, "agentDisplayName"),
            ]
            .into_iter()
            .find(|d| !d.is_empty())
            .map(|d| truncate(d, 200))
            .unwrap_or_default();
            agent.tool_use_id = call_id.to_owned();
            agent.spawn_depth = depth;
        }
        let agent = self.agents.entry(agent_id.to_owned()).or_default();
        if let Some(ts) = &timestamp {
            agent.started.get_or_insert_with(|| ts.clone());
            agent.last = Some(ts.clone());
        }
        if kind == "tool.execution_start" && text(data, "toolName") == "task" && !call_id.is_empty()
        {
            self.spawned_by
                .insert(call_id.to_owned(), agent_id.to_owned());
        }
        let entry = |role: &str, blocks: Vec<Block>| Entry {
            uuid: text(event, "id").to_owned(),
            role: role.to_owned(),
            timestamp: timestamp.clone(),
            blocks,
        };
        let message = match kind {
            "user.message" => {
                let content = text(data, "content");
                (!content.is_empty()).then(|| {
                    entry(
                        "user",
                        vec![Block::Text {
                            text: truncate(content, TEXT_LIMIT),
                        }],
                    )
                })
            }
            "assistant.message" => {
                let mut blocks = Vec::new();
                let thinking = text(data, "reasoningText");
                if !thinking.is_empty() {
                    blocks.push(Block::Thinking {
                        text: truncate(thinking, TEXT_LIMIT),
                    });
                }
                let content = text(data, "content");
                if !content.is_empty() {
                    blocks.push(Block::Text {
                        text: truncate(content, TEXT_LIMIT),
                    });
                }
                let requests = data.get("toolRequests").and_then(Value::as_array);
                for request in requests.into_iter().flatten() {
                    let id = text(request, "toolCallId");
                    let name = text(request, "name");
                    if id.is_empty() || name.is_empty() {
                        continue;
                    }
                    let arguments = request.get("arguments").unwrap_or(&Value::Null);
                    let (name, input) = tool_call(name, arguments);
                    blocks.push(Block::ToolUse {
                        id: id.to_owned(),
                        name,
                        input,
                    });
                }
                (!blocks.is_empty()).then(|| entry("assistant", blocks))
            }
            "tool.execution_complete" if !call_id.is_empty() => Some(entry(
                "user",
                vec![Block::ToolResult {
                    tool_use_id: call_id.to_owned(),
                    text: result_text(data),
                    is_error: data.get("success").and_then(Value::as_bool) == Some(false),
                }],
            )),
            _ => None,
        };
        if let Some(message) = message {
            entries
                .entry(agent_id.to_owned())
                .or_default()
                .push(message);
        }
    }

    fn apply_main(&mut self, kind: &str, data: &Value, call_id: &str, timestamp: Option<String>) {
        if call_id.is_empty() {
            return;
        }
        match kind {
            "tool.execution_start" => {
                if self.plain.contains(call_id)
                    || self.calls.iter().any(|c| c.tool_use_id == call_id)
                {
                    return;
                }
                let arguments = data.get("arguments").unwrap_or(&Value::Null);
                let (name, target, detail) = match text(data, "toolName") {
                    "skill" => ("Skill", text(arguments, "skill"), text(arguments, "args")),
                    "task" => (
                        "Agent",
                        text(arguments, "description"),
                        text(arguments, "agent_type"),
                    ),
                    _ => {
                        self.plain.insert(call_id.to_owned());
                        return;
                    }
                };
                self.calls.push(MainCall {
                    tool_use_id: call_id.to_owned(),
                    name: name.to_owned(),
                    target: truncate(target, 200),
                    detail: truncate(detail, 200),
                    started: timestamp,
                    ended: None,
                    is_error: false,
                });
            }
            "tool.execution_complete" => {
                let failed = data.get("success").and_then(Value::as_bool) == Some(false);
                if let Some(call) = self.calls.iter_mut().find(|c| c.tool_use_id == call_id)
                    // An agent call ends with its subagent (a background call answers at once).
                    && (call.name == "Skill" || failed)
                {
                    call.ended = timestamp;
                    call.is_error = failed;
                }
            }
            _ => {}
        }
    }
}

/// Watches the event log in `session_dir` and calls `emit` with every update, first with what
/// the log already holds.
pub fn watch(
    session_dir: &Path,
    emit: impl Fn(Update) + Send + 'static,
) -> Result<WatchHandle, String> {
    let dir = session_dir.canonicalize().map_err(|e| e.to_string())?;
    let file = dir.join(EVENTS);
    let mut tracker = Tracker::new(&dir);
    transcripts::watch_dir(&dir, move |p| p == file, move || tracker.refresh(), emit)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    const FIXTURE: &str = include_str!("../fixtures/copilot-events.jsonl");

    fn temp_dir(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "poimen-copilot-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn append(path: &Path, text: &str) {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    fn session(name: &str, text: &str) -> PathBuf {
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
                Update::Main { calls, tools } => Some((calls.as_slice(), *tools)),
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
}
