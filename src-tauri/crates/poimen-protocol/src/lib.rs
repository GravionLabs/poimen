//! Wire types of the `poimen-server` socket API, documented in `docs/protocol.md`.
//!
//! NDJSON over a local socket (a Unix socket, a named pipe on Windows): one JSON object per line. A client sends [`Request`]s and reads one
//! [`Response`] per request; after `events.subscribe` the connection also carries [`Event`]s. The
//! crate has no I/O and no dependency on the app, so any client (the app, a CLI) can use it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub mod host;

/// The protocol version the server speaks and this crate is written for. Raised on every
/// incompatible change; `ping` reports it.
pub const PROTOCOL_VERSION: u32 = 6;

/// Environment variable that overrides the socket path (and is set in every terminal).
pub const ENV_SOCKET: &str = "POIMEN_SOCKET";
/// Environment variable set in every terminal: its own id.
pub const ENV_TERMINAL_ID: &str = "POIMEN_TERMINAL_ID";
/// Environment variable of `poimen-server integration install`: the absolute path the hooks
/// call instead of the running binary. The app sets it when it runs a copy whose path is not
/// stable (inside an AppImage), pointing at the stable link of its staged copy.
pub const ENV_HOOK_EXE: &str = "POIMEN_HOOK_EXE";

/// The release channel a binary is built for, fixed at build time by `POIMEN_CHANNEL` (`rc` for
/// the pre-release builds of `release.yml`, unset or `stable` otherwise). Every path a stable
/// and an rc installation could share is named per channel, so the two run side by side, each
/// with its own server, settings and hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Rc,
}

impl Channel {
    pub const ALL: [Channel; 2] = [Channel::Stable, Channel::Rc];

    /// The channel of a `POIMEN_CHANNEL` value: unset, empty or `stable` is stable, `rc` is rc,
    /// anything else `None`.
    pub const fn parse(value: Option<&str>) -> Option<Channel> {
        match value {
            None => Some(Channel::Stable),
            Some(v) if v.is_empty() || const_eq(v, "stable") => Some(Channel::Stable),
            Some(v) if const_eq(v, "rc") => Some(Channel::Rc),
            Some(_) => None,
        }
    }

    /// `stable` or `rc`.
    pub const fn name(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Rc => "rc",
        }
    }

    /// What names of this channel end with: nothing on stable, `-rc` on rc.
    pub const fn suffix(self) -> &'static str {
        match self {
            Channel::Stable => "",
            Channel::Rc => "-rc",
        }
    }

    /// `base` with the channel suffix: `poimen` stays `poimen` on stable and is `poimen-rc` on rc.
    pub fn named(self, base: &str) -> String {
        format!("{base}{}", self.suffix())
    }

    /// The file name of this channel's `poimen-server` binary (`poimen-server-rc` on rc; `.exe`
    /// on Windows).
    pub fn server_program(self, windows: bool) -> String {
        let name = self.named("poimen-server");
        if windows { format!("{name}.exe") } else { name }
    }
}

const fn const_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

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

/// `%LOCALAPPDATA%\poimen-server` (on rc `poimen-server-rc`) when `LOCALAPPDATA` is set and not
/// empty.
pub fn windows_dir_from(
    local_app_data: Option<std::ffi::OsString>,
    channel: Channel,
) -> Option<PathBuf> {
    local_app_data
        .filter(|s| !s.is_empty())
        .map(|dir| PathBuf::from(dir).join(windows_dir_name(channel)))
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
    /// The program the terminal runs (the shell, for a default terminal); lets a client quote
    /// what it types for that shell. Absent from servers that do not report it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// The agent in the terminal, as its hooks last reported it; absent when there is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentInfo>,
    /// The free-form tokens tools in the terminal reported (`terminal.report_tokens`); expired
    /// ones are gone. Empty when there are none.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, String>,
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
}

/// Longest notification text, in characters.
pub const MAX_MESSAGE_CHARS: usize = 300;

/// A notification text as the protocol carries it: control characters (newlines, tabs included)
/// become a space, runs of whitespace collapse, the result is trimmed and cut to
/// [`MAX_MESSAGE_CHARS`] characters. `None` when nothing is left.
pub fn clean_message(text: &str) -> Option<String> {
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let joined = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = joined.chars().take(MAX_MESSAGE_CHARS).collect();
    let cut = cut.trim_end().to_owned();
    (!cut.is_empty()).then_some(cut)
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
}

