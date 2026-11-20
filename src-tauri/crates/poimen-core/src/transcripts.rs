//! Live subagent transcripts of Claude Code agents.
//!
//! Subagents do not write to the terminal, so what they do is read from Claude Code's session
//! transcripts (read-only; the JSONL format is internal to Claude Code and may change, so every
//! parser here skips what it does not know instead of failing):
//!
//! - `~/.claude/projects/<cwd, non-alphanumerics as "-">/<session>.jsonl`: the main transcript;
//! - `<session>/subagents/agent-<id>.jsonl`: one subagent (prompt, messages, tool calls, results);
//! - `<session>/subagents/agent-<id>.meta.json`: `agentType`, `description`, `toolUseId`,
//!   `spawnDepth`, `worktreeBranch` and, in recent versions, `finished`.
//!
//! A subagent is done when its meta says `finished`, or when the main transcript holds a
//! `tool_result` for its `toolUseId` that is not the "launched in the background" acknowledgement.
//! The watcher is event driven (`notify`); files are tailed from the last read offset.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};

use serde::Serialize;
use serde_json::Value;

use crate::copilot;

/// A session id is used as a file or directory name below the agent's transcript root; only
/// `[A-Za-z0-9_-]+` is accepted, so a value reported by a pane cannot escape that directory.
pub fn is_safe_session_id(session: &str) -> bool {
    !session.is_empty()
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Longest text kept per block; a transcript can hold whole files and diffs.
pub(crate) const TEXT_LIMIT: usize = 8000;
pub(crate) const INPUT_LIMIT: usize = 2000;

/// `~/.claude/projects`.
pub fn projects_root(home: &Path) -> PathBuf {
    home.join(".claude").join("projects")
}

/// The directory name Claude Code uses for a working directory: every character that is not an
/// ASCII letter or digit becomes `-` (so `/a/.b_c` is `-a--b-c`).
pub fn project_dir_name(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// The project directory holding the transcript of `session`: the one named after `cwd`, else any
/// project directory that has the session file (a session can outlive a renamed path spelling).
pub fn locate_project_dir(root: &Path, cwd: &str, session: &str) -> Option<PathBuf> {
    if !is_safe_session_id(session) {
        return None;
    }
    let file = format!("{session}.jsonl");
    let direct = root.join(project_dir_name(cwd));
    if direct.join(&file).is_file() {
        return Some(direct);
    }
    std::fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .find(|p| p.join(&file).is_file())
}

/// One piece of a message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text {
        text: String,
    },
    Thinking {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: String,
    },
    ToolResult {
        tool_use_id: String,
        text: String,
        is_error: bool,
    },
}

/// Token counts of one assistant message (`message.usage`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl Usage {
    /// Context fill of the message: everything the model read (input and both cache counts).
    pub fn context(&self) -> u64 {
        self.input
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_creation)
    }
}

/// Reads `message.usage`. `None` (unknown, never an error) when it is missing, not an object,
/// holds no count, or a count is not a non-negative integer.
pub fn parse_usage(message: &Value) -> Option<Usage> {
    let usage = message.get("usage")?.as_object()?;
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
        input: count("input_tokens")?,
        output: count("output_tokens")?,
        cache_read: count("cache_read_input_tokens")?,
        cache_creation: count("cache_creation_input_tokens")?,
    };
    known.then_some(parsed)
}

/// Usage of one assistant message of the main transcript (flow graph).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageSample {
    /// The API message id: a message written in several lines counts once.
    pub id: String,
    pub timestamp: Option<String>,
    pub model: Option<String>,
    pub usage: Usage,
}

/// The usage sample of one transcript line, if it is an assistant message that has usage.
pub fn parse_usage_sample(line: &str) -> Option<UsageSample> {
    let value: Value = serde_json::from_str(line).ok()?;
    if value.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let message = value.get("message")?;
    let usage = parse_usage(message)?;
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    Some(UsageSample {
        id: text(message, "id").or_else(|| text(&value, "uuid"))?,
        timestamp: text(&value, "timestamp"),
        model: text(message, "model"),
        usage,
    })
}

