//! Minimal client for the herdr socket API (newline-delimited JSON over a Unix socket; on Windows
//! herdr serves the same protocol on the named pipe `\\.\pipe\<socket path>`).
//!
//! Requests open a short-lived connection; subscriptions keep theirs open.
//! See <https://herdr.dev> (docs/socket-api) for the protocol.

use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Protocol(String),
    Api { code: String, message: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => write!(f, "herdr socket: {err}"),
            Error::Protocol(msg) => write!(f, "herdr protocol: {msg}"),
            Error::Api { code, message } => write!(f, "herdr {code}: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

/// Resolves the socket path like the herdr CLI does (minus the explicit `--session` flag).
pub fn socket_path_from(env: impl Fn(&str) -> Option<String>) -> PathBuf {
    if let Some(path) = env("HERDR_SOCKET_PATH").filter(|p| !p.is_empty()) {
        return PathBuf::from(path);
    }
    let base = if cfg!(windows) {
        PathBuf::from(env("APPDATA").unwrap_or_default()).join("herdr")
    } else {
        PathBuf::from(env("HOME").unwrap_or_default()).join(".config/herdr")
    };
    match env("HERDR_SESSION").filter(|s| !s.is_empty() && s != "default") {
        Some(session) => base.join("sessions").join(session).join("herdr.sock"),
        None => base.join("herdr.sock"),
    }
}

pub fn request_line(id: &str, method: &str, params: &Value) -> String {
    let mut line = json!({ "id": id, "method": method, "params": params }).to_string();
    line.push('\n');
    line
}

fn api_error(error: &Value) -> Error {
    Error::Api {
        code: error["code"].as_str().unwrap_or("unknown").to_owned(),
        message: error["message"].as_str().unwrap_or_default().to_owned(),
    }
}

/// Parses one response line: `{"id", "result"}` or `{"id", "error": {"code", "message"}}`.
pub fn parse_response(line: &str) -> Result<Value, Error> {
    let value: Value = serde_json::from_str(line)
        .map_err(|err| Error::Protocol(format!("invalid json: {err}")))?;
    if let Some(error) = value.get("error") {
        return Err(api_error(error));
    }
    value
        .get("result")
        .cloned()
        .ok_or_else(|| Error::Protocol("response without result".into()))
}

/// A pushed event: `{"event": "<kind>", "data": {...}}`.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct Event {
    pub kind: String,
    pub data: Value,
}

pub fn parse_event(line: &str) -> Result<Event, Error> {
    let value: Value = serde_json::from_str(line)
        .map_err(|err| Error::Protocol(format!("invalid json: {err}")))?;
    if let Some(error) = value.get("error") {
        // e.g. events_lost: the server closes the subscription after this line.
        return Err(api_error(error));
    }
    let kind = value["event"]
        .as_str()
        .ok_or_else(|| Error::Protocol("event without kind".into()))?
        .to_owned();
    Ok(Event {
        kind,
        data: value["data"].clone(),
    })
}

/// Whether anything answers on the socket.
#[derive(Debug, PartialEq, Eq)]
pub enum Probe {
    /// Nothing listens: the socket file is missing or the connection is refused (`NotFound`,
    /// `ConnectionRefused`). Only this state allows starting a server.
    Down,
    /// Something is bound to the socket but did not answer `ping` (server still starting, busy,
    /// closed the connection, or answered with an error). Never start a second server for it.
    Unresponsive,
    /// A server answered `ping`; the info is `None` when it reported no version.
    Up(Option<ServerInfo>),
}

#[derive(Debug, Clone)]
pub struct Client {
    socket: PathBuf,
}

/// The herdr protocol version poimen is written against.
pub const SUPPORTED_PROTOCOL: u32 = 22;

/// Version and protocol of the running herdr, from the `ping` reply.
#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct ServerInfo {
    pub version: String,
    /// `None` when the server reports a version but no protocol: shown, but not checked.
    pub protocol: Option<u32>,
    /// The protocol poimen supports, so the frontend can flag a mismatch.
    pub supported: u32,
}

/// Reads `{"type":"pong","version":"0.9.5","protocol":22}`; `None` when the server reports no version.
pub fn parse_ping(result: &Value) -> Option<ServerInfo> {
    Some(ServerInfo {
        version: result.get("version")?.as_str()?.to_string(),
        protocol: result
            .get("protocol")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok()),
        supported: SUPPORTED_PROTOCOL,
    })
}

/// Where `pane.split` puts the new pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Right,
    Down,
}

impl SplitDirection {
    fn as_str(self) -> &'static str {
        match self {
            SplitDirection::Right => "right",
            SplitDirection::Down => "down",
        }
    }
}

fn non_empty(cwd: Option<&str>) -> Option<&str> {
    cwd.map(str::trim).filter(|c| !c.is_empty())
}

