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

use crate::transcripts::{
    self, Block, Entry, INPUT_LIMIT, MainCall, SubagentInfo, TEXT_LIMIT, Tailer, Update, Usage,
    UsageSample, WatchHandle, is_safe_session_id, truncate,
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

pub(crate) const EVENTS: &str = "events.jsonl";

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

/// `session.usage_record` data: model and token counts (`usage.inputTokens`, `outputTokens`,
/// `cacheReadTokens`, `cacheWriteTokens`). `None` when no count is present or one is not a
/// non-negative integer.
pub fn parse_usage_record(data: &Value) -> Option<(Option<String>, Usage)> {
    let usage = data.get("usage")?.as_object()?;
    let mut known = false;
    let mut count = |key: &str| -> Option<u64> {
        match usage.get(key) {
            None | Some(Value::Null) => Some(0),
            Some(v) => {
                known = true;
                v.as_u64()
            }
        }
    };
    let parsed = Usage {
        input: count("inputTokens")?,
        output: count("outputTokens")?,
        cache_read: count("cacheReadTokens")?,
        cache_creation: count("cacheWriteTokens")?,
    };
    let model = usage
        .get("model")
        .and_then(Value::as_str)
        .map(str::to_owned);
    known.then_some((model, parsed))
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
    /// One sample per `session.usage_record` of the main session.
    usage: Vec<UsageSample>,
    sent_usage: usize,
    last_message: Option<String>,
    sent_last: Option<String>,
}

/// The usage record of one `events.jsonl` line of the main session: its event id and counts.
/// `None` for any other line (or a malformed one).
pub fn parse_usage_line(line: &str) -> Option<(String, Usage)> {
    if !line.contains("\"session.usage_record\"") {
        return None;
    }
    let event: Value = serde_json::from_str(line).ok()?;
    if event.get("type").and_then(Value::as_str) != Some("session.usage_record")
        || event.get("agentId").is_some()
    {
        return None;
    }
    let (_, usage) = parse_usage_record(event.get("data")?)?;
    Some((event.get("id").and_then(Value::as_str)?.to_owned(), usage))
}

/// [`parse_usage_line`] with the event's timestamp.
pub fn parse_usage_stamped(line: &str) -> Option<(String, Usage, Option<String>)> {
    let (id, usage) = parse_usage_line(line)?;
    let at = serde_json::from_str::<Value>(line)
        .ok()?
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Some((id, usage, at))
}

/// Watches the usage of the session in `session_dir`.
pub fn watch_usage(
    session_dir: &Path,
    emit: impl Fn(transcripts::Totals, Vec<transcripts::Sample>) + Send + 'static,
) -> Result<WatchHandle, String> {
    let dir = session_dir.canonicalize().map_err(|e| e.to_string())?;
    let file = dir.join(EVENTS);
    transcripts::watch_usage(
        transcripts::UsageReader::copilot(&dir),
        &dir,
        move |p| p == file,
        emit,
    )
}

/// Event lines worth parsing: subagent events and the main session's messages and tool calls.
fn relevant(line: &str) -> bool {
    line.contains("\"agentId\"")
        || line.contains("\"tool.execution_")
        || line.contains("\"subagent.")
        || line.contains("\"session.usage_record\"")
        || line.contains("\"assistant.message\"")
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
            usage: Vec::new(),
            sent_usage: 0,
            last_message: None,
            sent_last: None,
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
        if self.usage.len() != self.sent_usage
            || self.last_message != self.sent_last
            || (&self.calls, tools) != (&self.sent_main.0, self.sent_main.1)
        {
            self.sent_usage = self.usage.len();
            self.sent_last = self.last_message.clone();
            self.sent_main = (self.calls.clone(), tools);
            updates.push(Update::Main {
                calls: self.calls.clone(),
                tools,
                usage: self.usage.clone(),
                last_message: self.last_message.clone(),
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
            if kind == "session.usage_record" {
                if let Some((model, usage)) = parse_usage_record(data) {
                    self.usage.push(UsageSample {
                        id: text(event, "id").to_owned(),
                        timestamp,
                        model,
                        usage,
                    });
                }
                return;
            }
            if kind == "assistant.message" {
                let content = text(data, "content");
                if !content.trim().is_empty() {
                    self.last_message = Some(truncate(content, TEXT_LIMIT));
                }
            }
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
            message_id: None,
            model: None,
            usage: None,
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
                    context: None,
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
mod tests;