/// A user or assistant message of a transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub uuid: String,
    /// `user` or `assistant`.
    pub role: String,
    pub timestamp: Option<String>,
    pub blocks: Vec<Block>,
    /// API message id; several lines of one message share it (and its usage).
    pub message_id: Option<String>,
    pub model: Option<String>,
    /// `None` when the transcript has no (or unreadable) usage for the message.
    pub usage: Option<Usage>,
}

/// One transcript line: its timestamp (any entry type has one) and the message, if it is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub timestamp: Option<String>,
    pub entry: Option<Entry>,
}

pub(crate) fn truncate(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_block(block: &Value) -> Option<Block> {
    let text = |key: &str| block.get(key).and_then(Value::as_str);
    match text("type")? {
        "text" => Some(Block::Text {
            text: truncate(text("text")?, TEXT_LIMIT),
        }),
        "thinking" => Some(Block::Thinking {
            text: truncate(text("thinking")?, TEXT_LIMIT),
        }),
        "tool_use" => Some(Block::ToolUse {
            id: text("id")?.to_owned(),
            name: text("name")?.to_owned(),
            input: truncate(&block.get("input")?.to_string(), INPUT_LIMIT),
        }),
        "tool_result" => Some(Block::ToolResult {
            tool_use_id: text("tool_use_id")?.to_owned(),
            text: truncate(&result_text(block.get("content")?), TEXT_LIMIT),
            is_error: block
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        _ => None,
    }
}

/// Parses one JSONL line. `None` for text that is not a JSON object; a known object of an
/// unknown type (attachments, queue operations, ...) has a timestamp but no entry.
pub fn parse_line(line: &str) -> Option<Line> {
    let value: Value = serde_json::from_str(line).ok()?;
    let object = value.as_object()?;
    let timestamp = object
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let entry = (|| {
        let role = object.get("type")?.as_str()?;
        if !matches!(role, "user" | "assistant")
            || object.get("isMeta").and_then(Value::as_bool) == Some(true)
        {
            return None;
        }
        let message = object.get("message")?;
        let content = message.get("content")?;
        let blocks: Vec<Block> = match content {
            Value::String(text) => vec![Block::Text {
                text: truncate(text, TEXT_LIMIT),
            }],
            Value::Array(parts) => parts.iter().filter_map(parse_block).collect(),
            _ => Vec::new(),
        };
        (!blocks.is_empty()).then(|| Entry {
            uuid: object
                .get("uuid")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            role: role.to_owned(),
            timestamp: timestamp.clone(),
            blocks,
            message_id: message.get("id").and_then(Value::as_str).map(str::to_owned),
            model: message
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_owned),
            usage: parse_usage(message),
        })
    })();
    Some(Line { timestamp, entry })
}

/// Final tool results of a main-transcript line as `(tool_use_id, is_error)`. The acknowledgement
/// of a background launch (`toolUseResult.isAsync` / `status: async_launched`) does not count.
pub fn final_results(line: &str) -> Vec<(String, bool)> {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return Vec::new();
    };
    let result = value.get("toolUseResult");
    let background = result
        .and_then(|r| r.get("isAsync"))
        .and_then(Value::as_bool)
        == Some(true)
        || result.and_then(|r| r.get("status")).and_then(Value::as_str) == Some("async_launched");
    if background || value.get("type").and_then(Value::as_str) != Some("user") {
        return Vec::new();
    }
    value
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
        .filter_map(|b| {
            Some((
                b.get("tool_use_id").and_then(Value::as_str)?.to_owned(),
                b.get("is_error").and_then(Value::as_bool) == Some(true),
            ))
        })
        .collect()
}

/// A `Skill` or `Agent` call of the main transcript, the nodes of the flow graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MainCall {
    pub tool_use_id: String,
    /// `Skill` or `Agent`.
    pub name: String,
    /// The skill name, or the description of the subagent.
    pub target: String,
    /// The skill arguments, or the subagent type.
    pub detail: String,
    pub started: Option<String>,
    /// Timestamp of the final result; `None` while the call has none.
    pub ended: Option<String>,
    pub is_error: bool,
    /// Context fill of the message that made the call; `None` when unknown.
    pub context: Option<u64>,
}

