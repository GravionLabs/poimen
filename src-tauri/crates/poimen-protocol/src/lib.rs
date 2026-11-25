//! Wire types of the `poimen-server` socket API, documented in `docs/protocol.md`.
//!
//! NDJSON over a local socket (a Unix socket, a named pipe on Windows): one JSON object per line. A client sends [`Request`]s and reads one [`Response`] per request; after `events.subscribe` the connection also carries [`Event`]s. The crate has no I/O and no dependency on the app, so any client (the app, a CLI) can use it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

mod attach;
mod channel;
mod confirm;
pub mod files;
mod grant;
pub mod host;
pub mod lead;
pub mod report;
mod schedule;
pub mod status;
pub use attach::{AttachMode, AttachResult, DetachReason, TerminalSetDraft};
pub use channel::Channel;
pub use confirm::{
    AgentConfirm, AgentConfirmAnswer, AgentWriteAction, AgentWriteNotice, CONFIRM_WAIT_MS,
    ConfirmReason, ConfirmResult, Placement,
};
pub use files::windows_dir_from;
pub use grant::{
    AgentGrantAnswer, AgentGrantCheck, AgentGrantRequest, AgentGrantRevoke, AgentGrants,
    GRANT_DEFAULT_SECS, GRANT_MAX_SECS, GRANT_WAIT_MS, GrantInfo, GrantResult, GrantSpace,
    GrantVerdict,
};
pub use schedule::{ScheduleInfo, ScheduleList, ScheduleOutcome, ScheduleResult, ScheduleRun};
pub use status::{Hibernated, MAX_STATUS_HISTORY, StatusChange};

/// The protocol version of the server and this crate, raised on incompatible changes (`ping`).
pub const PROTOCOL_VERSION: u32 = 8;

/// Environment variable that overrides the socket path (and is set in every terminal).
pub const ENV_SOCKET: &str = "POIMEN_SOCKET";

/// The agent kinds with hooks, the one list of them on the Rust side: `poimen-server`'s
/// `integration::KINDS` (checked against it by a test) and the app's hook installer use it.
pub const HOOK_KINDS: [&str; 3] = ["claude", "copilot", "gemini"];

/// Environment variable set in every terminal: its own id.
pub const ENV_TERMINAL_ID: &str = "POIMEN_TERMINAL_ID";
/// Environment variable of `poimen-server integration install`: the absolute path the hooks call
/// instead of the running binary (inside an AppImage the app points it at its staged copy).
pub const ENV_HOOK_EXE: &str = "POIMEN_HOOK_EXE";

/// The channel of this build (`POIMEN_CHANNEL` at build time). An unknown value fails the build.
pub const CHANNEL: Channel = match Channel::parse(option_env!("POIMEN_CHANNEL")) {
    Some(channel) => channel,
    None => panic!("POIMEN_CHANNEL must be `stable` or `rc`"),
};

/// The directory under `%LOCALAPPDATA%` that holds the socket marker, `server.lock`,
/// `server.json` and the started server copies on Windows (not the install directory):
/// `poimen-server`, on rc `poimen-server-rc`.
pub fn windows_dir_name(channel: Channel) -> String {
    channel.named("poimen-server")
}

/// The socket this build's app and server use: `$POIMEN_SOCKET`, unless it is the default socket
/// of another channel (a terminal of the other installation passes it on to what it starts),
/// else the default of [`CHANNEL`] ([`SocketEnv::default_path`]). One server per user and channel.
pub fn socket_path() -> PathBuf {
    SocketEnv::from_process().path(CHANNEL)
}

/// The socket of the terminal this process runs in: `$POIMEN_SOCKET` as it is (whichever
/// channel's server owns the terminal), else [`socket_path`]. For commands run inside a terminal
/// (`poimen-server report`).
pub fn terminal_socket_path() -> PathBuf {
    let env = SocketEnv::from_process();
    env.override_path().unwrap_or_else(|| env.path(CHANNEL))
}

/// Whether `$POIMEN_SOCKET` names the default socket of another channel than [`CHANNEL`]: this
/// process runs in a terminal of the other installation, whose own hooks report for it.
pub fn foreign_socket() -> bool {
    SocketEnv::from_process().is_foreign(CHANNEL)
}

/// What the socket path is made of; empty values count as unset.
#[derive(Debug, Clone, Default)]
pub struct SocketEnv {
    /// `$POIMEN_SOCKET`.
    pub socket: Option<std::ffi::OsString>,
    /// Unix: `$XDG_RUNTIME_DIR`. Windows: `%LOCALAPPDATA%`.
    pub base: Option<std::ffi::OsString>,
    /// `$USER`, on Windows `%USERNAME%`.
    pub user: Option<String>,
    pub temp_dir: PathBuf,
    pub windows: bool,
}

impl SocketEnv {
    /// The environment of this process.
    pub fn from_process() -> Self {
        let windows = cfg!(windows);
        let (base, user) = if windows {
            ("LOCALAPPDATA", "USERNAME")
        } else {
            ("XDG_RUNTIME_DIR", "USER")
        };
        Self {
            socket: std::env::var_os(ENV_SOCKET),
            base: std::env::var_os(base),
            user: std::env::var(user).ok(),
            temp_dir: std::env::temp_dir(),
            windows,
        }
    }

    /// `$POIMEN_SOCKET`, when set and not empty.
    pub fn override_path(&self) -> Option<PathBuf> {
        self.socket
            .as_ref()
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    }