/// Params of `pane.split`: splits `target`, optionally starting the new shell in `cwd`.
pub fn split_params(target: &str, direction: SplitDirection, cwd: Option<&str>) -> Value {
    let mut params = json!({ "target_pane_id": target, "direction": direction.as_str() });
    if let Some(cwd) = non_empty(cwd) {
        params["cwd"] = json!(cwd);
    }
    params
}

/// Params of `pane.split` with the share (`ratio`, 0 to 1) the split pane keeps.
pub fn split_params_ratio(
    target: &str,
    direction: SplitDirection,
    cwd: Option<&str>,
    ratio: Option<f64>,
) -> Value {
    let mut params = split_params(target, direction, cwd);
    if let Some(ratio) = ratio {
        params["ratio"] = json!(ratio);
    }
    params
}

/// Params of `pane.send_input`: types `text` into a pane and presses Enter.
pub fn send_input_params(pane_id: &str, text: &str) -> Value {
    json!({ "pane_id": pane_id, "text": text, "keys": ["enter"] })
}

/// Params of `pane.close`.
pub fn close_params(pane_id: &str) -> Value {
    json!({ "pane_id": pane_id })
}

/// Params of `agent.start`: starts `kind` in an existing pane that sits at a shell prompt. The name
/// is required by herdr, so an empty one falls back to the kind; `args` are split on whitespace.
pub fn agent_start_params(pane_id: &str, kind: &str, name: Option<&str>, args: &str) -> Value {
    let kind = kind.trim();
    let name = non_empty(name).unwrap_or(kind);
    let args: Vec<&str> = args.split_whitespace().collect();
    let mut params = json!({ "pane_id": pane_id, "kind": kind, "name": name });
    if !args.is_empty() {
        params["args"] = json!(args);
    }
    params
}

/// Params of `agent.rename`; a blank name clears the agent name (`null`).
pub fn agent_rename_params(target: &str, name: Option<&str>) -> Value {
    json!({ "target": target, "name": non_empty(name) })
}

/// Params of `pane.rename`; a blank label clears the pane label (`null`).
pub fn pane_rename_params(pane_id: &str, label: Option<&str>) -> Value {
    json!({ "pane_id": pane_id, "label": non_empty(label) })
}

/// Params of `tab.create`: a new tab (with one terminal pane) in a workspace, optionally in `cwd`.
pub fn tab_create_params(workspace_id: &str, cwd: Option<&str>) -> Value {
    let mut params = json!({ "workspace_id": workspace_id });
    if let Some(cwd) = non_empty(cwd) {
        params["cwd"] = json!(cwd);
    }
    params
}

/// The pane a `pane.split` (`pane_info`) or `tab.create` (`tab_created`) result created.
pub fn created_pane_id(result: &Value) -> Option<String> {
    let pane = result.get("pane").or_else(|| result.get("root_pane"))?;
    Some(pane.get("pane_id")?.as_str()?.to_owned())
}

/// The id of the headless action that prints a plugin's data for a native view (ADR 0004). It is
/// not meant to be run by a person, so it is hidden from the Plugins menu and the palette.
pub const DATA_ACTION_ID: &str = "data";

/// An action a herdr plugin offers (`plugin.action.list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginAction {
    pub plugin_id: String,
    pub action_id: String,
    pub title: String,
    pub description: Option<String>,
}

/// Reads `{"type":"plugin_action_list","actions":[...]}`; entries without ids or title are skipped,
/// and so is the `data` action of the native-view contract ([`DATA_ACTION_ID`]).
pub fn parse_plugin_actions(result: &Value) -> Vec<PluginAction> {
    let Some(actions) = result.get("actions").and_then(Value::as_array) else {
        return Vec::new();
    };
    actions
        .iter()
        .filter_map(|a| {
            let text = |key: &str| a.get(key)?.as_str().map(str::to_owned);
            let action_id = text("action_id")?;
            if action_id == DATA_ACTION_ID {
                return None;
            }
            Some(PluginAction {
                plugin_id: text("plugin_id")?,
                action_id,
                title: text("title")?,
                description: text("description"),
            })
        })
        .collect()
}

/// What an invoked plugin action learns about where it was started (`PluginInvocationContext`).
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct PluginContext {
    pub focused_pane_id: Option<String>,
    pub focused_pane_cwd: Option<String>,
    pub focused_pane_agent: Option<String>,
    pub tab_id: Option<String>,
    pub tab_label: Option<String>,
    pub workspace_id: Option<String>,
    pub workspace_label: Option<String>,
    /// The URL a link handler action starts for (`clicked_url`, `HERDR_PLUGIN_CLICKED_URL`).
    pub clicked_url: Option<String>,
}