/// Skill and agent calls of one main-transcript line, and how many other tool calls it holds.
pub fn parse_main_calls(line: &str) -> (Vec<MainCall>, Vec<String>) {
    let mut calls = Vec::new();
    let mut plain = Vec::new();
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        return (calls, plain);
    };
    if value.get("type").and_then(Value::as_str) != Some("assistant") {
        return (calls, plain);
    }
    let started = value
        .get("timestamp")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let context = value
        .get("message")
        .and_then(parse_usage)
        .map(|u| u.context());
    let blocks = value
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(Value::as_array);
    for block in blocks.into_iter().flatten() {
        if block.get("type").and_then(Value::as_str) != Some("tool_use") {
            continue;
        }
        let text = |v: &Value, key: &str| {
            v.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let id = text(block, "id");
        let name = text(block, "name");
        let input = block.get("input").unwrap_or(&Value::Null);
        let (target, detail) = match name.as_str() {
            "Skill" => (text(input, "skill"), text(input, "args")),
            "Agent" => (text(input, "description"), text(input, "subagent_type")),
            _ => {
                plain.push(id);
                continue;
            }
        };
        calls.push(MainCall {
            tool_use_id: id,
            name,
            target: truncate(&target, 200),
            detail: truncate(&detail, 200),
            started: started.clone(),
            ended: None,
            is_error: false,
            context,
        });
    }
    (calls, plain)
}

/// `agent-<id>.meta.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    pub agent_type: String,
    pub description: String,
    pub tool_use_id: String,
    pub spawn_depth: u32,
    pub worktree_branch: Option<String>,
    pub finished: bool,
}

pub fn parse_meta(text: &str) -> Meta {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Meta::default();
    };
    let string = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Meta {
        agent_type: string("agentType"),
        description: string("description"),
        tool_use_id: string("toolUseId"),
        spawn_depth: value
            .get("spawnDepth")
            .and_then(Value::as_u64)
            .and_then(|d| u32::try_from(d).ok())
            .unwrap_or(0),
        worktree_branch: Some(string("worktreeBranch")).filter(|b| !b.is_empty()),
        finished: value.get("finished").and_then(Value::as_bool) == Some(true),
    }
}

/// A subagent as the panel lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SubagentInfo {
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    pub tool_use_id: String,
    pub spawn_depth: u32,
    pub worktree_branch: Option<String>,
    /// `running` or `done`.
    pub status: &'static str,
    /// Timestamp of the first and of the latest transcript line.
    pub started: Option<String>,
    pub last: Option<String>,
}

/// Reads the lines a file gained since the last call. A half-written last line waits for its
/// newline; a file that shrank (rewritten) is read from the start again.
#[derive(Debug)]
pub struct Tailer {
    path: PathBuf,
    offset: u64,
    partial: Vec<u8>,
}

impl Tailer {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            offset: 0,
            partial: Vec::new(),
        }
    }

    pub fn read_new(&mut self) -> Vec<String> {
        let Ok(mut file) = File::open(&self.path) else {
            return Vec::new();
        };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            self.offset = 0;
            self.partial.clear();
        }
        if len == self.offset || file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        if file.read_to_end(&mut bytes).is_err() {
            return Vec::new();
        }
        self.offset += bytes.len() as u64;
        self.partial.extend(bytes);
        // One pass; only the bytes after the last newline stay for the next read.
        let Some(last) = self.partial.iter().rposition(|b| *b == b'\n') else {
            return Vec::new();
        };
        let tail = self.partial.split_off(last + 1);
        let complete = std::mem::replace(&mut self.partial, tail);
        complete[..last]
            .split(|b| *b == b'\n')
            .map(|line| String::from_utf8_lossy(line).trim().to_owned())
            .filter(|text| !text.is_empty())
            .collect()
    }
}