    /// The default socket of `channel`, ignoring `$POIMEN_SOCKET`. Unix:
    /// `$XDG_RUNTIME_DIR/poimen/server.sock`, else `<temp dir>/poimen-<user>/server.sock`.
    /// Windows: `%LOCALAPPDATA%\poimen-server\server.sock` (a marker file; the server listens on
    /// the named pipe of that path), else `<temp dir>\poimen-<user>\server.sock`. On rc the
    /// directories are `poimen-rc`, `poimen-server-rc` and `poimen-rc-<user>`.
    pub fn default_path(&self, channel: Channel) -> PathBuf {
        if self.windows {
            if let Some(dir) = windows_dir_from(self.base.clone(), channel) {
                return dir.join("server.sock");
            }
        } else if let Some(dir) = self.base.as_ref().filter(|s| !s.is_empty()) {
            return PathBuf::from(dir)
                .join(channel.named("poimen"))
                .join("server.sock");
        }
        self.temp_dir
            .join(user_dir_name(self.user.clone(), channel))
            .join("server.sock")
    }

    /// Whether `$POIMEN_SOCKET` is the default socket of another channel than `channel`.
    pub fn is_foreign(&self, channel: Channel) -> bool {
        self.override_path().is_some_and(|socket| {
            Channel::ALL
                .into_iter()
                .any(|other| other != channel && self.default_path(other) == socket)
        })
    }

    /// The socket of `channel`: `$POIMEN_SOCKET` unless it is [foreign](Self::is_foreign), else
    /// the [default](Self::default_path).
    pub fn path(&self, channel: Channel) -> PathBuf {
        match self.override_path() {
            Some(socket) if !self.is_foreign(channel) => socket,
            _ => self.default_path(channel),
        }
    }
}

fn user_dir_name(user: Option<String>, channel: Channel) -> String {
    let user = user
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| "user".into());
    format!("{}-{user}", channel.named("poimen"))
}

/// The effective user id of this process, read as the owner of a file created now (std has no
/// safe uid getter and this crate forbids `unsafe`).
#[cfg(unix)]
pub fn current_uid() -> std::io::Result<u32> {
    use std::os::unix::fs::MetadataExt;
    use std::sync::atomic::{AtomicU64, Ordering};
    // Threads of one process may probe within the same clock tick (macOS: microseconds).
    static PROBE: AtomicU64 = AtomicU64::new(0);
    let probe = std::env::temp_dir().join(format!(
        "poimen-uid-{}-{}-{}",
        std::process::id(),
        PROBE.fetch_add(1, Ordering::Relaxed),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    ));
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)?;
    let uid = file.metadata().map(|m| m.uid());
    drop(file);
    let _ = std::fs::remove_file(&probe);
    uid
}

/// Checks that `dir` is a real directory (not a symlink) owned by the current user with no
/// group/other permission bits. With `fix`, a directory we own but with loose bits is chmod 0700;
/// a symlink, a foreign owner or loose bits without `fix` give `PermissionDenied`. A missing
/// directory is `NotFound`.
#[cfg(unix)]
pub fn check_private_dir(dir: &std::path::Path, fix: bool) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let deny = |why: &str| {
        Error::new(
            ErrorKind::PermissionDenied,
            format!("{} {why}", dir.display()),
        )
    };
    let meta = std::fs::symlink_metadata(dir)?;
    if !meta.file_type().is_dir() {
        return Err(deny("is not a real directory"));
    }
    if meta.uid() != current_uid()? {
        return Err(deny("is owned by another user"));
    }
    if meta.mode() & 0o077 != 0 {
        if !fix {
            return Err(deny("is accessible by group or others"));
        }
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// A request line. `id` is echoed in the response; any string the client picks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// A response line: exactly one of `result` and `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// `None` when the request line could not be parsed far enough to know its id.
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorBody>,
}

impl Response {
    pub fn ok(id: &str, result: Value) -> Self {
        Self {
            id: Some(id.to_string()),
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Option<&str>, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            id: id.map(str::to_string),
            result: None,
            error: Some(ErrorBody {
                code,
                message: message.into(),
            }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    /// The line is not JSON.
    ParseError,
    /// JSON, but not a request (no `id` or `method`).
    InvalidRequest,
    UnknownMethod,
    /// The `params` do not fit the method.
    InvalidParams,
    /// A space or terminal id that does not exist.
    NotFound,
    /// The terminal exists but its process has exited.
    Exited,
    /// The shell or command could not be started.
    SpawnFailed,
    /// The request needs the controller of the terminal and this connection is not it (an
    /// observer's input or resize, a resize while another connection controls).
    NotController,
    /// The terminal already has a controller and the attach did not ask to take over.
    Conflict,
    Internal,
}

/// A line sent by the server: a response, or an event on a subscribed connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
// An event carries a whole `Terminal`; boxing it would change every reader for no gain.
#[allow(clippy::large_enum_variant)]
#[serde(untagged)]
pub enum ServerLine {
    Event(Event),
    Response(Response),
}

/// Parses one line of the server. A line with an `event` field is an [`Event`].
pub fn parse_server_line(line: &str) -> Result<ServerLine, serde_json::Error> {
    serde_json::from_str(line)
}

/// Parses one request line into a request, or the error response to send back.
pub fn parse_request(line: &str) -> Result<Request, Response> {
    let value: Value = serde_json::from_str(line)
        .map_err(|e| Response::err(None, ErrorCode::ParseError, e.to_string()))?;
    let id = value.get("id").and_then(Value::as_str).map(str::to_string);
    serde_json::from_value(value)
        .map_err(|e| Response::err(id.as_deref(), ErrorCode::InvalidRequest, e.to_string()))
}

/// Deserializes the `params` of a request; the error is the response to send back.
pub fn params<T: for<'de> Deserialize<'de>>(request: &Request) -> Result<T, Response> {
    let value = if request.params.is_null() {
        Value::Object(Default::default())
    } else {
        request.params.clone()
    };
    serde_json::from_value(value).map_err(|e| {
        Response::err(
            Some(&request.id),
            ErrorCode::InvalidParams,
            format!("{}: {e}", request.method),
        )
    })
}

// --- model -------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Space {
    pub id: String,
    pub name: String,
    pub cwd: String,
    /// The `tab_id`s of the space in order (empty from a protocol 3 server).
    #[serde(default)]
    pub tabs: Vec<String>,
    /// The focused tab, shared by every client (the last `tab.focus` wins); `None` without tabs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tab: Option<String>,
}

/// How a split arranges its two children: `right` puts the second one right of the first,
/// `down` below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    Right,
    Down,
}

/// The ratio a split gets when `tab.split` names none.
pub const DEFAULT_SPLIT_RATIO: f64 = 0.5;

/// Whether `ratio` is a share the first child of a split can take: above 0, below 1.
pub fn valid_ratio(ratio: f64) -> bool {
    ratio.is_finite() && ratio > 0.0 && ratio < 1.0
}

/// A node of a tab's split tree: a terminal, or a split of two nodes. Every leaf is a running
/// terminal and every running terminal is in exactly one leaf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Node {
    Terminal {
        terminal_id: String,
    },
    Split {
        /// Assigned by the server, unique within it and stable for the life of the split (also
        /// across restarts); `tab.resize_split` addresses the split by it.
        split_id: String,
        direction: SplitDirection,
        /// The share of the first child, above 0 and below 1.
        ratio: f64,
        children: Box<[Node; 2]>,
    },
}