/// Params of `plugin.action.invoke`; the context lists only the fields that are set.
pub fn plugin_invoke_params(plugin_id: &str, action_id: &str, context: &PluginContext) -> Value {
    let mut ctx = serde_json::Map::new();
    ctx.insert("invocation_source".into(), json!("poimen"));
    let fields = [
        ("focused_pane_id", &context.focused_pane_id),
        ("focused_pane_cwd", &context.focused_pane_cwd),
        ("focused_pane_agent", &context.focused_pane_agent),
        ("tab_id", &context.tab_id),
        ("tab_label", &context.tab_label),
        ("workspace_id", &context.workspace_id),
        ("workspace_label", &context.workspace_label),
        ("clicked_url", &context.clicked_url),
    ];
    for (key, value) in fields {
        if let Some(value) = non_empty(value.as_deref()) {
            ctx.insert(key.into(), json!(value));
        }
    }
    json!({ "plugin_id": plugin_id, "action_id": action_id, "context": ctx })
}

/// A pane a plugin declares (`panes[]` of `plugin.list`), which `plugin.pane.open` can start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginPaneDef {
    pub plugin_id: String,
    pub plugin_name: String,
    pub entrypoint: String,
    pub title: String,
}

/// Reads `{"type":"plugin_list","plugins":[...]}` into the panes the enabled plugins declare;
/// disabled plugins and entries without id or title are skipped. The plugin name falls back to
/// its id.
pub fn parse_plugin_pane_defs(result: &Value) -> Vec<PluginPaneDef> {
    let Some(plugins) = result.get("plugins").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut defs = Vec::new();
    for plugin in plugins {
        let Some(plugin_id) = plugin.get("plugin_id").and_then(Value::as_str) else {
            continue;
        };
        if plugin.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let name = plugin
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(plugin_id);
        let panes = plugin.get("panes").and_then(Value::as_array);
        for pane in panes.into_iter().flatten() {
            let (Some(entrypoint), Some(title)) = (
                pane.get("id").and_then(Value::as_str),
                pane.get("title").and_then(Value::as_str),
            ) else {
                continue;
            };
            defs.push(PluginPaneDef {
                plugin_id: plugin_id.to_owned(),
                plugin_name: name.to_owned(),
                entrypoint: entrypoint.to_owned(),
                title: title.to_owned(),
            });
        }
    }
    defs
}

/// An agent whose session transcript poimen can read (`agent` of `pane.list`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptAgent {
    /// Claude Code: `~/.claude/projects/<cwd>/<session>.jsonl`.
    Claude,
    /// GitHub Copilot CLI: `~/.copilot/session-state/<session>/events.jsonl`.
    Copilot,
}

/// The agent session of a pane (`agent` and `agent_session.value` of `pane.list`) and the
/// directory the session was started in; the transcript files are found from these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSession {
    pub agent: TranscriptAgent,
    pub session: String,
    /// Empty when herdr reports none; only Claude Code needs it.
    pub cwd: String,
}

/// A session id is used as a file or directory name below the agent's transcript root; only
/// `[A-Za-z0-9_-]+` is accepted, so a value reported by a pane cannot escape that directory.
pub fn is_safe_session_id(session: &str) -> bool {
    !session.is_empty()
        && session
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Reads the agent session of `pane_id` out of a `pane.list` result; `None` for another pane, an
/// agent without a readable transcript (`codex`, ...), or a pane without a session id. Claude
/// Code also needs the pane's `cwd` (its transcripts are grouped by directory).
pub fn parse_agent_session(result: &Value, pane_id: &str) -> Option<AgentSession> {
    let pane = result
        .get("panes")?
        .as_array()?
        .iter()
        .find(|p| p.get("pane_id").and_then(Value::as_str) == Some(pane_id))?;
    let agent = match pane.get("agent")?.as_str()? {
        "claude" => TranscriptAgent::Claude,
        "copilot" => TranscriptAgent::Copilot,
        _ => return None,
    };
    let session = pane.get("agent_session")?.get("value")?.as_str()?;
    let cwd = pane.get("cwd").and_then(Value::as_str).unwrap_or_default();
    let needs_cwd = agent == TranscriptAgent::Claude;
    (is_safe_session_id(session) && !(needs_cwd && cwd.is_empty())).then(|| AgentSession {
        agent,
        session: session.to_owned(),
        cwd: cwd.to_owned(),
    })
}

/// A plugin pane herdr reports (`plugin_pane_opened`, `plugin_pane_focused`, `plugin_pane_closed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PluginPane {
    pub plugin_id: String,
    pub entrypoint: String,
    pub pane_id: String,
}