/// What a refresh found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Update {
    /// The subagents of the session changed (a new one, a status, a timestamp).
    List { agents: Vec<SubagentInfo> },
    /// New messages of one subagent, in order.
    Entries {
        agent_id: String,
        entries: Vec<Entry>,
    },
    /// The main agent's skill and agent calls and its count of other tool calls (flow graph).
    /// `usage` holds one sample per assistant message of the main transcript (empty when it has
    /// none).
    Main {
        calls: Vec<MainCall>,
        tools: u32,
        usage: Vec<UsageSample>,
        /// The text of the main agent's last assistant message that has any (#979), cut at
        /// [`TEXT_LIMIT`]; absent while there is none.
        #[serde(skip_serializing_if = "Option::is_none")]
        last_message: Option<String>,
    },
}

/// The text of an assistant line of the main transcript: its text blocks joined, `None` for any
/// other line or one without text (a tool call alone).
pub fn assistant_text(line: &str) -> Option<String> {
    if !line.contains("\"assistant\"") || !line.contains("\"text\"") {
        return None;
    }
    let entry = parse_line(line)?.entry?;
    if entry.role != "assistant" {
        return None;
    }
    let text = entry
        .blocks
        .iter()
        .filter_map(|b| match b {
            Block::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

struct Tracked {
    tailer: Tailer,
    started: Option<String>,
    last: Option<String>,
}

/// Follows the transcripts of one session.
pub struct Tracker {
    sub_dir: PathBuf,
    parent: Tailer,
    done: HashSet<String>,
    agents: BTreeMap<String, Tracked>,
    listed: Vec<SubagentInfo>,
    calls: Vec<MainCall>,
    plain: HashSet<String>,
    sent_main: (Vec<MainCall>, u32),
    /// One sample per message id, in order of first appearance; a later line replaces its sample.
    usage: Vec<UsageSample>,
    usage_index: HashMap<String, usize>,
    last_message: Option<String>,
    sent_last: Option<String>,
}

impl Tracker {
    /// `project_dir` holds `<session>.jsonl` and the `<session>/` directory; `None` for a session
    /// id that is not a plain file name.
    pub fn new(project_dir: &Path, session: &str) -> Option<Self> {
        if !is_safe_session_id(session) {
            return None;
        }
        Some(Self {
            sub_dir: project_dir.join(session).join("subagents"),
            parent: Tailer::new(project_dir.join(format!("{session}.jsonl"))),
            done: HashSet::new(),
            agents: BTreeMap::new(),
            listed: Vec::new(),
            calls: Vec::new(),
            plain: HashSet::new(),
            sent_main: (Vec::new(), 0),
            usage: Vec::new(),
            usage_index: HashMap::new(),
            last_message: None,
            sent_last: None,
        })
    }

    /// Reads what the files gained; the updates are empty when nothing changed.
    pub fn refresh(&mut self) -> Vec<Update> {
        let mut usage_changed = false;
        for line in self.parent.read_new() {
            if line.contains("\"usage\"")
                && let Some(sample) = parse_usage_sample(&line)
            {
                match self.usage_index.get(&sample.id) {
                    Some(&i) => self.usage[i] = sample,
                    None => {
                        self.usage_index.insert(sample.id.clone(), self.usage.len());
                        self.usage.push(sample);
                    }
                }
                usage_changed = true;
            }
            if let Some(text) = assistant_text(&line) {
                self.last_message = Some(text);
            }
            if line.contains("\"tool_use\"") {
                let (calls, plain) = parse_main_calls(&line);
                for call in calls {
                    if !self.calls.iter().any(|c| c.tool_use_id == call.tool_use_id) {
                        self.calls.push(call);
                    }
                }
                self.plain.extend(plain);
            }
            if line.contains("tool_result") {
                let ended = parse_line(&line).and_then(|l| l.timestamp);
                for (id, is_error) in final_results(&line) {
                    if let Some(call) = self.calls.iter_mut().find(|c| c.tool_use_id == id) {
                        call.ended = ended.clone();
                        call.is_error = is_error;
                    }
                    self.done.insert(id);
                }
            }
        }
        if let Ok(dir) = std::fs::read_dir(&self.sub_dir) {
            for path in dir.flatten().map(|e| e.path()) {
                let id = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .and_then(|n| n.strip_prefix("agent-"))
                    .and_then(|n| n.strip_suffix(".jsonl"));
                if let Some(id) = id {
                    self.agents.entry(id.to_owned()).or_insert_with(|| Tracked {
                        tailer: Tailer::new(path.clone()),
                        started: None,
                        last: None,
                    });
                }
            }
        }
        let mut updates = Vec::new();
        for (id, agent) in &mut self.agents {
            let mut entries = Vec::new();
            for line in agent.tailer.read_new() {
                let Some(parsed) = parse_line(&line) else {
                    continue;
                };
                if let Some(ts) = parsed.timestamp {
                    agent.started.get_or_insert_with(|| ts.clone());
                    agent.last = Some(ts);
                }
                entries.extend(parsed.entry);
            }
            if !entries.is_empty() {
                updates.push(Update::Entries {
                    agent_id: id.clone(),
                    entries,
                });
            }
        }
        let mut agents: Vec<SubagentInfo> = self
            .agents
            .iter()
            .map(|(id, agent)| {
                let meta =
                    std::fs::read_to_string(self.sub_dir.join(format!("agent-{id}.meta.json")))
                        .map(|t| parse_meta(&t))
                        .unwrap_or_default();
                let finished = meta.finished || self.done.contains(&meta.tool_use_id);
                SubagentInfo {
                    agent_id: id.clone(),
                    status: if finished { "done" } else { "running" },
                    agent_type: meta.agent_type,
                    description: meta.description,
                    tool_use_id: meta.tool_use_id,
                    spawn_depth: meta.spawn_depth,
                    worktree_branch: meta.worktree_branch,
                    started: agent.started.clone(),
                    last: agent.last.clone(),
                }
            })
            .collect();
        agents.sort_by(|a, b| (&a.started, &a.agent_id).cmp(&(&b.started, &b.agent_id)));
        if agents != self.listed {
            self.listed = agents.clone();
            updates.insert(0, Update::List { agents });
        }
        let tools = u32::try_from(self.plain.len()).unwrap_or(u32::MAX);
        if usage_changed
            || self.last_message != self.sent_last
            || (&self.calls, tools) != (&self.sent_main.0, self.sent_main.1)
        {
            self.sent_main = (self.calls.clone(), tools);
            self.sent_last = self.last_message.clone();
            updates.push(Update::Main {
                calls: self.calls.clone(),
                tools,
                usage: self.usage.clone(),
                last_message: self.last_message.clone(),
            });
        }
        updates
    }
}

enum Message {
    Changed,
    Stop,
}

/// A running watch; dropping it stops the thread and the file watcher.
pub struct WatchHandle {
    stop: Sender<Message>,
    _watcher: notify::RecommendedWatcher,
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        let _ = self.stop.send(Message::Stop);
    }
}

/// Watches the project directory of `session` and calls `emit` with every update, first with the
/// existing transcripts. Events, not polling: the thread sleeps until the file system reports a
/// change below the session.
pub fn watch(
    project_dir: &Path,
    session: &str,
    emit: impl Fn(Update) + Send + 'static,
) -> Result<WatchHandle, String> {
    if !is_safe_session_id(session) {
        return Err(format!("not a valid session id: {session:?}"));
    }
    // Events carry resolved paths (`/private/var` for `/var` on macOS).
    let project_dir = &project_dir.canonicalize().map_err(|e| e.to_string())?;
    let session_dir = project_dir.join(session);
    let session_file = project_dir.join(format!("{session}.jsonl"));
    let mut tracker = Tracker::new(project_dir, session).ok_or("not a valid session id")?;
    watch_dir(
        project_dir,
        move |p| p == session_file || p.starts_with(&session_dir),
        move || tracker.refresh(),
        emit,
    )
}

/// Watches `dir` recursively and calls `refresh` once at the start and again after every change
/// to a path `relevant` accepts (bursts coalesced); every update goes to `emit`. `dir` must be
/// canonical, since events carry resolved paths.
pub(crate) fn watch_dir<T>(
    dir: &Path,
    relevant: impl Fn(&Path) -> bool + Send + 'static,
    mut refresh: impl FnMut() -> Vec<T> + Send + 'static,
    emit: impl Fn(T) + Send + 'static,
) -> Result<WatchHandle, String> {
    let (tx, rx) = mpsc::channel();
    let notifier = tx.clone();
    let mut watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
        if let Ok(event) = event
            && event.paths.iter().any(|p| relevant(p))
        {
            let _ = notifier.send(Message::Changed);
        }
    })
    .map_err(|e| e.to_string())?;
    watcher
        .watch(dir, RecursiveMode::Recursive)
        .map_err(|e| e.to_string())?;
    std::thread::spawn(move || {
        for update in refresh() {
            emit(update);
        }
        while let Ok(Message::Changed) = rx.recv() {
            // A burst of writes becomes one refresh.
            std::thread::sleep(Duration::from_millis(40));
            let mut stopped = false;
            while let Ok(message) = rx.try_recv() {
                stopped |= matches!(message, Message::Stop);
            }
            for update in refresh() {
                emit(update);
            }
            if stopped {
                break;
            }
        }
    });
    Ok(WatchHandle {
        stop: tx,
        _watcher: watcher,
    })
}