impl Node {
    pub fn terminal(terminal_id: impl Into<String>) -> Self {
        Node::Terminal {
            terminal_id: terminal_id.into(),
        }
    }

    /// The terminals of the leaves, depth first (first child before second).
    pub fn terminal_ids(&self) -> Vec<&str> {
        let mut ids = Vec::new();
        self.walk(&mut |node| {
            if let Node::Terminal { terminal_id } = node {
                ids.push(terminal_id.as_str());
            }
        });
        ids
    }

    /// The `split_id`s of the tree, depth first.
    pub fn split_ids(&self) -> Vec<&str> {
        let mut ids = Vec::new();
        self.walk(&mut |node| {
            if let Node::Split { split_id, .. } = node {
                ids.push(split_id.as_str());
            }
        });
        ids
    }

    pub fn contains_terminal(&self, terminal_id: &str) -> bool {
        self.terminal_ids().contains(&terminal_id)
    }

    /// The first leaf (depth first).
    pub fn first_terminal(&self) -> &str {
        match self {
            Node::Terminal { terminal_id } => terminal_id,
            Node::Split { children, .. } => children[0].first_terminal(),
        }
    }

    fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Node)) {
        f(self);
        if let Node::Split { children, .. } = self {
            children[0].walk(f);
            children[1].walk(f);
        }
    }
}

/// A tab of a space: a name and a split tree of terminals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tab {
    pub tab_id: String,
    pub space_id: String,
    pub name: String,
    pub root: Node,
    /// The focused leaf of the tab, shared by every client (the last `tab.focus` wins).
    #[serde(default)]
    pub active_terminal: String,
    /// The tab this one belongs below (since protocol 7, set from the nesting of a layout's
    /// `children[]`); absent for a top-level tab. Closing the parent makes its children top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// App-only hint (#657, additive in protocol 7): the editor group this tab opens in. The server
    /// stores and passes it on; the TUI ignores it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<TabPlace>,
}

/// Where the app opens a tab: in an editor group split off the group of tab `of`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabPlace {
    pub direction: SplitDirection,
    /// A tab of the same space.
    pub of: String,
    /// The share the group of `of` keeps.
    pub ratio: f64,
}

/// Result of `space.list`: the spaces with the tabs of all of them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SpaceList {
    pub spaces: Vec<Space>,
    /// Absent from a protocol 3 server.
    #[serde(default)]
    pub tabs: Vec<Tab>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalStatus {
    Running,
    Exited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terminal {
    pub id: String,
    pub space_id: String,
    pub name: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub status: TerminalStatus,
    /// Set once `status` is `exited`; `None` when the process was killed by a signal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<u32>,
    /// The program the terminal runs (the shell by default); lets a client quote what it types. Absent from older servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// The arguments after `program`; lets a client start the same command again. Absent when none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// The agent in the terminal, as its hooks last reported it; absent when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentInfo>,
    /// The free-form tokens tools in the terminal reported (`terminal.report_tokens`); expired ones are gone.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, String>,
    /// The host survives a restart: `server.shutdown` with `keep_terminals` keeps it (ADR 0009). Absent (false) from older servers.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub survives_restart: bool,
    /// The version of the `poimen-pty-host` it runs in (can be older than `host_bundled_version`). Absent from servers before #606 and exited terminals.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_version: Option<String>,
    /// The version of the host this server ships with (a new terminal gets it); set with `host_version`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_bundled_version: Option<String>,
    /// The child records of OSC 7501 `id=` (#684), least recently updated first; `app` from the nearest ancestor. Absent when none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub records: Vec<ChildRecord>,
    /// The agent was stopped because it had been `done` for `agents.hibernateAfter` minutes
    /// (ADR 0014, #903); the shell stays. Absent when the terminal is not hibernated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hibernated: Option<Hibernated>,
    /// The terminal whose agent started this one with `start_agent` (#982); outlives the starter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_by: Option<String>,
    /// The agent-events run the terminal's session belongs to (#744), set from `TerminalCreate.run`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// The task of that run the session works on (#744, read by #745), from `TerminalCreate.task`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// What the agent in the terminal reported with `report_done` (#980); the last one wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub report: Option<report::AgentReport>,
    /// Where `agent` comes from: `osc7501` or the hook kind (#1153).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_source: Option<String>,
    /// The last status changes, newest last, in memory only (#1153).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub status_history: Vec<StatusChange>,
    /// Resident memory in bytes of the child of the terminal's host and its descendants (#971);
    /// only in `terminal.list`, read at most every 30 s. Absent where it cannot be read (Windows,
    /// an exited terminal, an older server).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
    /// The process is gone (a reboot, a vanished host) and what is left is the history rebuilt
    /// from the terminal's log on disk (#1195, ADR 0015): `status` is `exited`, `exit_code`
    /// absent, `terminal.attach` is read-only. Absent (false) otherwise and from older servers.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_history: bool,
}

/// What an agent in a terminal is doing (reported by hooks). `Gone` is only ever reported: the
/// agent ended, the terminal then has no [`AgentInfo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    /// The program failed (OSC 7501 `state=error`, #682). Older clients do not know it.
    Error,
    Gone,
}

