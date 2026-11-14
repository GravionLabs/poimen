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
    },
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
        if usage_changed || (&self.calls, tools) != (&self.sent_main.0, self.sent_main.1) {
            self.sent_main = (self.calls.clone(), tools);
            updates.push(Update::Main {
                calls: self.calls.clone(),
                tools,
                usage: self.usage.clone(),
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
pub(crate) fn watch_dir(
    dir: &Path,
    relevant: impl Fn(&Path) -> bool + Send + 'static,
    mut refresh: impl FnMut() -> Vec<Update> + Send + 'static,
    emit: impl Fn(Update) + Send + 'static,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn temp_dir(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "poimen-transcripts-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
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

    fn session_fixture(name: &str) -> (PathBuf, PathBuf) {
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
}