/// Token totals of a session (#895): the sum of every message's usage, each message id once.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Totals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

/// Sums usage by message id: a message written in several lines (or sent again) counts once, a
/// later line of it replaces the earlier one.
#[derive(Debug, Default)]
pub struct UsageSum {
    seen: HashMap<String, Usage>,
    /// Timestamp (as written in the file) per message id, for the windows of #896.
    stamps: HashMap<String, String>,
}

/// The usage of one message with its timestamp: what a run window (#896) sums.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sample {
    pub at: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl UsageSum {
    /// Records `usage` of message `id`; `true` when the totals changed.
    pub fn add(&mut self, id: &str, usage: Usage) -> bool {
        self.seen.insert(id.to_owned(), usage) != Some(usage)
    }

    /// [`add`](Self::add) that remembers the message's timestamp; a changed timestamp counts as a
    /// change too, since the samples differ.
    pub fn add_at(&mut self, id: &str, usage: Usage, at: Option<&str>) -> bool {
        let mut changed = self.add(id, usage);
        if let Some(at) = at
            && self.stamps.get(id).map(String::as_str) != Some(at)
        {
            self.stamps.insert(id.to_owned(), at.to_owned());
            changed = true;
        }
        changed
    }

    /// The messages that have a timestamp, in no particular order.
    pub fn samples(&self) -> Vec<Sample> {
        self.stamps
            .iter()
            .filter_map(|(id, at)| {
                let u = self.seen.get(id)?;
                Some(Sample {
                    at: at.clone(),
                    input: u.input,
                    output: u.output,
                    cache_read: u.cache_read,
                    cache_creation: u.cache_creation,
                })
            })
            .collect()
    }

    /// Folds the usage an update carries (the main samples and the entries of subagents, as a
    /// follower of a remote machine receives them); `true` when the totals changed.
    pub fn apply(&mut self, update: &Update) -> bool {
        let mut changed = false;
        match update {
            Update::Main { usage, .. } => {
                for sample in usage {
                    changed |= self.add_at(&sample.id, sample.usage, sample.timestamp.as_deref());
                }
            }
            Update::Entries { entries, .. } => {
                for entry in entries {
                    if let (Some(id), Some(usage)) = (&entry.message_id, entry.usage) {
                        changed |= self.add_at(id, usage, entry.timestamp.as_deref());
                    }
                }
            }
            Update::List { .. } => {}
        }
        changed
    }

    /// [`apply`](Self::apply) for an update as JSON (what `agent.transcript` returns); anything
    /// that is not a usage-carrying update, or a count that is not a number, is skipped.
    pub fn apply_json(&mut self, update: &Value) -> bool {
        let usage_of = |v: &Value| -> Option<Usage> {
            let n = |k: &str| v.get(k).and_then(Value::as_u64);
            Some(Usage {
                input: n("input")?,
                output: n("output")?,
                cache_read: n("cache_read")?,
                cache_creation: n("cache_creation")?,
            })
        };
        let mut changed = false;
        match update.get("kind").and_then(Value::as_str) {
            Some("main") => {
                for sample in update
                    .get("usage")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let (Some(id), Some(u)) = (
                        sample.get("id").and_then(Value::as_str),
                        sample.get("usage").and_then(usage_of),
                    ) {
                        changed |=
                            self.add_at(id, u, sample.get("timestamp").and_then(Value::as_str));
                    }
                }
            }
            Some("entries") => {
                for entry in update
                    .get("entries")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let (Some(id), Some(u)) = (
                        entry.get("message_id").and_then(Value::as_str),
                        entry.get("usage").and_then(usage_of),
                    ) {
                        changed |=
                            self.add_at(id, u, entry.get("timestamp").and_then(Value::as_str));
                    }
                }
            }
            _ => {}
        }
        changed
    }

    pub fn totals(&self) -> Totals {
        self.seen.values().fold(Totals::default(), |t, u| Totals {
            input: t.input.saturating_add(u.input),
            output: t.output.saturating_add(u.output),
            cache_read: t.cache_read.saturating_add(u.cache_read),
            cache_creation: t.cache_creation.saturating_add(u.cache_creation),
        })
    }
}