/// The agent of a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub status: AgentStatus,
    /// Which agent, e.g. `claude`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The latest notification text of the agent (see [`clean_message`]); cleared when the agent
    /// turns `working` again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// When `message` was accepted, milliseconds since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_at: Option<u64>,
    /// Progress of the program, 0 to 100 (OSC 7501 `progress`, #682).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
    /// Why the agent is blocked: `permission`, `question` or `auth` (OSC 7501 `kind`, #682).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// `warning` for a `done` that wants a look (OSC 7501 `tone`, #1152); only with `done`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
    /// The program's own title (OSC 7501 `title`, #682), plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// When the last accepted `terminal.report_agent` came in, milliseconds since the epoch,
    /// also when the status did not change (#897).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_at: Option<u64>,
    /// When `status` last changed, milliseconds since the epoch (#897).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_since: Option<u64>,
    /// When the server marked the agent silent, milliseconds since the epoch: `working` without a
    /// report for `agents.silentAfter` minutes (#898). Set only by the server's sweeper, cleared
    /// by the next report; never present for another status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silent_since: Option<u64>,
}

/// A child record of a terminal: what a program reported with OSC 7501 and an `id` (a
/// `/`-separated path such as `run/issue-12`, #684). Everything but `id` and `status` is optional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChildRecord {
    pub id: String,
    pub status: AgentStatus,
    /// The reporting program; the nearest ancestor's when the record names none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// Most child records a terminal keeps; the least recently updated one goes first.
pub const MAX_CHILD_RECORDS: usize = 256;
/// Longest `id` of a child record, in bytes.
pub const MAX_RECORD_ID: usize = 128;
/// Most levels (segments) of an `id`.
pub const MAX_RECORD_DEPTH: usize = 8;

/// Whether `id` is a valid child record id: 1 to 8 segments split by `/`, each 1 to 32 of
/// `[A-Za-z0-9_.+-]`, at most 128 bytes in all.
pub fn valid_record_id(id: &str) -> bool {
    id.len() <= MAX_RECORD_ID
        && id.split('/').count() <= MAX_RECORD_DEPTH
        && id.split('/').all(|s| {
            (1..=32).contains(&s.len())
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-'))
        })
}

/// Kinds of a block an OSC 7501 report may name.
pub const BLOCK_KINDS: [&str; 3] = ["permission", "question", "auth"];
/// The values of the OSC 7501 key `tone` (#1152), kept only with `state=done`.
pub const TONES: [&str; 1] = ["warning"];

/// Bidirectional overrides and isolates, zero-width and other invisible formatting characters:
/// text from a report must not reorder or hide what the user reads (#682).
pub fn is_invisible_format(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}'
        | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}'
        | '\u{2060}'..='\u{206F}'
        | '\u{FEFF}'
        | '\u{FFF9}'..='\u{FFFB}'
        | '\u{E0000}'..='\u{E007F}')
}

/// Longest notification text, in characters.
pub const MAX_MESSAGE_CHARS: usize = 300;

/// A notification text as the protocol carries it: control characters (newlines, tabs included)
/// become a space, runs of whitespace collapse, the result is trimmed and cut to
/// [`MAX_MESSAGE_CHARS`] characters. `None` when nothing is left.
pub fn clean_message(text: &str) -> Option<String> {
    let flat: String = text
        .chars()
        .filter(|c| !is_invisible_format(*c))
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let joined = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(MAX_MESSAGE_CHARS).collect();
    let cut = cut.trim_end().to_owned();
    (!cut.is_empty()).then_some(cut)
}

/// Params of `server.shutdown`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerShutdown {
    /// Leave the terminals running in their hosts for the next server to adopt (a restart);
    /// `false` (the default, and what a client without the field gets) ends every terminal.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub keep_terminals: bool,
}

/// Params of `server.set_colors`: the colors the server answers terminal color queries with
/// while no controller is attached. `#rrggbb` strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetColors {
    pub foreground: String,
    pub background: String,
    /// Defaults to the foreground.
    #[serde(default)]
    pub cursor: Option<String>,
    /// The 16 ANSI colors; when absent the previous ones stay.
    #[serde(default)]
    pub palette: Option<Vec<String>>,
}