/// Reads `{"type":"plugin_pane_opened","plugin_pane":{plugin_id,entrypoint,pane:{pane_id,..}}}`
/// (the same shape for focused and closed).
pub fn parse_plugin_pane(result: &Value) -> Option<PluginPane> {
    let info = result.get("plugin_pane")?;
    let text = |v: Option<&Value>| v?.as_str().map(str::to_owned);
    Some(PluginPane {
        plugin_id: text(info.get("plugin_id"))?,
        entrypoint: text(info.get("entrypoint"))?,
        pane_id: text(info.get("pane")?.get("pane_id"))?,
    })
}

/// Params of `plugin.pane.open`: the pane opens as a herdr tab (`placement: tab`) and takes focus,
/// so poimen can show it as an editor tab.
pub fn plugin_pane_open_params(
    plugin_id: &str,
    entrypoint: &str,
    workspace_id: Option<&str>,
) -> Value {
    let mut params = json!({
        "plugin_id": plugin_id,
        "entrypoint": entrypoint,
        "placement": "tab",
        "focus": true,
    });
    if let Some(id) = non_empty(workspace_id) {
        params["workspace_id"] = json!(id);
    }
    params
}

/// The connection to the API socket: a Unix socket, on Windows the named pipe herdr listens on.
#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;
#[cfg(windows)]
type Stream = std::fs::File;

/// The named pipe herdr on Windows serves for the socket path `socket`.
#[cfg(any(windows, test))]
pub fn pipe_name(socket: &Path) -> String {
    format!(r"\\.\pipe\{}", socket.display())
}

#[cfg(unix)]
fn open_stream(socket: &Path) -> io::Result<Stream> {
    Stream::connect(socket)
}

/// Opens the pipe, retrying briefly while every pipe instance is busy (`ERROR_PIPE_BUSY`).
#[cfg(windows)]
fn open_stream(socket: &Path) -> io::Result<Stream> {
    const ERROR_PIPE_BUSY: i32 = 231;
    let pipe = pipe_name(socket);
    let mut attempts = 0;
    loop {
        match std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&pipe)
        {
            Err(err) if err.raw_os_error() == Some(ERROR_PIPE_BUSY) && attempts < 20 => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            result => return result,
        }
    }
}

/// Sends one `ping` on `stream` and reads the answer.
fn ping_on(mut stream: Stream) -> Result<Value, Error> {
    stream.write_all(request_line(&next_id(), "ping", &json!({})).as_bytes())?;
    let mut line = String::new();
    match BufReader::new(stream).read_line(&mut line)? {
        0 => Err(Error::Protocol("connection closed before response".into())),
        _ => parse_response(&line),
    }
}

#[cfg(unix)]
fn ping_within(socket: &Path, timeout: std::time::Duration) -> Result<Value, Error> {
    let stream = open_stream(socket)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    ping_on(stream)
}

/// A pipe handle has no read timeout, so the ping runs on a helper thread and the caller stops
/// waiting after `timeout` (the thread ends once the server answers or closes the pipe).
#[cfg(windows)]
fn ping_within(socket: &Path, timeout: std::time::Duration) -> Result<Value, Error> {
    let stream = open_stream(socket)?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(ping_on(stream));
    });
    rx.recv_timeout(timeout)
        .unwrap_or_else(|_| Err(Error::Io(io::ErrorKind::TimedOut.into())))
}

/// Read/write timeout of one `Client::probe`.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(1500);

/// `Down` only when nothing listens (socket file missing, connection refused); every other
/// failure means something may be bound to the socket, so it is `Unresponsive`.
pub fn classify_probe_error(err: &Error) -> Probe {
    match err {
        Error::Io(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            Probe::Down
        }
        _ => Probe::Unresponsive,
    }
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> String {
    format!("poimen_{}", NEXT_ID.fetch_add(1, Ordering::Relaxed))
}

impl Client {
    pub fn from_env() -> Self {
        Self {
            socket: socket_path_from(|key| std::env::var(key).ok()),
        }
    }

    fn connect(&self, method: &str, params: &Value) -> Result<BufReader<Stream>, Error> {
        let mut stream = open_stream(&self.socket)?;
        stream.write_all(request_line(&next_id(), method, params).as_bytes())?;
        Ok(BufReader::new(stream))
    }

    pub fn request(&self, method: &str, params: Value) -> Result<Value, Error> {
        let mut reader = self.connect(method, &params)?;
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(Error::Protocol("connection closed before response".into()));
        }
        parse_response(&line)
    }

    /// The running herdr's version, or `None` when it is unreachable or too old to say.
    pub fn info(&self) -> Option<ServerInfo> {
        parse_ping(&self.request("ping", json!({})).ok()?)
    }

    /// Asks the socket once whether a server answers (`ping`). Never starts anything. Reads and
    /// writes time out after `PROBE_TIMEOUT`, so a socket that accepts but never answers cannot
    /// hold the caller.
    pub fn probe(&self) -> Probe {
        match ping_within(&self.socket, PROBE_TIMEOUT) {
            Ok(result) => Probe::Up(parse_ping(&result)),
            Err(err) => classify_probe_error(&err),
        }
    }

    pub fn subscribe(&self, subscriptions: Vec<Value>) -> Result<Subscription, Error> {
        let mut reader = self.connect(
            "events.subscribe",
            &json!({ "subscriptions": subscriptions }),
        )?;
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Err(Error::Protocol(
                "connection closed before subscription ack".into(),
            ));
        }
        parse_response(&line)?;
        Ok(Subscription { reader })
    }
}