/// Reads only the usage of a session: the main transcript and the subagent files for Claude,
/// `events.jsonl` for Copilot. Malformed lines are skipped.
pub struct UsageReader {
    main: Tailer,
    sub_dir: Option<PathBuf>,
    subagents: BTreeMap<PathBuf, Tailer>,
    copilot: bool,
    sum: UsageSum,
}

impl UsageReader {
    /// Claude Code: `project_dir` holds `<session>.jsonl` and `<session>/subagents`.
    pub fn claude(project_dir: &Path, session: &str) -> Option<Self> {
        if !is_safe_session_id(session) {
            return None;
        }
        Some(Self {
            main: Tailer::new(project_dir.join(format!("{session}.jsonl"))),
            sub_dir: Some(project_dir.join(session).join("subagents")),
            subagents: BTreeMap::new(),
            copilot: false,
            sum: UsageSum::default(),
        })
    }

    /// Copilot CLI: `session_dir` holds `events.jsonl`.
    pub fn copilot(session_dir: &Path) -> Self {
        Self {
            main: Tailer::new(session_dir.join(copilot::EVENTS)),
            sub_dir: None,
            subagents: BTreeMap::new(),
            copilot: true,
            sum: UsageSum::default(),
        }
    }

    /// The timestamped samples read so far.
    pub fn samples(&self) -> Vec<Sample> {
        self.sum.samples()
    }