/// Params of `terminal.report_agent`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalReportAgent {
    pub terminal_id: String,
    /// Increases with every report of a reporter (e.g. nanoseconds since the epoch); a report
    /// whose `seq` is not above the last accepted one is dropped.
    pub seq: u64,
    pub status: AgentStatus,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Notification text of the agent; optional.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// `message` is a stand-in (a permission request's tool): a `blocked` agent keeps its text.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub message_fallback: bool,
}

/// Most tokens in one `terminal.report_tokens` (as herdr).
pub const MAX_TOKENS_PER_REPORT: usize = 16;
/// Longest token name (as herdr).
pub const MAX_TOKEN_NAME: usize = 32;

/// A token name is 1 to [`MAX_TOKEN_NAME`] ASCII letters, digits, `_` or `-` (as herdr).
pub fn valid_token_name(name: &str) -> bool {
    (1..=MAX_TOKEN_NAME).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Params of `terminal.report_tokens`: sets the tokens of `source` on the terminal; a `null`
/// value removes the token. `ttl_s` (above 0) expires the tokens set by this report after that
/// many seconds. A token is owned by the source that set it last.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalReportTokens {
    pub terminal_id: String,
    pub tokens: BTreeMap<String, Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ttl_s: Option<u64>,
    pub source: String,
}

/// Params of `terminal.clear_tokens`: removes every token `source` set on the terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalClearTokens {
    pub terminal_id: String,
    pub source: String,
}

// --- request params and results ----------------------------------------------------------------

/// Result of `ping`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingResult {
    pub version: String,
    pub protocol: u32,
    pub pid: u32,
    /// The kinds of the clients with an open event subscription, one entry per connection,
    /// sorted. Absent from older servers.
    #[serde(default)]
    pub clients: Vec<ClientKind>,
}

/// The outcome of one check of `machine.check`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    /// Something tasks need is missing.
    Missing,
    /// Something optional is missing (one of two agents, a hook integration).
    Warning,
}

/// One check of `machine.check`: `id` is `server`, `git`, `gh`, `gh_auth`, `claude`, `copilot`,
/// `hooks_claude` or `hooks_copilot`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineCheckItem {
    pub id: String,
    pub status: CheckStatus,
    /// What was found (a path, a version) or why it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The command to run on the machine to fix it; poimen never runs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

/// Result of `machine.check` (since protocol 6, additive).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MachineCheckResult {
    pub version: String,
    pub protocol: u32,
    pub checks: Vec<MachineCheckItem>,
}

impl MachineCheckResult {
    /// No check is `missing` (warnings do not count).
    pub fn ok(&self) -> bool {
        self.checks.iter().all(|c| c.status != CheckStatus::Missing)
    }
}

/// Params of `repo.ensure` (#282): the GitHub repository `owner/repo`, at its path under
/// `reposDir` of the server's user config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoEnsure {
    pub owner: String,
    pub repo: String,
}

/// Result of `repo.ensure`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoEnsureResult {
    /// The repository's path on the server's machine.
    pub path: String,
    /// `true`: cloned now; `false`: an existing clone, fetched.
    pub cloned: bool,
}

/// The issue of `worktree.create`; its fields fill the task mode's templates.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskIssue {
    pub nr: u64,
    pub title: String,
    #[serde(default)]
    pub url: String,
}

/// Params of `worktree.create` (#282): the worktree of `issue` for task mode `mode` of the
/// repository of `repo_dir`, as `task_worktree` of the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeCreate {
    pub repo_dir: String,
    pub issue: TaskIssue,
    pub mode: String,
}

/// Result of `worktree.create`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeCreateResult {
    pub path: String,
    pub branch: String,
    /// `false` when an existing worktree of the branch was returned.
    pub created: bool,
}

/// Params of `agent.transcript` (#698): the next updates of the agent transcript of a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTranscript {
    pub terminal_id: String,
    /// Names the reader: the server keeps one position in the transcript per `(terminal_id,
    /// follower)`, so two views of the same terminal each get every update once.
    pub follower: String,
    /// Starts the reader over, so the answer holds everything from the start of the transcript.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub restart: bool,
}

/// What `agent.transcript` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptState {
    /// The transcript is followed; `updates` holds what it gained.
    Ok,
    /// No agent with a readable transcript (or no session id) is reported in the terminal.
    NoAgent,
    /// The agent reported a session but its transcript file is not there (yet).
    Missing,
    /// The transcript is there but the server cannot read it; `reason` says why.
    Unreadable,
}

/// Result of `agent.transcript`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentTranscriptResult {
    pub state: TranscriptState,
    /// Why the transcript is `unreadable`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// The session the updates belong to (`ok` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    /// The reader for `(terminal_id, follower)` was created by this request (the first one, a
    /// `restart`, or after the server lost it): the updates then start at the transcript's start.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub fresh: bool,
    /// The updates since the last request, in the shape of the app's `subagents` event (`kind`
    /// `list`, `entries` or `main`, see "Agent transcripts" in docs/protocol.md).
    #[serde(default)]
    pub updates: Vec<Value>,
}

/// A merged pull request of `worktree.cleanup`: its head branch and number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergedPull {
    pub branch: String,
    pub number: u64,
}

/// Params of `worktree.cleanup` (#284): "Clean up merged tasks" in the clone of `owner/repo`
/// under `reposDir` of the server's user config. `merged` comes from the client's `gh`; only the
/// git work runs on the server's machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeCleanup {
    pub owner: String,
    pub repo: String,
    pub merged: Vec<MergedPull>,
    /// A space of this server that is never touched (the client's focused one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focused_space: Option<String>,
    /// Remove without the project's teardown when the project file is not trusted.
    #[serde(default)]
    pub skip_teardown: bool,
}