impl SpaceApplyLayout {
    /// The resolved layout of the space, in its own directory, without a trust answer yet.
    pub fn resolved(space_id: impl Into<String>) -> Self {
        Self {
            space_id: space_id.into(),
            cwd: None,
            preset: None,
            trust: None,
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

/// How a connection attaches to a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttachMode {
    /// Receives the output and size changes; cannot send input or resize.
    Observe,
    /// The one connection that sends input and sets the size.
    Control,
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

/// Result of `terminal.attach`. The records of the stream have a `seq` greater than `seq` here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachResult {
    pub terminal_id: String,
    pub seq: u64,
    pub cols: u16,
    pub rows: u16,
    pub mode: AttachMode,
    /// Base64 of the bytes that bring a fresh emulator of `cols` x `rows` to the current state
    /// (screen, scrollback, modes, cursor, title). Write it to the emulator as is.
    pub snapshot: String,
}

/// Why the server ended an attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetachReason {
    /// The client did not keep up; attach again for a fresh snapshot.
    Overflow,
    /// Another connection took over the control.
    TakenOver,
    /// The terminal was closed.
    Closed,
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
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn subscribe_params_with_and_without_client() {
        let r = parse_request(
            r#"{"id":"1","method":"events.subscribe","params":{"client":{"kind":"tui","pid":7}}}"#,
        )
        .unwrap();
        let p: EventsSubscribe = params(&r).unwrap();
        assert_eq!(
            p.client,
            Some(ClientInfo {
                kind: ClientKind::Tui,
                pid: 7
            })
        );
        let r = parse_request(r#"{"id":"1","method":"events.subscribe"}"#).unwrap();
        let p: EventsSubscribe = params(&r).unwrap();
        assert_eq!(p.client, None);
        let r = parse_request(
            r#"{"id":"1","method":"events.subscribe","params":{"client":{"kind":"bogus","pid":1}}}"#,
        )
        .unwrap();
        // A kind of a newer client is read as unknown, not refused.
        let p: EventsSubscribe = params(&r).unwrap();
        assert_eq!(p.client.unwrap().kind, ClientKind::Unknown);
    }

    #[test]
    fn ping_result_without_clients_and_the_clients_event() {
        let p: PingResult =
            serde_json::from_value(json!({"version":"1","protocol":4,"pid":2})).unwrap();
        assert!(p.clients.is_empty());
        let e: Event = serde_json::from_value(
            json!({"event":"server.clients_changed","data":{"clients":["app","unknown"]}}),
        )
        .unwrap();
        assert_eq!(
            e,
            Event::ServerClientsChanged {
                clients: vec![ClientKind::App, ClientKind::Unknown]
            }
        );
    }

    #[test]
    fn parses_a_request_with_and_without_params() {
        let r =
            parse_request(r#"{"id":"1","method":"space.create","params":{"name":"a","cwd":"/x"}}"#)
                .unwrap();
        assert_eq!(r.method, "space.create");
        let p: SpaceCreate = params(&r).unwrap();
        assert_eq!(p.cwd, "/x");
        assert!(
            p.layout,
            "space.create builds the layout unless told not to"
        );
        assert_eq!(p.trust, None);

        let r = parse_request(r#"{"id":"2","method":"ping"}"#).unwrap();
        assert_eq!(r.params, Value::Null);
    }

    #[test]
    fn rejects_garbage_with_parse_error_and_no_id() {
        let e = parse_request("not json").unwrap_err();
        assert_eq!(e.id, None);
        assert_eq!(e.error.unwrap().code, ErrorCode::ParseError);
    }

    #[test]
    fn rejects_a_request_without_method_but_keeps_its_id() {
        let e = parse_request(r#"{"id":"7"}"#).unwrap_err();
        assert_eq!(e.id.as_deref(), Some("7"));
        assert_eq!(e.error.unwrap().code, ErrorCode::InvalidRequest);
    }

    #[test]
    fn wrong_params_are_invalid_params() {
        let r =
            parse_request(r#"{"id":"3","method":"terminal.resize","params":{"terminal_id":"t"}}"#)
                .unwrap();
        let e = params::<TerminalResize>(&r).unwrap_err();
        assert_eq!(e.id.as_deref(), Some("3"));
        assert_eq!(e.error.unwrap().code, ErrorCode::InvalidParams);
    }

    #[test]
    fn optional_params_default() {
        let r = parse_request(r#"{"id":"4","method":"terminal.list"}"#).unwrap();
        assert_eq!(params::<TerminalList>(&r).unwrap().space_id, None);
    }

    #[test]
    fn event_wire_shape() {
        let e = Event::TerminalExited {
            terminal_id: "t1".into(),
            exit_code: Some(3),
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"event":"terminal.exited","data":{"terminal_id":"t1","exit_code":3}})
        );
        let status: Event = serde_json::from_value(
            json!({"event":"agent.status_changed","data":{"terminal_id":"t1","status":"blocked"}}),
        )
        .unwrap();
        assert_eq!(
            status,
            Event::AgentStatusChanged {
                terminal_id: "t1".into(),
                status: AgentStatus::Blocked,
                agent: None,
                session_id: None,
                message: None,
                message_at: None
            }
        );
    }

    #[test]
    fn clean_message_strips_controls_collapses_and_truncates() {
        assert_eq!(
            clean_message("  Allow\u{1b}[31m Bash?\n\tnow\u{0} ").as_deref(),
            Some("Allow [31m Bash? now")
        );
        assert_eq!(clean_message("\n\t \u{7}"), None);
        assert_eq!(clean_message(""), None);
        let long = "ab ".repeat(200);
        let cut = clean_message(&long).unwrap();
        assert_eq!(cut.chars().count(), 299);
        assert!(cut.ends_with('b'));
        assert_eq!(
            clean_message(&"é".repeat(400)).unwrap().chars().count(),
            300
        );
    }

    #[test]
    fn set_colors_params_have_optional_cursor_and_palette() {
        let p: SetColors =
            serde_json::from_value(json!({"foreground":"#ffffff","background":"#000000"})).unwrap();
        assert_eq!((p.cursor, p.palette), (None, None));
    }

    #[test]
    fn report_agent_params_and_the_terminal_agent_wire_shape() {
        let p: TerminalReportAgent =
            serde_json::from_value(json!({"terminal_id":"t1","seq":5,"status":"gone"})).unwrap();
        assert_eq!(p.status, AgentStatus::Gone);
        assert_eq!((p.agent, p.session_id, p.message), (None, None, None));
        let p: TerminalReportAgent = serde_json::from_value(
            json!({"terminal_id":"t1","seq":6,"status":"blocked","message":"Allow Bash?"}),
        )
        .unwrap();
        assert_eq!(p.message.as_deref(), Some("Allow Bash?"));
        let t: Terminal = serde_json::from_value(json!({"id":"t1","space_id":"s1","name":"n",
            "cwd":"/","cols":80,"rows":24,"status":"running",
            "agent":{"status":"working","agent":"claude","session_id":"abc"}}))
        .unwrap();
        assert_eq!(t.agent.unwrap().session_id.as_deref(), Some("abc"));
    }

    #[test]
    fn token_wire_shapes_and_names() {
        let p: TerminalReportTokens = serde_json::from_value(json!({"terminal_id":"t1",
            "tokens":{"deliver_step":"build","deliver_pr":null},"ttl_s":30,"source":"deliver"}))
        .unwrap();
        assert_eq!(p.tokens["deliver_step"].as_deref(), Some("build"));
        assert_eq!(p.tokens["deliver_pr"], None);
        assert_eq!(p.ttl_s, Some(30));
        let p: TerminalReportTokens =
            serde_json::from_value(json!({"terminal_id":"t1","tokens":{},"source":"s"})).unwrap();
        assert_eq!(p.ttl_s, None);
        assert!(
            serde_json::from_value::<TerminalReportTokens>(
                json!({"terminal_id":"t1","tokens":{"a":1},"source":"s"})
            )
            .is_err()
        );
        let c: TerminalClearTokens =
            serde_json::from_value(json!({"terminal_id":"t1","source":"deliver"})).unwrap();
        assert_eq!(c.source, "deliver");
        let t: Terminal = serde_json::from_value(json!({"id":"t1","space_id":"s1","name":"n",
            "cwd":"/","cols":80,"rows":24,"status":"running","tokens":{"deliver":"on"}}))
        .unwrap();
        assert_eq!(t.tokens["deliver"], "on");
        let event: Event = serde_json::from_str(
            r#"{"event":"terminal.tokens_changed","data":{"terminal_id":"t1","tokens":{"a":"b"}}}"#,
        )
        .unwrap();
        assert_eq!(
            event,
            Event::TerminalTokensChanged {
                terminal_id: "t1".into(),
                tokens: [("a".to_owned(), "b".to_owned())].into()
            }
        );
        assert!(valid_token_name("deliver_step-2"));
        assert!(valid_token_name(&"a".repeat(32)));
        assert!(!valid_token_name(""));
        assert!(!valid_token_name(&"a".repeat(33)));
        assert!(!valid_token_name("a b"));
        assert!(!valid_token_name("é"));
        assert!(!valid_token_name("a.b"));
    }

    #[test]
    fn attach_wire_shapes() {
        let p: TerminalAttach =
            serde_json::from_value(json!({"terminal_id":"t1","mode":"control","take_over":true}))
                .unwrap();
        assert_eq!(p.mode, AttachMode::Control);
        assert!(p.take_over);
        let p: TerminalAttach =
            serde_json::from_value(json!({"terminal_id":"t1","mode":"observe"})).unwrap();
        assert!(!p.take_over);
        assert!(
            serde_json::from_value::<TerminalAttach>(json!({"terminal_id":"t1","mode":"x"}))
                .is_err()
        );
        let d: TerminalDetach = serde_json::from_value(json!({"terminal_id":"t1"})).unwrap();
        assert_eq!(d.terminal_id, "t1");
        assert!(serde_json::from_value::<TerminalDetach>(json!({})).is_err());

        let e = Event::TerminalDetached {
            terminal_id: "t1".into(),
            reason: DetachReason::TakenOver,
        };
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            json!({"event":"terminal.detached","data":{"terminal_id":"t1","reason":"taken_over"}})
        );
        let line =
            r#"{"event":"terminal.output","data":{"terminal_id":"t1","seq":3,"data":"aGk="}}"#;
        match parse_server_line(line).unwrap() {
            ServerLine::Event(Event::TerminalOutput { seq, data, .. }) => {
                assert_eq!((seq, decode_bytes(&data).unwrap()), (3, b"hi".to_vec()));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_protocol_3_space_has_no_tabs_and_a_protocol_4_space_lists_them() {
        let old: Space = serde_json::from_value(json!({"id":"s1","name":"a","cwd":"/x"})).unwrap();
        assert_eq!((old.tabs.len(), old.active_tab), (0, None));
        let space: Space = serde_json::from_value(
            json!({"id":"s1","name":"a","cwd":"/x","tabs":["tab2","tab3"],"active_tab":"tab3"}),
        )
        .unwrap();
        assert_eq!(space.tabs, ["tab2", "tab3"]);
        assert_eq!(space.active_tab.as_deref(), Some("tab3"));
        assert_eq!(
            serde_json::to_value(Space {
                id: "s1".into(),
                name: "a".into(),
                cwd: "/x".into(),
                ..Default::default()
            })
            .unwrap(),
            json!({"id":"s1","name":"a","cwd":"/x","tabs":[]})
        );
    }

    fn tree() -> Value {
        json!({"kind":"split","split_id":"split5","direction":"right","ratio":0.25,"children":[
            {"kind":"terminal","terminal_id":"t1"},
            {"kind":"split","split_id":"split7","direction":"down","ratio":0.5,"children":[
                {"kind":"terminal","terminal_id":"t4"},
                {"kind":"terminal","terminal_id":"t6"}
            ]}
        ]})
    }

    #[test]
    fn a_tab_and_its_split_tree_round_trip() {
        let wire = json!({"tab_id":"tab2","space_id":"s1","name":"dev","root":tree(),
            "active_terminal":"t4"});
        let tab: Tab = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(tab.root.terminal_ids(), ["t1", "t4", "t6"]);
        assert_eq!(tab.root.split_ids(), ["split5", "split7"]);
        assert_eq!(tab.root.first_terminal(), "t1");
        assert!(tab.root.contains_terminal("t6"));
        assert!(!tab.root.contains_terminal("t9"));
        match &tab.root {
            Node::Split {
                direction, ratio, ..
            } => assert_eq!((*direction, *ratio), (SplitDirection::Right, 0.25)),
            other => panic!("{other:?}"),
        }
        assert_eq!(serde_json::to_value(&tab).unwrap(), wire);
        let leaf: Node =
            serde_json::from_value(json!({"kind":"terminal","terminal_id":"t1"})).unwrap();
        assert_eq!(leaf, Node::terminal("t1"));
    }

    #[test]
    fn a_split_needs_exactly_two_children_and_a_known_kind_and_direction() {
        let leaf = json!({"kind":"terminal","terminal_id":"t1"});
        let split = |children: Value, direction: &str| {
            json!({"kind":"split","split_id":"split1","direction":direction,"ratio":0.5,
                "children":children})
        };
        assert!(serde_json::from_value::<Node>(split(json!([leaf, leaf]), "down")).is_ok());
        assert!(serde_json::from_value::<Node>(split(json!([leaf]), "down")).is_err());
        assert!(serde_json::from_value::<Node>(split(json!([leaf, leaf, leaf]), "down")).is_err());
        assert!(serde_json::from_value::<Node>(split(json!([leaf, leaf]), "left")).is_err());
        assert!(serde_json::from_value::<Node>(json!({"kind":"pane","terminal_id":"t1"})).is_err());
        assert!(serde_json::from_value::<Node>(json!({"kind":"split","split_id":"x"})).is_err());
    }

    #[test]
    fn ratios_are_strictly_between_0_and_1() {
        assert!(valid_ratio(0.5));
        assert!(valid_ratio(0.01));
        assert!(!valid_ratio(0.0));
        assert!(!valid_ratio(1.0));
        assert!(!valid_ratio(-0.2));
        assert!(!valid_ratio(f64::NAN));
        assert!(!valid_ratio(f64::INFINITY));
    }

    #[test]
    fn the_space_list_carries_tabs_and_defaults_them_for_protocol_3() {
        let list: SpaceList = serde_json::from_value(json!({"spaces":[{"id":"s1","name":"a",
            "cwd":"/x","tabs":["tab2"],"active_tab":"tab2"}],"tabs":[{"tab_id":"tab2",
            "space_id":"s1","name":"zsh","root":{"kind":"terminal","terminal_id":"t3"},
            "active_terminal":"t3"}]}))
        .unwrap();
        assert_eq!(list.tabs[0].root, Node::terminal("t3"));
        let old: SpaceList =
            serde_json::from_value(json!({"spaces":[{"id":"s1","name":"a","cwd":"/x"}]})).unwrap();
        assert!(old.tabs.is_empty());
    }

    #[test]
    fn tab_request_params() {
        let p: TabCreate = serde_json::from_value(json!({"space_id":"s1"})).unwrap();
        assert_eq!((p.name, p.terminal_id), (None, None));
        let p: TabCreate =
            serde_json::from_value(json!({"space_id":"s1","name":"logs","terminal_id":"t2"}))
                .unwrap();
        assert_eq!(
            (p.name.as_deref(), p.terminal_id.as_deref()),
            (Some("logs"), Some("t2"))
        );
        assert!(serde_json::from_value::<TabCreate>(json!({})).is_err());

        let p: TabRename = serde_json::from_value(json!({"tab_id":"tab2","name":"x"})).unwrap();
        assert_eq!(p.name, "x");
        let p: TabClose = serde_json::from_value(json!({"tab_id":"tab2"})).unwrap();
        assert_eq!(p.tab_id, "tab2");
        let p: TabMove = serde_json::from_value(json!({"tab_id":"tab2","index":0})).unwrap();
        assert_eq!(p.index, 0);
        assert!(serde_json::from_value::<TabMove>(json!({"tab_id":"tab2","index":-1})).is_err());

        let p: TabSplit =
            serde_json::from_value(json!({"terminal_id":"t1","direction":"down"})).unwrap();
        assert_eq!(p.direction, SplitDirection::Down);
        assert_eq!(
            (p.ratio, p.cwd, p.name, p.command),
            (None, None, None, None)
        );
        let p: TabSplit = serde_json::from_value(json!({"terminal_id":"t1","direction":"right",
            "ratio":0.3,"cwd":"/w","name":"logs","command":["tail","-f","x"]}))
        .unwrap();
        assert_eq!(p.ratio, Some(0.3));
        assert_eq!(p.command.unwrap(), ["tail", "-f", "x"]);
        assert!(serde_json::from_value::<TabSplit>(json!({"terminal_id":"t1"})).is_err());

        let p: TabResizeSplit =
            serde_json::from_value(json!({"tab_id":"tab2","split_id":"split3","ratio":0.7}))
                .unwrap();
        assert_eq!((p.split_id.as_str(), p.ratio), ("split3", 0.7));
        assert!(
            serde_json::from_value::<TabResizeSplit>(json!({"tab_id":"tab2","ratio":0.7})).is_err()
        );

        let p: TabFocus = serde_json::from_value(json!({"tab_id":"tab2"})).unwrap();
        assert_eq!(p.terminal_id, None);
        let p: TabFocus =
            serde_json::from_value(json!({"tab_id":"tab2","terminal_id":"t4"})).unwrap();
        assert_eq!(p.terminal_id.as_deref(), Some("t4"));
    }

    #[test]
    fn tab_event_wire_shapes() {
        let tab = json!({"tab_id":"tab2","space_id":"s1","name":"dev","root":tree(),
            "active_terminal":"t1"});
        for name in ["tab.created", "tab.changed"] {
            let event: Event =
                serde_json::from_value(json!({"event":name,"data":{"tab":tab}})).unwrap();
            let parsed = match &event {
                Event::TabCreated { tab } | Event::TabChanged { tab } => tab,
                other => panic!("{other:?}"),
            };
            assert_eq!(parsed.tab_id, "tab2");
            assert_eq!(
                serde_json::to_value(&event).unwrap(),
                json!({"event":name,"data":{"tab":tab}})
            );
        }
        let closed = Event::TabClosed {
            tab_id: "tab2".into(),
            space_id: "s1".into(),
        };
        assert_eq!(
            serde_json::to_value(&closed).unwrap(),
            json!({"event":"tab.closed","data":{"tab_id":"tab2","space_id":"s1"}})
        );
        let changed: Event = serde_json::from_value(json!({"event":"space.changed","data":{
            "space":{"id":"s1","name":"a","cwd":"/x","tabs":["tab3","tab2"],"active_tab":"tab2"}}}))
        .unwrap();
        match changed {
            Event::SpaceChanged { space } => assert_eq!(space.tabs, ["tab3", "tab2"]),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bytes_round_trip_including_non_utf8() {
        let raw = [0u8, 0xff, 0x1b, b'[', 0xc3];
        assert_eq!(decode_bytes(&encode_bytes(&raw)).unwrap(), raw);
        assert!(decode_bytes("***").is_err());
    }

    #[test]
    fn server_lines_are_told_apart() {
        let line = r#"{"event":"space.closed","data":{"space_id":"s1"}}"#;
        assert!(matches!(
            parse_server_line(line).unwrap(),
            ServerLine::Event(Event::SpaceClosed { .. })
        ));
        let line = r#"{"id":"1","result":{"protocol":1}}"#;
        assert!(matches!(
            parse_server_line(line).unwrap(),
            ServerLine::Response(_)
        ));
        let line = r#"{"id":"1","error":{"code":"not_found","message":"x"}}"#;
        match parse_server_line(line).unwrap() {
            ServerLine::Response(r) => assert_eq!(r.error.unwrap().code, ErrorCode::NotFound),
            other => panic!("{other:?}"),
        }
    }

    fn unix_env(socket: Option<&str>, runtime: Option<&str>, user: Option<&str>) -> SocketEnv {
        SocketEnv {
            socket: socket.map(Into::into),
            base: runtime.map(Into::into),
            user: user.map(Into::into),
            temp_dir: PathBuf::from("/tmp"),
            windows: false,
        }
    }

    #[test]
    fn socket_path_precedence() {
        let stable = Channel::Stable;
        assert_eq!(
            unix_env(Some("/a.sock"), Some("/run/user/1"), None).path(stable),
            PathBuf::from("/a.sock")
        );
        assert_eq!(
            unix_env(Some(""), Some("/run/user/1"), None).path(stable),
            PathBuf::from("/run/user/1/poimen/server.sock")
        );
        assert_eq!(
            unix_env(None, None, Some("jk")).path(stable),
            PathBuf::from("/tmp/poimen-jk/server.sock")
        );
    }

    #[test]
    fn the_rc_channel_has_its_own_socket() {
        assert_eq!(
            unix_env(None, Some("/run/user/1"), None).path(Channel::Rc),
            PathBuf::from("/run/user/1/poimen-rc/server.sock")
        );
        assert_eq!(
            unix_env(None, None, Some("jk")).path(Channel::Rc),
            PathBuf::from("/tmp/poimen-rc-jk/server.sock")
        );
        assert_eq!(
            unix_env(None, None, None).path(Channel::Rc),
            PathBuf::from("/tmp/poimen-rc-user/server.sock")
        );
    }

    #[test]
    fn the_default_socket_of_the_other_channel_is_foreign_and_ignored() {
        // A stable app started from a terminal of the rc server, and the other way round.
        let rc_terminal = unix_env(
            Some("/run/user/1/poimen-rc/server.sock"),
            Some("/run/user/1"),
            None,
        );
        assert!(rc_terminal.is_foreign(Channel::Stable));
        assert!(!rc_terminal.is_foreign(Channel::Rc));
        assert_eq!(
            rc_terminal.path(Channel::Stable),
            PathBuf::from("/run/user/1/poimen/server.sock")
        );
        assert_eq!(
            rc_terminal.path(Channel::Rc),
            PathBuf::from("/run/user/1/poimen-rc/server.sock")
        );
        let stable_terminal = unix_env(Some("/tmp/poimen-jk/server.sock"), None, Some("jk"));
        assert!(stable_terminal.is_foreign(Channel::Rc));
        assert!(!stable_terminal.is_foreign(Channel::Stable));
        // Any other path is an explicit choice, honored by both.
        let custom = unix_env(Some("/a.sock"), Some("/run/user/1"), None);
        assert!(!custom.is_foreign(Channel::Stable));
        assert!(!custom.is_foreign(Channel::Rc));
        assert_eq!(custom.path(Channel::Rc), PathBuf::from("/a.sock"));
        assert!(!unix_env(None, Some("/run/user/1"), None).is_foreign(Channel::Rc));
    }

    fn windows_env(socket: Option<&str>, local: Option<&str>, user: Option<&str>) -> SocketEnv {
        SocketEnv {
            socket: socket.map(Into::into),
            base: local.map(Into::into),
            user: user.map(Into::into),
            temp_dir: PathBuf::from(r"C:\Temp"),
            windows: true,
        }
    }

    #[test]
    fn windows_socket_path_precedence() {
        let tmp = PathBuf::from(r"C:\Temp");
        let local = PathBuf::from(r"C:\Users\u\AppData\Local");
        let local_str = r"C:\Users\u\AppData\Local";
        let stable = Channel::Stable;
        assert_eq!(
            windows_env(Some(r"D:\x.sock"), Some(local_str), None).path(stable),
            PathBuf::from(r"D:\x.sock")
        );
        assert_eq!(
            windows_env(Some(""), Some(local_str), None).path(stable),
            local.join("poimen-server").join("server.sock")
        );
        assert_eq!(
            windows_env(None, Some(""), Some("jk")).path(stable),
            tmp.join("poimen-jk").join("server.sock")
        );
        assert_eq!(
            windows_env(None, None, None).path(stable),
            tmp.join("poimen-user").join("server.sock")
        );
        assert_eq!(
            windows_env(None, Some(local_str), None).path(Channel::Rc),
            local.join("poimen-server-rc").join("server.sock")
        );
        assert_eq!(
            windows_env(None, None, Some("jk")).path(Channel::Rc),
            tmp.join("poimen-rc-jk").join("server.sock")
        );
        let stable_terminal = windows_env(
            Some(
                &local
                    .join("poimen-server")
                    .join("server.sock")
                    .to_string_lossy(),
            ),
            Some(local_str),
            None,
        );
        assert!(stable_terminal.is_foreign(Channel::Rc));
        assert_eq!(
            stable_terminal.path(Channel::Rc),
            local.join("poimen-server-rc").join("server.sock")
        );
    }

    #[test]
    fn the_windows_dir_needs_local_app_data() {
        assert_eq!(
            windows_dir_from(Some(r"C:\L".into()), Channel::Stable),
            Some(PathBuf::from(r"C:\L").join("poimen-server"))
        );
        assert_eq!(
            windows_dir_from(Some(r"C:\L".into()), Channel::Rc),
            Some(PathBuf::from(r"C:\L").join("poimen-server-rc"))
        );
        assert_eq!(windows_dir_from(Some("".into()), Channel::Stable), None);
        assert_eq!(windows_dir_from(None, Channel::Rc), None);
    }

    #[test]
    fn channel_values_parse_and_name_things() {
        assert_eq!(Channel::parse(None), Some(Channel::Stable));
        assert_eq!(Channel::parse(Some("")), Some(Channel::Stable));
        assert_eq!(Channel::parse(Some("stable")), Some(Channel::Stable));
        assert_eq!(Channel::parse(Some("rc")), Some(Channel::Rc));
        assert_eq!(Channel::parse(Some("RC")), None);
        assert_eq!(Channel::parse(Some("beta")), None);
        assert_eq!(Channel::parse(Some("rcx")), None);
        assert_eq!(Channel::Stable.named("poimen"), "poimen");
        assert_eq!(Channel::Rc.named("poimen"), "poimen-rc");
        assert_eq!(Channel::Stable.server_program(false), "poimen-server");
        assert_eq!(Channel::Rc.server_program(false), "poimen-server-rc");
        assert_eq!(Channel::Rc.server_program(true), "poimen-server-rc.exe");
        assert_eq!(Channel::Rc.name(), "rc");
        assert_eq!(serde_json::to_value(Channel::Rc).unwrap(), "rc");
        assert_eq!(serde_json::to_value(Channel::Stable).unwrap(), "stable");
    }

    #[test]
    fn space_create_params_carry_the_layout_flag_trust_answer_and_command() {
        let p: SpaceCreate = serde_json::from_value(json!({
            "name": "a", "cwd": "/x", "layout": false,
            "trust": {"kind": "trust", "hash": "ab12"}, "command": "claude"
        }))
        .unwrap();
        assert!(!p.layout);
        assert_eq!(
            p.trust,
            Some(TrustAnswer::Trust {
                hash: "ab12".into()
            })
        );
        assert_eq!(p.command.as_deref(), Some("claude"));
        let without: TrustAnswer = serde_json::from_value(json!({"kind": "without"})).unwrap();
        assert_eq!(without, TrustAnswer::Without);
        assert!(serde_json::from_value::<TrustAnswer>(json!({"kind": "maybe"})).is_err());
        assert_eq!(
            serde_json::to_value(SpaceCreate::empty("a", "/x")).unwrap(),
            json!({"name": "a", "cwd": "/x", "layout": false})
        );
        assert!(SpaceCreate::with_layout("a", "/x").layout);
    }

    #[test]
    fn space_create_result_is_the_trust_question_or_the_space() {
        let asked: SpaceCreateResult = serde_json::from_value(json!({"trust_required": {
            "root": "/r", "files": ["/r/.poimen/config.json"], "hash": "ab12",
            "commands": [{"what": "setup", "command": "make"}],
            "layout": [{"name": "agent", "command": "claude"},
                       {"name": "sh", "split": {"from": "agent", "direction": "down", "ratio": 0.5}}]
        }}))
        .unwrap();
        let SpaceCreateResult::TrustRequired { trust_required } = asked else {
            panic!("{asked:?}")
        };
        assert_eq!(trust_required.hash, "ab12");
        assert_eq!(trust_required.commands[0].command, "make");
        let layout = trust_required.layout.unwrap();
        assert_eq!(layout[0].split, None);
        assert_eq!(
            layout[1].split,
            Some(LayoutSplit {
                from: "agent".into(),
                direction: SplitDirection::Down,
                ratio: 0.5
            })
        );

        let created: SpaceCreateResult = serde_json::from_value(json!({
            "space": {"id": "s1", "name": "a", "cwd": "/x", "tabs": ["tab2"], "active_tab": "tab2"},
            "tabs": [{"tab_id": "tab2", "space_id": "s1", "name": "agent",
                      "root": {"kind": "terminal", "terminal_id": "t3"}, "active_terminal": "t3"}],
            "warnings": ["w"], "setup_started": true
        }))
        .unwrap();
        let SpaceCreateResult::Created(created) = created else {
            panic!("{created:?}")
        };
        assert_eq!(created.space.id, "s1");
        assert_eq!(created.tabs[0].root, Node::terminal("t3"));
        assert_eq!(created.warnings, ["w"]);
        assert!(created.setup_started);

        // A protocol 4 server answers only `{space}`.
        let old: SpaceCreateResult =
            serde_json::from_value(json!({"space": {"id": "s1", "name": "a", "cwd": "/x"}}))
                .unwrap();
        assert!(
            matches!(old, SpaceCreateResult::Created(c) if c.tabs.is_empty() && !c.setup_started)
        );
    }

    #[test]
    fn space_apply_layout_params_carry_space_cwd_preset_and_trust() {
        let r = parse_request(
            r#"{"id":"1","method":"space.apply_layout","params":{"space_id":"s1","cwd":"/x",
                "preset":"duo","trust":{"kind":"without"}}}"#,
        )
        .unwrap();
        assert_eq!(r.method, "space.apply_layout");
        let p: SpaceApplyLayout = params(&r).unwrap();
        assert_eq!(p.space_id, "s1");
        assert_eq!(p.cwd.as_deref(), Some("/x"));
        assert_eq!(p.preset.as_deref(), Some("duo"));
        assert_eq!(p.trust, Some(TrustAnswer::Without));

        // Only `space_id` is required; the defaults stay off the wire.
        let bare: SpaceApplyLayout = serde_json::from_value(json!({"space_id": "s2"})).unwrap();
        assert_eq!(bare, SpaceApplyLayout::resolved("s2"));
        assert_eq!(
            serde_json::to_value(SpaceApplyLayout::preset("s2", "duo")).unwrap(),
            json!({"space_id": "s2", "preset": "duo"})
        );
        assert!(serde_json::from_value::<SpaceApplyLayout>(json!({"preset": "duo"})).is_err());
    }

    #[test]
    fn space_apply_layout_result_is_the_trust_question_or_the_new_tabs() {
        let asked: SpaceApplyLayoutResult = serde_json::from_value(json!({"trust_required": {
            "root": "/x", "files": ["/x/.poimen/config.json"], "hash": "h",
            "commands": [{"what": "preset \"duo\": pane \"a\"", "command": "vim"}],
            "layout": [{"name": "a", "command": "vim"}]
        }}))
        .unwrap();
        let SpaceApplyLayoutResult::TrustRequired { trust_required } = asked else {
            panic!("{asked:?}")
        };
        assert_eq!(trust_required.hash, "h");
        assert_eq!(trust_required.commands[0].command, "vim");

        let applied: SpaceApplyLayoutResult = serde_json::from_value(json!({
            "space": {"id": "s1", "name": "a", "cwd": "/x", "tabs": ["tab2", "tab4"]},
            "tabs": [{"tab_id": "tab4", "space_id": "s1", "name": "a",
                      "root": {"kind": "terminal", "terminal_id": "t5"}, "active_terminal": "t5"}],
            "warnings": [], "setup_started": false
        }))
        .unwrap();
        let SpaceApplyLayoutResult::Applied(applied) = applied else {
            panic!("{applied:?}")
        };
        assert_eq!(applied.space.tabs, ["tab2", "tab4"]);
        assert_eq!(applied.tabs.len(), 1);
        assert_eq!(applied.tabs[0].root, Node::terminal("t5"));
    }
}

#[cfg(all(test, unix))]
mod private_dir_tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn fresh(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-pd-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn mode(p: &std::path::Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn a_0700_dir_of_ours_passes() {
        let dir = fresh("ok");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
        check_private_dir(&dir, false).unwrap();
    }

    #[test]
    fn a_0777_dir_is_fixed_with_fix_and_refused_without() {
        let dir = fresh("loose");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = check_private_dir(&dir, false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(mode(&dir), 0o777);
        check_private_dir(&dir, true).unwrap();
        assert_eq!(mode(&dir), 0o700);
    }

    #[test]
    fn a_symlink_to_a_dir_is_refused_even_with_fix() {
        let target = fresh("target");
        let link = std::env::temp_dir().join(format!("poimen-pd-{}-link", std::process::id()));
        let _ = std::fs::remove_file(&link);
        symlink(&target, &link).unwrap();
        let err = check_private_dir(&link, true).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        std::fs::remove_file(&link).unwrap();
    }

    #[test]
    fn a_dir_of_another_user_is_refused() {
        // `/` is owned by root; skipped when the tests run as root.
        if current_uid().unwrap() == 0 {
            return;
        }
        let err = check_private_dir(std::path::Path::new("/"), true).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_missing_dir_is_not_found() {
        let err = check_private_dir(&fresh("m").join("nope"), false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