    /// Reads what the files gained; the new totals when they changed.
    pub fn refresh(&mut self) -> Option<Totals> {
        let mut changed = false;
        for line in self.main.read_new() {
            changed |= self.line(&line);
        }
        if let Some(dir) = &self.sub_dir
            && let Ok(entries) = std::fs::read_dir(dir)
        {
            for path in entries.flatten().map(|e| e.path()) {
                let is_agent = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("agent-") && n.ends_with(".jsonl"));
                if is_agent {
                    self.subagents
                        .entry(path.clone())
                        .or_insert_with(|| Tailer::new(path));
                }
            }
        }
        let mut lines = Vec::new();
        for tailer in self.subagents.values_mut() {
            lines.extend(tailer.read_new());
        }
        for line in lines {
            changed |= self.line(&line);
        }
        changed.then(|| self.sum.totals())
    }

    fn line(&mut self, line: &str) -> bool {
        if self.copilot {
            return copilot::parse_usage_stamped(line)
                .is_some_and(|(id, usage, at)| self.sum.add_at(&id, usage, at.as_deref()));
        }
        line.contains("\"usage\"")
            && parse_usage_sample(line)
                .is_some_and(|s| self.sum.add_at(&s.id, s.usage, s.timestamp.as_deref()))
    }
}