pub struct Subscription {
    reader: BufReader<Stream>,
}

impl Subscription {
    /// Blocks until the next event. Errors when the server closes or reports `events_lost`.
    pub fn next_event(&mut self) -> Result<Event, Error> {
        let mut line = String::new();
        loop {
            line.clear();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(Error::Protocol("subscription closed".into()));
            }
            if !line.trim().is_empty() {
                return parse_event(&line);
            }
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn agent_session_is_read_from_the_pane_list() {
        let result = json!({"panes": [
            {"pane_id": "w1:p1", "agent": "claude", "cwd": "/home/u/proj",
             "agent_session": {"agent": "claude", "kind": "id", "value": "abc-123"}},
            {"pane_id": "w1:p2", "agent": "codex", "cwd": "/x",
             "agent_session": {"value": "zzz"}},
            {"pane_id": "w1:p3", "cwd": "/x"},
            {"pane_id": "w1:p4", "agent": "claude", "cwd": "/x"},
            {"pane_id": "w1:p5", "agent": "copilot", "cwd": "/home/u/proj",
             "agent_session": {"source": "herdr:copilot", "kind": "id", "value": "0b7e-c0"}},
            {"pane_id": "w1:p6", "agent": "copilot",
             "agent_session": {"source": "herdr:copilot", "kind": "id", "value": "0b7e-c1"}},
            {"pane_id": "w1:p7", "agent": "claude",
             "agent_session": {"value": "abc-124"}}
        ]});
        assert_eq!(
            parse_agent_session(&result, "w1:p1"),
            Some(AgentSession {
                agent: TranscriptAgent::Claude,
                session: "abc-123".into(),
                cwd: "/home/u/proj".into()
            })
        );
        assert_eq!(
            parse_agent_session(&result, "w1:p5"),
            Some(AgentSession {
                agent: TranscriptAgent::Copilot,
                session: "0b7e-c0".into(),
                cwd: "/home/u/proj".into()
            })
        );
        // Copilot sessions are found by id alone; Claude needs the directory.
        assert_eq!(
            parse_agent_session(&result, "w1:p6").map(|s| (s.agent, s.cwd)),
            Some((TranscriptAgent::Copilot, String::new()))
        );
        assert_eq!(parse_agent_session(&result, "w1:p7"), None);
        assert_eq!(parse_agent_session(&result, "w1:p2"), None);
        assert_eq!(parse_agent_session(&result, "w1:p3"), None);
        assert_eq!(parse_agent_session(&result, "w1:p4"), None);
        assert_eq!(parse_agent_session(&result, "w9:p9"), None);
        assert_eq!(parse_agent_session(&json!({}), "w1:p1"), None);
    }

    #[test]
    fn agent_session_ids_must_be_plain_file_names() {
        let with = |v: &str| {
            json!({"panes": [{"pane_id": "p", "agent": "claude", "cwd": "/x",
                "agent_session": {"value": v}}]})
        };
        for bad in ["../x", "/abs", "", "a/b", "..", "a.b"] {
            assert_eq!(parse_agent_session(&with(bad), "p"), None, "{bad}");
        }
        let uuid = "0b7e1c52-3f4a-4d2e-9a61-5c8d7e2f1a90";
        assert_eq!(parse_agent_session(&with(uuid), "p").unwrap().session, uuid);
    }

    #[test]
    fn plugin_actions_are_parsed_and_incomplete_ones_skipped() {
        let result = json!({"type": "plugin_action_list", "actions": [
            {"plugin_id": "link-trim", "action_id": "open", "title": "Open link", "command": ["node"]},
            {"plugin_id": "x", "action_id": "y", "title": "Y", "description": "why"},
            {"plugin_id": "broken"}
        ]});
        let actions = parse_plugin_actions(&result);
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].plugin_id, "link-trim");
        assert_eq!(actions[0].description, None);
        assert_eq!(actions[1].description.as_deref(), Some("why"));
        assert!(parse_plugin_actions(&json!({})).is_empty());
    }

    #[test]
    fn plugin_pane_defs_come_from_enabled_plugins_with_a_title() {
        let result = json!({"type": "plugin_list", "plugins": [
            {"plugin_id": "deliver-board", "name": "Deliver Board", "enabled": true, "panes": [
                {"id": "board", "title": "Deliver board", "placement": "popup", "command": ["node"]},
                {"id": "broken"}
            ]},
            {"plugin_id": "review-popup", "panes": [{"id": "review", "title": "Review"}]},
            {"plugin_id": "off", "enabled": false, "panes": [{"id": "x", "title": "X"}]},
            {"plugin_id": "agent-notify", "events": []}
        ]});
        let defs = parse_plugin_pane_defs(&result);
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].plugin_name, "Deliver Board");
        assert_eq!(defs[0].entrypoint, "board");
        assert_eq!(defs[1].plugin_name, "review-popup");
        assert!(parse_plugin_pane_defs(&json!({})).is_empty());
    }

    #[test]
    fn plugin_pane_results_are_parsed_for_open_focus_and_close() {
        for kind in [
            "plugin_pane_opened",
            "plugin_pane_focused",
            "plugin_pane_closed",
        ] {
            let result = json!({"type": kind, "plugin_pane": {
                "plugin_id": "deliver-board", "entrypoint": "board",
                "pane": {"pane_id": "w1:p4", "terminal_id": "t", "workspace_id": "w1", "tab_id": "w1:t2"}
            }});
            assert_eq!(
                parse_plugin_pane(&result),
                Some(PluginPane {
                    plugin_id: "deliver-board".into(),
                    entrypoint: "board".into(),
                    pane_id: "w1:p4".into(),
                })
            );
        }
        assert_eq!(parse_plugin_pane(&json!({"type": "ok"})), None);
        assert_eq!(
            parse_plugin_pane(&json!({"plugin_pane": {"plugin_id": "x"}})),
            None
        );
    }

    #[test]
    fn plugin_pane_open_params_ask_for_a_focused_tab() {
        assert_eq!(
            plugin_pane_open_params("review-popup", "review", None),
            json!({"plugin_id": "review-popup", "entrypoint": "review", "placement": "tab", "focus": true})
        );
        assert_eq!(
            plugin_pane_open_params("review-popup", "review", Some("w2"))["workspace_id"],
            "w2"
        );
        assert!(plugin_pane_open_params("a", "b", Some(" "))["workspace_id"].is_null());
    }

    #[test]
    fn the_data_action_is_hidden_from_the_runnable_actions() {
        let result = json!({"type": "plugin_action_list", "actions": [
            {"plugin_id": "deliver-board", "action_id": "open", "title": "Open the deliver board"},
            {"plugin_id": "deliver-board", "action_id": "data", "title": "Deliver board data (JSON)"}
        ]});
        let actions = parse_plugin_actions(&result);
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].action_id, "open");
    }

    #[test]
    fn plugin_invoke_params_carry_the_clicked_url() {
        let ctx = PluginContext {
            focused_pane_cwd: Some("/r".into()),
            clicked_url: Some("https://github.com/o/r/issues/7".into()),
            ..Default::default()
        };
        let params = plugin_invoke_params("deliver-starter", "start-link", &ctx);
        assert_eq!(
            params["context"]["clicked_url"],
            "https://github.com/o/r/issues/7"
        );
        assert_eq!(params["context"]["focused_pane_cwd"], "/r");
    }

    #[test]
    fn plugin_invoke_params_carry_the_set_context_fields() {
        let ctx = PluginContext {
            focused_pane_id: Some("p_1".into()),
            workspace_id: Some("w_1".into()),
            tab_label: Some("  ".into()),
            ..Default::default()
        };
        assert_eq!(
            plugin_invoke_params("review-popup", "open", &ctx),
            json!({"plugin_id": "review-popup", "action_id": "open", "context": {
                "invocation_source": "poimen", "focused_pane_id": "p_1", "workspace_id": "w_1"
            }})
        );
    }
    use super::*;

    fn io_err(kind: io::ErrorKind) -> Error {
        Error::Io(io::Error::from(kind))
    }

    #[test]
    fn only_a_missing_or_refusing_socket_is_down() {
        assert_eq!(
            classify_probe_error(&io_err(io::ErrorKind::NotFound)),
            Probe::Down
        );
        assert_eq!(
            classify_probe_error(&io_err(io::ErrorKind::ConnectionRefused)),
            Probe::Down
        );
    }

    #[test]
    fn any_other_failure_is_unresponsive() {
        for kind in [
            io::ErrorKind::WouldBlock,
            io::ErrorKind::TimedOut,
            io::ErrorKind::PermissionDenied,
        ] {
            assert_eq!(classify_probe_error(&io_err(kind)), Probe::Unresponsive);
        }
        assert_eq!(
            classify_probe_error(&Error::Protocol("closed".into())),
            Probe::Unresponsive
        );
        let api = Error::Api {
            code: "busy".into(),
            message: "starting".into(),
        };
        assert_eq!(classify_probe_error(&api), Probe::Unresponsive);
    }

    fn probe_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-probe-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn probing_a_missing_socket_is_down() {
        let dir = probe_dir("missing");
        let client = Client {
            socket: dir.join("none.sock"),
        };
        assert_eq!(client.probe(), Probe::Down);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn probing_a_socket_that_never_answers_times_out_as_unresponsive() {
        let dir = probe_dir("silent");
        let path = dir.join("s.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let client = Client { socket: path };
        assert_eq!(client.probe(), Probe::Unresponsive);
        drop(listener);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(unix)]
    #[test]
    fn probing_a_socket_that_answers_is_up() {
        let dir = probe_dir("up");
        let path = dir.join("s.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream);
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            let id = serde_json::from_str::<Value>(&line).unwrap()["id"].clone();
            let reply = json!({"id": id, "result": {"type": "pong", "version": "0.9.5"}});
            writeln!(reader.get_mut(), "{reply}").unwrap();
        });
        let client = Client { socket: path };
        assert!(matches!(client.probe(), Probe::Up(_)));
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(dir);
    }

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_owned())
        }
    }

    #[cfg(windows)]
    #[test]
    fn socket_path_defaults_to_appdata_on_windows() {
        let path = socket_path_from(env(&[("APPDATA", r"C:\Users\u\AppData\Roaming")]));
        assert_eq!(
            path,
            PathBuf::from(r"C:\Users\u\AppData\Roaming\herdr\herdr.sock")
        );
    }

    #[test]
    fn pipe_name_prefixes_the_socket_path() {
        let name = pipe_name(Path::new(r"C:\Users\u\AppData\Roaming\herdr\herdr.sock"));
        assert_eq!(
            name,
            r"\\.\pipe\C:\Users\u\AppData\Roaming\herdr\herdr.sock"
        );
    }

    #[cfg(unix)]
    #[test]
    fn socket_path_defaults_to_config_dir() {
        let path = socket_path_from(env(&[("HOME", "/home/u")]));
        assert_eq!(path, PathBuf::from("/home/u/.config/herdr/herdr.sock"));
    }

    #[cfg(unix)]
    #[test]
    fn socket_path_uses_named_session() {
        let path = socket_path_from(env(&[("HOME", "/home/u"), ("HERDR_SESSION", "work")]));
        assert_eq!(
            path,
            PathBuf::from("/home/u/.config/herdr/sessions/work/herdr.sock")
        );
    }

    #[test]
    fn socket_path_override_wins() {
        let path = socket_path_from(env(&[
            ("HOME", "/home/u"),
            ("HERDR_SESSION", "work"),
            ("HERDR_SOCKET_PATH", "/tmp/x.sock"),
        ]));
        assert_eq!(path, PathBuf::from("/tmp/x.sock"));
    }

    #[test]
    fn request_line_is_one_json_line() {
        let line = request_line("r1", "pane.list", &json!({}));
        assert!(line.ends_with('\n'));
        let value: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(
            value,
            json!({ "id": "r1", "method": "pane.list", "params": {} })
        );
    }

    #[test]
    fn parse_response_returns_result() {
        let result = parse_response(r#"{"id":"r1","result":{"type":"pong"}}"#).unwrap();
        assert_eq!(result, json!({ "type": "pong" }));
    }

    #[test]
    fn parse_response_maps_api_error() {
        let err = parse_response(r#"{"id":"r1","error":{"code":"not_found","message":"no pane"}}"#)
            .unwrap_err();
        assert!(matches!(err, Error::Api { ref code, .. } if code == "not_found"));
    }

    #[test]
    fn parse_event_reads_envelope() {
        let event = parse_event(
            r#"{"event":"pane.agent_status_changed","data":{"pane_id":"w1:p1","agent_status":"blocked"}}"#,
        )
        .unwrap();
        assert_eq!(event.kind, "pane.agent_status_changed");
        assert_eq!(event.data["agent_status"], "blocked");
    }

    #[test]
    fn parse_event_surfaces_events_lost() {
        let err = parse_event(r#"{"id":"sub","error":{"code":"events_lost","message":"behind"}}"#)
            .unwrap_err();
        assert!(matches!(err, Error::Api { ref code, .. } if code == "events_lost"));
    }

    #[test]
    fn parse_ping_reads_version_and_protocol() {
        let pong = json!({"type":"pong","version":"0.9.5","protocol":22,"capabilities":{}});
        assert_eq!(
            parse_ping(&pong),
            Some(ServerInfo {
                version: "0.9.5".into(),
                protocol: Some(22),
                supported: SUPPORTED_PROTOCOL
            })
        );
    }

    #[test]
    fn split_params_name_target_direction_and_cwd() {
        assert_eq!(
            split_params("w1:p1", SplitDirection::Right, Some("/tmp/x")),
            json!({ "target_pane_id": "w1:p1", "direction": "right", "cwd": "/tmp/x" })
        );
        assert_eq!(
            split_params("w1:p1", SplitDirection::Down, None),
            json!({ "target_pane_id": "w1:p1", "direction": "down" })
        );
    }

    #[test]
    fn blank_cwd_is_left_out() {
        assert_eq!(
            split_params("p", SplitDirection::Down, Some("  ")),
            json!({ "target_pane_id": "p", "direction": "down" })
        );
        assert_eq!(
            tab_create_params("w1", Some("")),
            json!({ "workspace_id": "w1" })
        );
    }

    #[test]
    fn tab_create_params_carry_workspace_and_cwd() {
        assert_eq!(
            tab_create_params("w1", Some(" /work ")),
            json!({ "workspace_id": "w1", "cwd": "/work" })
        );
    }

    #[test]
    fn agent_start_params_carry_pane_kind_name_and_args() {
        assert_eq!(
            agent_start_params("w1:p1", "claude", Some(" helper "), " --model  opus "),
            json!({ "pane_id": "w1:p1", "kind": "claude", "name": "helper", "args": ["--model", "opus"] })
        );
    }

    #[test]
    fn agent_start_params_default_the_name_to_the_kind_and_skip_empty_args() {
        assert_eq!(
            agent_start_params("w1:p1", "codex", Some(" "), "  "),
            json!({ "pane_id": "w1:p1", "kind": "codex", "name": "codex" })
        );
        assert_eq!(
            agent_start_params("w1:p1", "codex", None, ""),
            json!({ "pane_id": "w1:p1", "kind": "codex", "name": "codex" })
        );
    }

    #[test]
    fn rename_params_clear_with_null_when_blank() {
        assert_eq!(
            agent_rename_params("w1:p2", Some(" review ")),
            json!({ "target": "w1:p2", "name": "review" })
        );
        assert_eq!(
            agent_rename_params("w1:p2", Some("")),
            json!({ "target": "w1:p2", "name": null })
        );
        assert_eq!(
            pane_rename_params("w1:p1", Some("build")),
            json!({ "pane_id": "w1:p1", "label": "build" })
        );
        assert_eq!(
            pane_rename_params("w1:p1", None),
            json!({ "pane_id": "w1:p1", "label": null })
        );
    }

    #[test]
    fn split_ratio_and_send_input_params() {
        assert_eq!(
            split_params_ratio("w1:p1", SplitDirection::Right, Some("/a"), Some(0.6)),
            json!({ "target_pane_id": "w1:p1", "direction": "right", "cwd": "/a", "ratio": 0.6 })
        );
        assert_eq!(
            split_params_ratio("w1:p1", SplitDirection::Down, None, None),
            split_params("w1:p1", SplitDirection::Down, None)
        );
        assert_eq!(
            send_input_params("w1:p1", "claude"),
            json!({ "pane_id": "w1:p1", "text": "claude", "keys": ["enter"] })
        );
    }

    #[test]
    fn close_params_name_the_pane() {
        assert_eq!(close_params("w1:p2"), json!({ "pane_id": "w1:p2" }));
    }

    #[test]
    fn split_direction_parses_from_the_ipc_payload() {
        let right: SplitDirection = serde_json::from_str("\"right\"").unwrap();
        assert_eq!(right, SplitDirection::Right);
        assert!(serde_json::from_str::<SplitDirection>("\"left\"").is_err());
    }

    #[test]
    fn created_pane_id_reads_split_and_tab_results() {
        let split = json!({"type":"pane_info","pane":{"pane_id":"w1:p3"}});
        let tab =
            json!({"type":"tab_created","tab":{"tab_id":"w1:t2"},"root_pane":{"pane_id":"w1:p4"}});
        assert_eq!(created_pane_id(&split).as_deref(), Some("w1:p3"));
        assert_eq!(created_pane_id(&tab).as_deref(), Some("w1:p4"));
        assert_eq!(created_pane_id(&json!({"type":"ok"})), None);
    }

    #[test]
    fn parse_ping_is_none_for_an_old_server() {
        assert_eq!(parse_ping(&json!({"type":"pong"})), None);
        assert_eq!(parse_ping(&json!({"protocol":22})), None);
    }

    #[test]
    fn parse_ping_keeps_the_version_without_a_protocol() {
        assert_eq!(
            parse_ping(&json!({"type":"pong","version":"0.1"})),
            Some(ServerInfo {
                version: "0.1".into(),
                protocol: None,
                supported: SUPPORTED_PROTOCOL
            })
        );
    }
}