/// A worktree `worktree.cleanup` removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupRemoved {
    pub path: String,
    pub branch: String,
    pub pr: u64,
    pub branch_deleted: bool,
    /// Why the branch was kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// A worktree `worktree.cleanup` left, with the reason and the command to remove it by hand.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupLeft {
    pub path: String,
    pub branch: String,
    pub pr: u64,
    pub reason: String,
    pub command: String,
}

/// Result of `worktree.cleanup`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeCleanupResult {
    /// The clone on the server's machine; `None` when there is none (nothing to clean up).
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub removed: Vec<CleanupRemoved>,
    #[serde(default)]
    pub left: Vec<CleanupLeft>,
    /// The project file has a teardown but is not trusted on that machine: nothing was removed.
    #[serde(default)]
    pub teardown_untrusted: bool,
}

/// What a client is, said on `events.subscribe` (ADR 0006).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    App,
    Tui,
    Cli,
    /// A subscription without `client`, or a kind this version does not know.
    #[serde(other)]
    Unknown,
}

/// `client` of `events.subscribe`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClientInfo {
    pub kind: ClientKind,
    pub pid: u32,
}

/// Params of `events.subscribe`; `client` is optional so old clients keep working.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventsSubscribe {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<ClientInfo>,
    /// Asks for the progress events of the runs in the repositories of the server's spaces
    /// ([`Event::RunEvents`], #692): first the events of the recent runs from the start, then the
    /// new ones. Off by default, so a client that does not ask never sees the event.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub run_events: bool,
    /// The client shows the confirmations of agents' writes ([`Event::AgentConfirmRequested`],
    /// [`Event::AgentWrote`], #615): only a subscription of kind `app` with this set counts as
    /// an app that can be asked. A machine's link does not set it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub confirms: bool,
}

/// `space.create`. With `layout` (the default) the server resolves the layout of `cwd` (project
/// file, deprecated layout file, space template, `spaces.layout` of the user config, built-in
/// default), asks for trust in the project file when it has commands and builds the tabs and
/// splits itself; `layout: false` creates an empty space (the protocol 4 behavior).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceCreate {
    pub name: String,
    pub cwd: String,
    #[serde(default = "yes")]
    pub layout: bool,
    /// The user's answer to a `trust_required` result; absent on the first send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust: Option<TrustAnswer>,
    /// Replaces the command of the layout's root pane (a task's command); typed into its shell.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Extra environment, added after the server's own variables (`POIMEN_SOCKET`, `POIMEN_TERMINAL_ID` stay).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

fn yes() -> bool {
    true
}

impl SpaceCreate {
    /// A space without terminals.
    pub fn empty(name: impl Into<String>, cwd: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            cwd: cwd.into(),
            layout: false,
            trust: None,
            command: None,
            env: BTreeMap::new(),
        }
    }

    /// A space with the resolved layout, without a trust answer yet.
    pub fn with_layout(name: impl Into<String>, cwd: impl Into<String>) -> Self {
        Self {
            layout: true,
            ..Self::empty(name, cwd)
        }
    }
}

/// The answer to a [`TrustRequired`]: `{"kind": "trust", "hash": ...}` runs the project
/// commands (valid only for that content hash, which the server then stores in
/// `trustedProjects`), `{"kind": "without"}` builds the panes without them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrustAnswer {
    Trust { hash: String },
    Without,
}

/// One command of a project file, for the trust prompt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrustCommand {
    /// Where it comes from (`setup`, `layout: pane "agent"`, ...).
    pub what: String,
    pub command: String,
}

/// A pane of the layout a space would get.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutPane {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Absent for the root pane.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split: Option<LayoutSplit>,
}

/// How a pane of a layout splits an earlier one (`from`, by name).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayoutSplit {
    pub from: String,
    pub direction: SplitDirection,
    /// The share the split pane keeps.
    pub ratio: f64,
}

/// `trust_required` of a `space.create` result: the project file has commands that are not
/// trusted. Nothing was created; the client asks the user and sends `space.create` again with
/// `trust`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrustRequired {
    /// The repository root the trust is stored for.
    pub root: String,
    /// The files read, for display.
    pub files: Vec<String>,
    /// SHA-256 of the files' content, the `hash` of a `trust` answer.
    pub hash: String,
    pub commands: Vec<TrustCommand>,
    /// The layout that would be built with the commands; absent when the project layout is
    /// invalid (the space then gets one pane).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layout: Option<Vec<LayoutPane>>,
}

/// A `space.create` that created the space.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpaceCreated {
    pub space: Space,
    /// The tabs of the new space in order (empty for `layout: false`).
    #[serde(default)]
    pub tabs: Vec<Tab>,
    /// Problems that did not stop the space (an invalid project layout, a template without its
    /// agent, a pane that could not be split).
    #[serde(default)]
    pub warnings: Vec<String>,
    /// Whether the `setup` of the project file was typed into its `setup` tab.
    #[serde(default)]
    pub setup_started: bool,
}

/// Result of `space.create`: the space, or the trust question (nothing created).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SpaceCreateResult {
    TrustRequired { trust_required: TrustRequired },
    Created(SpaceCreated),
}