/// Watches the session like [`watch`] but reads only its usage; `emit` gets the totals first
/// (when the files hold usage) and after every change.
pub fn watch_usage(
    mut reader: UsageReader,
    watch_root: &Path,
    relevant: impl Fn(&Path) -> bool + Send + 'static,
    emit: impl Fn(Totals, Vec<Sample>) + Send + 'static,
) -> Result<WatchHandle, String> {
    watch_dir(
        &watch_root.canonicalize().map_err(|e| e.to_string())?,
        relevant,
        move || {
            reader
                .refresh()
                .map(|t| (t, reader.samples()))
                .into_iter()
                .collect()
        },
        move |(t, samples)| emit(t, samples),
    )
}

/// Watches the usage of a Claude Code session in `project_dir`.
pub fn watch_claude_usage(
    project_dir: &Path,
    session: &str,
    emit: impl Fn(Totals, Vec<Sample>) + Send + 'static,
) -> Result<WatchHandle, String> {
    let dir = project_dir.canonicalize().map_err(|e| e.to_string())?;
    let reader = UsageReader::claude(&dir, session).ok_or("not a valid session id")?;
    let (file, session_dir) = (dir.join(format!("{session}.jsonl")), dir.join(session));
    watch_usage(
        reader,
        &dir,
        move |p| p == file || p.starts_with(&session_dir),
        emit,
    )
}

/// Why [`open`] found no transcript to follow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenError {
    /// Not an agent whose transcript can be read (or an unusable session id).
    NoAgent,
    /// The agent reported a session but its transcript file is not there (yet).
    Missing,
    /// The transcript is there but cannot be opened; carries the reason.
    Unreadable(String),
}

/// A followed session of either agent, read incrementally with [`Follow::refresh`].
pub enum Follow {
    Claude(Tracker),
    Copilot(copilot::Tracker),
}

impl Follow {
    /// What the files gained since the last call; empty when nothing changed.
    pub fn refresh(&mut self) -> Vec<Update> {
        match self {
            Follow::Claude(t) => t.refresh(),
            Follow::Copilot(t) => t.refresh(),
        }
    }
}

/// Locates the transcript of the `agent` (`claude` or `copilot`) session `session` below `home`
/// (`cwd` is the directory it ran in; Claude Code groups its transcripts by it) and prepares to
/// follow it. This is what a server does for the panes of its machine, with the files of its own
/// home.
pub fn open(home: &Path, agent: &str, session: &str, cwd: &str) -> Result<Follow, OpenError> {
    if !is_safe_session_id(session) {
        return Err(OpenError::NoAgent);
    }
    let (follow, file) = match agent {
        "claude" => {
            if cwd.is_empty() {
                return Err(OpenError::NoAgent);
            }
            let dir =
                locate_project_dir(&projects_root(home), cwd, session).ok_or(OpenError::Missing)?;
            let tracker = Tracker::new(&dir, session).ok_or(OpenError::NoAgent)?;
            (
                Follow::Claude(tracker),
                dir.join(format!("{session}.jsonl")),
            )
        }
        "copilot" => {
            let dir = copilot::locate_session_dir(&copilot::sessions_root(home), session)
                .ok_or(OpenError::Missing)?;
            let file = dir.join(copilot::EVENTS);
            (Follow::Copilot(copilot::Tracker::new(&dir)), file)
        }
        _ => return Err(OpenError::NoAgent),
    };
    File::open(&file).map_err(|e| OpenError::Unreadable(format!("{}: {e}", file.display())))?;
    Ok(follow)
}

#[cfg(test)]
mod tests;