/// `space.apply_layout` (protocol 6): builds a layout into an existing space as new tabs, after
/// the same trust handshake as `space.create`. Without `preset` the layout is the one the server
/// resolves for `cwd` like `space.create` does (rebuilding a space whose terminals are gone),
/// with the project's `setup` tab; with `preset` it is that preset of the project file (one new
/// tab, no setup).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceApplyLayout {
    pub space_id: String,
    /// Defaults to the space's `cwd`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// The name of a preset of the project file; absent: the resolved layout of the space.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// The user's answer to a `trust_required` result; absent on the first send.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trust: Option<TrustAnswer>,
    /// Extra environment, added after the server's own variables (`POIMEN_SOCKET`, `POIMEN_TERMINAL_ID` stay).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

impl SpaceApplyLayout {
    /// The resolved layout of the space, in its own directory, without a trust answer yet.
    pub fn resolved(space_id: impl Into<String>) -> Self {
        Self {
            space_id: space_id.into(),
            cwd: None,
            preset: None,
            trust: None,
            env: BTreeMap::new(),
        }
    }

    /// The preset `name` of the project file, without a trust answer yet.
    pub fn preset(space_id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            preset: Some(name.into()),
            ..Self::resolved(space_id)
        }
    }
}

/// Result of `space.apply_layout`: the space with the tabs it built (only the new ones, in
/// order), or the trust question (nothing created).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SpaceApplyLayoutResult {
    TrustRequired { trust_required: TrustRequired },
    Applied(SpaceCreated),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceRename {
    pub space_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpaceClose {
    pub space_id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalList {
    /// Only the terminals of this space.
    #[serde(default)]
    pub space_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCreate {
    pub space_id: String,
    /// Defaults to the cwd of the space.
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    /// Program and arguments, never through a shell; defaults to the user's `$SHELL`.
    #[serde(default)]
    pub command: Option<Vec<String>>,
    #[serde(default)]
    pub cols: Option<u16>,
    #[serde(default)]
    pub rows: Option<u16>,
    /// Extra environment, added after the server's own variables (`POIMEN_SOCKET`, `POIMEN_TERMINAL_ID` stay).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    /// Becomes `Terminal.started_by` (#982).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_by: Option<String>,
    /// Becomes `Terminal.run` (#744): the agent-events run of a scripted session.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    /// Becomes `Terminal.task` (#744): the task of that run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<String>,
    /// Where the client wants the terminal's tab (#744, ADR 0010). Reserved: the server accepts
    /// it and keeps no placement yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<Placement>,
}

/// `tab.create`: a new tab at the end of the space, with `terminal_id` (moved out of its tab) or
/// a new terminal (the space's cwd, the default shell).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabCreate {
    pub space_id: String,
    /// Defaults to the name of the terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
    /// Extra environment, added after the server's own variables (`POIMEN_SOCKET`, `POIMEN_TERMINAL_ID` stay).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabRename {
    pub tab_id: String,
    pub name: String,
}

/// `tab.close`: closes the tab and its terminals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabClose {
    pub tab_id: String,
}

/// `tab.move`: moves the tab to `index` of its space's order (past the end: the last place).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabMove {
    pub tab_id: String,
    pub index: usize,
}

/// `tab.split`: creates a terminal and splits the leaf of `terminal_id` into the old leaf (first
/// child) and the new terminal (second child).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabSplit {
    pub terminal_id: String,
    pub direction: SplitDirection,
    /// The share of the old leaf; [`DEFAULT_SPLIT_RATIO`] when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratio: Option<f64>,
    /// Defaults to the cwd of the split terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Program and arguments, never through a shell; defaults to the user's `$SHELL`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// Extra environment, added after the server's own variables (`POIMEN_SOCKET`, `POIMEN_TERMINAL_ID` stay).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
}

/// `tab.resize_split`: sets the ratio of one split; an unknown `split_id` is `not_found`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TabResizeSplit {
    pub tab_id: String,
    pub split_id: String,
    pub ratio: f64,
}

/// `tab.focus`: makes the tab the space's active tab and, with `terminal_id`, that leaf the
/// tab's active terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TabFocus {
    pub tab_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRename {
    pub terminal_id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalClose {
    pub terminal_id: String,
}

/// `terminal.detach`: ends the attachment of this connection to `terminal_id`; the server
/// answers `{"detached": true}` and then closes the connection. For clients that cannot shut
/// a connection down themselves (named pipes on Windows).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDetach {
    pub terminal_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalInput {
    pub terminal_id: String,
    /// Written to the terminal as is (UTF-8).
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalResize {
    pub terminal_id: String,
    pub cols: u16,
    pub rows: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalAttach {
    pub terminal_id: String,
    pub mode: AttachMode,
    /// With `control`: replace the current controller (which gets `terminal.detached` with
    /// reason `taken_over`) instead of failing with `conflict`.
    #[serde(default)]
    pub take_over: bool,
}

/// Encodes bytes for the wire (standard base64).
pub fn encode_bytes(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(data)
}

/// Decodes [`encode_bytes`].
pub fn decode_bytes(data: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.decode(data)
}

// --- events ------------------------------------------------------------------------------------

/// An event line: `{"event": "<name>", "data": {...}}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum Event {
    #[serde(rename = "space.created")]
    SpaceCreated { space: Space },
    #[serde(rename = "space.renamed")]
    SpaceRenamed { space: Space },
    #[serde(rename = "space.closed")]
    SpaceClosed { space_id: String },
    /// The order of the space's tabs or its `active_tab` changed.
    #[serde(rename = "space.changed")]
    SpaceChanged { space: Space },
    #[serde(rename = "tab.created")]
    TabCreated { tab: Tab },
    /// The tab after any change of its tree, name, place in the order or focus.
    #[serde(rename = "tab.changed")]
    TabChanged { tab: Tab },
    #[serde(rename = "tab.closed")]
    TabClosed { tab_id: String, space_id: String },
    #[serde(rename = "terminal.created")]
    TerminalCreated { terminal: Terminal },
    #[serde(rename = "terminal.renamed")]
    TerminalRenamed { terminal: Terminal },
    #[serde(rename = "terminal.closed")]
    TerminalClosed {
        terminal_id: String,
        space_id: String,
    },
    #[serde(rename = "terminal.exited")]
    TerminalExited {
        terminal_id: String,
        exit_code: Option<u32>,
    },
    /// Stream record of an attached connection: raw PTY bytes (base64).
    #[serde(rename = "terminal.output")]
    TerminalOutput {
        terminal_id: String,
        seq: u64,
        data: String,
    },
    /// Stream record of an attached connection: the controller changed the size.
    #[serde(rename = "terminal.resized")]
    TerminalResized {
        terminal_id: String,
        seq: u64,
        cols: u16,
        rows: u16,
    },
    /// Stream record: the server ended this attachment (the process did not exit).
    #[serde(rename = "terminal.detached")]
    TerminalDetached {
        terminal_id: String,
        reason: DetachReason,
    },
    /// A controller attached (subscribers).
    #[serde(rename = "terminal.controller_attached")]
    TerminalControllerAttached { terminal_id: String },
    /// The controller detached or was dropped (subscribers).
    #[serde(rename = "terminal.controller_detached")]
    TerminalControllerDetached { terminal_id: String },
    /// The tokens of a terminal changed (reported, cleared or expired); carries all of them.
    #[serde(rename = "terminal.tokens_changed")]
    TerminalTokensChanged {
        terminal_id: String,
        tokens: BTreeMap<String, String>,
    },
    /// A subscribed client connected or went away (subscribers, except the one that joined);
    /// carries the kinds now connected, sorted.
    #[serde(rename = "server.clients_changed")]
    ServerClientsChanged { clients: Vec<ClientKind> },
    /// The server started or stopped holding a sleep inhibitor (`agents.keepAwake`, #987).
    #[serde(rename = "server.awake_changed")]
    ServerAwakeChanged { awake: bool },
    /// Progress events of a run (agent-events, jsonl-file binding) in the repository at `root`,
    /// as written to `<git-common-dir>/.agents/events/<run>.jsonl`: complete lines, JSON objects,
    /// in file order and unchanged. Only to subscriptions with `run_events`.
    #[serde(rename = "run.events")]
    RunEvents {
        root: String,
        run: String,
        events: Vec<Value>,
    },
    /// An agent asks to write into another space (#615); only to subscriptions of kind `app`.
    /// Answered by `agent.confirm_answer`; gone with [`Event::AgentConfirmResolved`].
    #[serde(rename = "agent.confirm_requested")]
    AgentConfirmRequested {
        id: String,
        caller_terminal_id: String,
        caller_name: String,
        action: AgentWriteAction,
        space_id: String,
        space_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal_id: Option<String>,
        text: String,
        /// How long the request stays open.
        wait_ms: u64,
    },
    /// The confirmation `id` is settled (answered, timed out or abandoned).
    #[serde(rename = "agent.confirm_resolved")]
    AgentConfirmResolved { id: String },
    /// A lead agent asks for a scoped write grant (ADR 0018, #1211); only to subscriptions of kind
    /// `app`. Answered by `agent.grant_answer`; gone with [`Event::AgentGrantResolved`].
    #[serde(rename = "agent.grant_requested")]
    AgentGrantRequested {
        id: String,
        lead_terminal_id: String,
        lead_name: String,
        plan: String,
        spaces: Vec<GrantSpace>,
        tools: Vec<AgentWriteAction>,
        max_agents: u32,
        /// How long the grant lasts once confirmed.
        duration_s: u64,
        /// How long the request stays open.
        wait_ms: u64,
    },
    /// The grant request `id` is settled (answered, timed out or abandoned).
    #[serde(rename = "agent.grant_resolved")]
    AgentGrantResolved { id: String },
    /// The active grants changed (given, narrowed, revoked, expired or ended with the lead);
    /// carries all of them. Only to subscriptions of kind `app`.
    #[serde(rename = "agent.grants_changed")]
    AgentGrantsChanged { grants: Vec<GrantInfo> },
    /// An agent wrote through `poimen mcp` (#615); only to subscriptions of kind `app`.
    #[serde(rename = "agent.wrote")]
    AgentWrote {
        caller_terminal_id: String,
        caller_name: String,
        action: AgentWriteAction,
        space_id: String,
        space_name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal_id: Option<String>,
        text: String,
        confirmed: bool,
    },
    #[serde(rename = "agent.status_changed")]
    AgentStatusChanged {
        terminal_id: String,
        status: AgentStatus,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_at: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        progress: Option<u8>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        kind: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tone: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        title: Option<String>,
        /// Set while the server marks the agent silent (#898).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        silent_since: Option<u64>,
    },
    /// The child records of a terminal changed (#684); `records` is the whole list.
    #[serde(rename = "terminal.records_changed")]
    TerminalRecordsChanged {
        terminal_id: String,
        records: Vec<ChildRecord>,
    },
    /// A terminal's agent was hibernated, or the hibernation ended (`hibernated` absent), #903.
    #[serde(rename = "terminal.hibernated_changed")]
    TerminalHibernatedChanged {
        terminal_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        hibernated: Option<Hibernated>,
    },
    /// Resuming a hibernated agent failed (#904): the hibernation ended and the shell stays.
    #[serde(rename = "terminal.resume_failed")]
    TerminalResumeFailed { terminal_id: String, reason: String },
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "private_dir_tests.rs"]
mod private_dir_tests;
