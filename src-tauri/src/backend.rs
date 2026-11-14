//! The terminal backend poimen talks to, and starting it when it is not running (ADR 0003).
//!
//! poimen never owns pane state and stops the server only on a confirmed restart or quit: `ensure_running` only starts a missing
//! server, detached from poimen, and waits until it answers. A server that already answers (also
//! one started by the herdr TUI) is left alone. A later `poimen-server` backend implements the same
//! trait.

use std::fmt;
use std::io;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::gui_path;
use serde_json::Value;

use crate::herdr::{
    self, AgentSession, Client, Probe, PromptOutcome, SUPPORTED_PROTOCOL, ServerInfo,
};
use crate::spaces;
use crate::stream::{self, Sessions};
use crate::watch;

/// How long a started server may take to answer, and how often it is asked.
pub const START_TIMEOUT: Duration = Duration::from_secs(10);
pub const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// What `ensure_running` found or did.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A server already answered; nothing was started.
    AlreadyRunning,
    /// Nothing answered and `autoStart` is off.
    Disabled,
    /// poimen started the server and it answers.
    Started,
    /// A server answers but is older than the one poimen bundles. Left running: stopping it ends
    /// every terminal, so it needs the user's consent (`Backend::restart_server`).
    Outdated { running: String, bundled: String },
    /// A server answers with another protocol version. No second server is started on its socket.
    Incompatible { running: String, protocol: u32 },
}

#[derive(Debug, PartialEq, Eq)]
pub enum StartError {
    /// The `herdr` program is not on the PATH poimen uses.
    NotFound,
    /// The process could not be spawned.
    Spawn(String),
    /// The server did not answer within the timeout.
    Timeout(Duration),
    /// The socket is bound but does not answer, and `autoStart` is off (nothing was waited for).
    NotAnswering,
    /// The started server exited before it answered.
    Exited,
    /// The started server speaks an older protocol than poimen supports.
    ProtocolTooOld { version: String, protocol: u32 },
    /// Another backend's start failed; the text is the whole message.
    Other(String),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::NotFound => write!(
                f,
                "`herdr` was not found on PATH; install herdr or add it to the PATH of your login shell"
            ),
            StartError::Spawn(err) => write!(f, "could not start `herdr server`: {err}"),
            StartError::Timeout(limit) => write!(
                f,
                "`herdr server` did not answer within {} s",
                limit.as_secs()
            ),
            StartError::NotAnswering => {
                write!(f, "the herdr socket exists but the server is not answering")
            }
            StartError::Exited => write!(f, "`herdr server` exited right after it was started"),
            StartError::ProtocolTooOld { version, protocol } => write!(
                f,
                "herdr {version} speaks protocol {protocol}, poimen needs {SUPPORTED_PROTOCOL}; update herdr"
            ),
            StartError::Other(message) => write!(f, "{message}"),
        }
    }
}

pub use poimen_core::layout::SplitDirection;

/// How a terminal stream is attached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Read-only; any number of observers per terminal.
    Observe,
    /// Writable; one controller per terminal.
    Control,
}

/// Version and protocol of the running backend.
pub type BackendInfo = ServerInfo;

/// Spaces, tabs and terminals in one read, in the backend-neutral shape (#586) every backend maps
/// its own model onto; the wire shape of the `snapshot` command.
pub use poimen_core::snapshot::Snapshot;

/// A space that was just created.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSpace {
    pub space_id: String,
    /// The terminal the space starts with.
    pub root_terminal: Option<String>,
}

/// A space whose layout the backend resolves and builds itself (poimen-server's `space.create`).
#[derive(Debug, Clone, Copy)]
pub struct LayoutRequest<'a> {
    pub cwd: &'a str,
    pub label: &'a str,
    /// The user's answer to an earlier [`LaidOut::Trust`]; `None` on the first send.
    pub decision: Option<&'a crate::trust::Decision>,
    /// Replaces the command of the layout's root pane (a task's command).
    pub command: Option<&'a str>,
}

/// A layout the backend builds into an existing space itself (poimen-server's
/// `space.apply_layout`): the resolved layout of the space (rebuild) or a preset of its project
/// file, as new tabs.
#[derive(Debug, Clone, Copy)]
pub struct ApplyRequest<'a> {
    pub space_id: &'a str,
    /// The directory whose project file and layout count (the space's own for a rebuild).
    pub cwd: &'a str,
    /// A preset of the project file; `None`: the resolved layout with the project's `setup`.
    pub preset: Option<&'a str>,
    /// The user's answer to an earlier [`LaidOut::Trust`]; `None` on the first send.
    pub decision: Option<&'a crate::trust::Decision>,
}

/// What [`Backend::create_space_with_layout`] and [`Backend::apply_layout`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaidOut {
    /// The space exists with its layout (and the setup tab, when `setup_started`); for
    /// `apply_layout` the space it was built into.
    Created {
        space_id: String,
        warnings: Vec<String>,
        setup_started: bool,
    },
    /// The project file has untrusted commands; nothing was created.
    Trust(crate::trust::TrustRequest),
}

/// An event pushed by the backend (`kind` and a payload), or one of the synthetic
/// `poimen.connected`, `poimen.disconnected` and `poimen.resync`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackendEvent {
    pub kind: String,
    pub data: Value,
}

/// The kind of the event that carries an agent status change of one terminal to the frontend,
/// with `{terminal_id, agent_status, agent_message?, agent_message_at?}`, the same for every
/// backend (herdr's `pane.agent_status_changed` and poimen-server's `agent.status_changed` are
/// mapped onto it).
pub const AGENT_STATUS_CHANGED: &str = "terminal.agent_status_changed";

/// Why a stream ended, when the reason is more than text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseCode {
    /// Another client holds the controller; the stream can be reopened as an observer or with a
    /// takeover.
    ControllerHeld,
    /// The terminal's process ended (`exit_code` says how); the last screen stays.
    Exited,
    /// The connection to the server ended without a record (it stopped or crashed); the
    /// terminal may live on in its host, so the view attaches again once a server answers.
    Lost,
}

/// One record of a terminal stream, the same for every backend. `bytes` is base64.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamRecord {
    /// The screen is replaced: a full redraw at the given size.
    Reset { cols: u16, rows: u16, bytes: String },
    /// More output to write to the screen.
    Output { bytes: String },
    /// The size of the stream changed.
    Resized { cols: u16, rows: u16 },
    /// This client holds (or no longer holds) the controller of the terminal.
    Controller { attached: bool },
    /// The last record of every stream.
    Closed {
        reason: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        exit_code: Option<i32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        code: Option<CloseCode>,
    },
}

/// What `open_stream` needs.
#[derive(Debug, Clone, Copy)]
pub struct StreamRequest<'a> {
    pub mode: Mode,
    pub target: &'a str,
    pub cols: u16,
    pub rows: u16,
    pub takeover: bool,
}

/// An opened stream. `emulated` streams carry raw PTY bytes: xterm.js tracks the terminal modes
/// itself, so the mouse and the wheel reach the application as escape sequences. Rendered streams
/// (herdr frames) carry a finished screen: the view forwards mouse and wheel as commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OpenedStream {
    pub id: u32,
    pub emulated: bool,
}

/// Receives the records of a stream, from a reader thread, with the stream id.
pub type OnRecord = Box<dyn Fn(u32, StreamRecord) + Send + 'static>;

/// A terminal backend: what the commands of poimen need from herdr today and from another daemon
/// later. Wire shapes stay in the implementation (`HerdrBackend`: `herdr.rs`, `stream.rs`,
/// `watch.rs`). Errors are text for the user.
pub trait Backend: Send + Sync {
    /// Makes sure the backend answers: returns at once when it does, otherwise starts it (when
    /// `auto_start` allows) and waits. Never stops or restarts a running backend.
    fn ensure_running(&self, auto_start: bool) -> Result<Outcome, StartError>;
    /// Stops the running server and starts the bundled one. Ends every terminal: only after the
    /// user confirmed. Not supported by every backend.
    fn restart_server(&self) -> Result<Outcome, StartError> {
        Err(StartError::Other("this backend cannot be restarted".into()))
    }
    /// Stops the running server (the confirmed "Quit poimen and stop server"). Ends every terminal.
    fn stop_server(&self) -> Result<(), StartError> {
        Err(StartError::Other("this backend cannot be stopped".into()))
    }
    /// Version and protocol; `None` when unreachable or too old to say.
    fn info(&self) -> Option<BackendInfo>;
    /// Lists spaces, tabs and terminals.
    fn snapshot(&self) -> Result<Snapshot, String>;
    fn create_space(&self, cwd: &str, label: &str) -> Result<NewSpace, String>;
    /// Creates a space with the layout the backend resolves and builds itself, asking for trust
    /// in the project file first; `None` when the backend does not build layouts (herdr: the app
    /// creates the space with [`Backend::create_space`] and applies the layout).
    fn create_space_with_layout(
        &self,
        _request: LayoutRequest<'_>,
    ) -> Option<Result<LaidOut, String>> {
        None
    }
    /// Builds a layout into an existing space as new tabs, asking for trust in the project file
    /// first; `None` when the backend does not build layouts (herdr: the app creates the
    /// terminal and applies the layout with `layout::apply`).
    fn apply_layout(&self, _request: ApplyRequest<'_>) -> Option<Result<LaidOut, String>> {
        None
    }
    fn focus_space(&self, space_id: &str) -> Result<(), String>;
    fn close_space(&self, space_id: &str) -> Result<(), String>;
    /// Renames a space; the label must not be blank.
    fn rename_space(&self, space_id: &str, label: &str) -> Result<(), String>;
    /// A new terminal in a space (a new tab); returns its id.
    fn create_terminal(&self, space_id: &str, cwd: Option<&str>) -> Result<Option<String>, String>;
    /// A split is a new terminal next to `target`; `ratio` is the share the target keeps.
    fn split(
        &self,
        target: &str,
        direction: SplitDirection,
        cwd: Option<&str>,
        ratio: Option<f64>,
    ) -> Result<Option<String>, String>;
    fn close_terminal(&self, terminal_id: &str) -> Result<(), String>;
    /// Replaces a terminal by a new one with the same directory, program and place in its tab
    /// (a terminal on an older PTY host runs on the bundled one afterwards); the old process
    /// ends. Returns the new terminal's id. Not supported by every backend.
    fn restart_terminal(&self, _terminal_id: &str) -> Result<Option<String>, String> {
        Err("this backend cannot restart a terminal".into())
    }
    /// A blank label clears the name.
    fn rename_terminal(&self, terminal_id: &str, label: Option<&str>) -> Result<(), String>;
    /// Closes a tab with all its terminals. Not supported by every backend.
    fn close_tab(&self, _tab_id: &str) -> Result<(), String> {
        Err("this backend cannot close a tab".into())
    }
    /// Renames a tab; the name must not be blank. Not supported by every backend.
    fn rename_tab(&self, _tab_id: &str, _name: &str) -> Result<(), String> {
        Err("this backend cannot rename a tab".into())
    }
    /// Sets the ratio (share of the first child) of a split of a tab. Not supported by every
    /// backend.
    fn resize_split(&self, _tab_id: &str, _split_id: &str, _ratio: f64) -> Result<(), String> {
        Err("this backend cannot resize a split".into())
    }
    /// Focuses a terminal of a tab for every client. Not supported by every backend.
    fn focus_tab(&self, _tab_id: &str, _terminal_id: &str) -> Result<(), String> {
        Err("this backend cannot focus a pane of a tab".into())
    }
    /// Types `text` into the terminal and presses Enter.
    fn send_input(&self, terminal_id: &str, text: &str) -> Result<(), String>;
    fn agent_start(
        &self,
        terminal_id: &str,
        kind: &str,
        name: Option<&str>,
        args: &[String],
    ) -> Result<(), String>;
    fn agent_rename(&self, target: &str, name: Option<&str>) -> Result<(), String>;
    fn agent_prompt(&self, target: &str, text: &str) -> PromptOutcome;
    /// The agent session of a terminal whose transcript poimen can read.
    fn agent_session(&self, terminal_id: &str) -> Result<Option<AgentSession>, String>;
    /// The updates the agent transcript of a terminal gained since the last call of `follower`,
    /// read where the transcript lives (`agent.transcript`, #698). Only the poimen backend can:
    /// the others read the local files themselves.
    fn agent_transcript(
        &self,
        _terminal_id: &str,
        _follower: &str,
        _restart: bool,
    ) -> Result<poimen_client::protocol::AgentTranscriptResult, String> {
        Err("this backend cannot read agent transcripts".into())
    }
    /// Opens a terminal stream; `on_record` gets its records and, last, a `Closed`.
    fn open_stream(
        &self,
        request: StreamRequest<'_>,
        on_record: OnRecord,
    ) -> Result<OpenedStream, String>;
    /// Whether streams carry raw PTY bytes (xterm.js tracks the modes) rather than finished
    /// frames.
    fn emulated(&self) -> bool;
    fn stream_input(&self, id: u32, text: &str) -> Result<(), String>;
    fn stream_resize(&self, id: u32, cols: u16, rows: u16) -> Result<(), String>;
    /// Only for rendered streams.
    fn stream_scroll(&self, id: u32, up: bool, lines: u16) -> Result<(), String>;
    /// Only for rendered streams.
    fn stream_mouse(
        &self,
        id: u32,
        action: &str,
        button: &str,
        column: u16,
        row: u16,
        modifiers: u8,
    ) -> Result<(), String>;
    /// Ends one stream (releases a held controller); the terminal keeps running.
    fn close_stream(&self, id: u32);
    fn close_streams(&self);
    /// Subscribes to events and calls `emit` for each one, reconnecting when the connection is
    /// lost. Runs forever on the calling thread.
    fn watch(&self, emit: &dyn Fn(&BackendEvent));
}

/// The side effects `HerdrBackend` needs to start the server; faked in tests.
pub trait Runner {
    fn probe(&self) -> Probe;
    /// Finds `herdr` on the PATH of a GUI app.
    fn resolve(&self) -> Option<PathBuf>;
    /// Starts `<program> server` detached and returns without waiting for it.
    fn spawn(&self, program: &Path) -> io::Result<()>;
    fn sleep(&self, duration: Duration);
    /// Monotonic time since an arbitrary start; the deadline of `ensure_running` is measured with
    /// it, so slow probes count.
    fn now(&self) -> Duration;
    /// Whether the server started by `spawn` has exited again.
    fn exited(&self) -> bool;
}

/// The herdr backend: the socket client, the `herdr` binary for terminal streams, and the streams
/// that are open.
pub struct HerdrBackend<R: Runner> {
    runner: R,
    client: Client,
    bin: PathBuf,
    sessions: Sessions,
}

impl<R: Runner> HerdrBackend<R> {
    pub fn new(runner: R, client: Client, bin: PathBuf) -> Self {
        Self {
            runner,
            client,
            bin,
            sessions: Sessions::default(),
        }
    }
}

fn too_old(info: &ServerInfo) -> Option<StartError> {
    match info.protocol {
        Some(protocol) if protocol < SUPPORTED_PROTOCOL => Some(StartError::ProtocolTooOld {
            version: info.version.clone(),
            protocol,
        }),
        _ => None,
    }
}

impl<R: Runner> HerdrBackend<R> {
    /// `ensure_running`: only `Probe::Down` (nothing listens) allows a spawn, and only once: a socket that is bound
    /// but silent (server starting, busy, or started a moment ago by the herdr TUI or another
    /// poimen) is waited for up to `START_TIMEOUT` without spawning. A second poimen starting at
    /// the same instant can still pass its own re-probe before the first one spawns; the
    /// `StartLock` only serializes this process.
    fn start_if_down(&self, auto_start: bool) -> Result<Outcome, StartError> {
        let start = self.runner.now();
        let mut spawned = false;
        let mut first = true;
        // The deadline is checked before every probe (a probe can take up to PROBE_TIMEOUT), so the
        // call ends within START_TIMEOUT plus at most one probe.
        while first || self.runner.now().saturating_sub(start) < START_TIMEOUT {
            if !first {
                self.runner.sleep(POLL_INTERVAL);
                if self.runner.now().saturating_sub(start) >= START_TIMEOUT {
                    break;
                }
            }
            first = false;
            match self.runner.probe() {
                Probe::Up(info) if spawned => {
                    return match info.as_ref().and_then(too_old) {
                        Some(err) => Err(err),
                        None => Ok(Outcome::Started),
                    };
                }
                Probe::Up(_) => return Ok(Outcome::AlreadyRunning),
                Probe::Unresponsive if !auto_start => return Err(StartError::NotAnswering),
                Probe::Unresponsive => {}
                Probe::Down if spawned => {
                    if self.runner.exited() {
                        return Err(StartError::Exited);
                    }
                }
                Probe::Down if !auto_start => return Ok(Outcome::Disabled),
                Probe::Down => {
                    let program = self.runner.resolve().ok_or(StartError::NotFound)?;
                    // Re-probe right before spawning: the server may have appeared meanwhile.
                    if self.runner.probe() == Probe::Down {
                        self.runner
                            .spawn(&program)
                            .map_err(|e| StartError::Spawn(e.to_string()))?;
                        spawned = true;
                    }
                }
            }
        }
        Err(StartError::Timeout(START_TIMEOUT))
    }
}

/// The real runner: the herdr socket of the environment and the `herdr` binary of the login PATH.
pub struct SystemRunner {
    client: Client,
    origin: Instant,
    exited: Arc<AtomicBool>,
}

impl SystemRunner {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            origin: Instant::now(),
            exited: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl Runner for SystemRunner {
    fn probe(&self) -> Probe {
        self.client.probe()
    }

    fn resolve(&self) -> Option<PathBuf> {
        // One resolution for the start and for the terminals (`stream::resolve_bin`).
        let bin = crate::stream::resolve_bin_from_env();
        (bin.is_absolute() && bin.is_file()).then_some(bin)
    }

    fn spawn(&self, program: &Path) -> io::Result<()> {
        // Own process group, no terminal, no pipes: the server outlives poimen. It inherits
        // HERDR_SESSION / HERDR_SOCKET_PATH, so it listens where poimen looks. A small thread
        // reaps the child if it ever exits, so it does not stay a zombie while poimen runs.
        self.exited.store(false, Ordering::SeqCst);
        let exited = Arc::clone(&self.exited);
        let mut command = gui_path::command(program);
        command.arg("server");
        // Windows: the server keeps the inherited PATH; elsewhere it gets the login-shell PATH.
        if !cfg!(windows) {
            command.env("PATH", gui_path::child_path());
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        command.process_group(0);
        // Windows: own process group and no console window (CREATE_NEW_PROCESS_GROUP |
        // CREATE_NO_WINDOW), so closing poimen's console does not end the server.
        #[cfg(windows)]
        command.creation_flags(0x0000_0200 | gui_path::CREATE_NO_WINDOW);
        let mut child = command.spawn()?;
        std::thread::spawn(move || {
            let _ = child.wait();
            exited.store(true, Ordering::SeqCst);
        });
        Ok(())
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }

    fn now(&self) -> Duration {
        self.origin.elapsed()
    }

    fn exited(&self) -> bool {
        self.exited.load(Ordering::SeqCst)
    }
}

impl<R: Runner + Send + Sync> HerdrBackend<R> {
    fn list(&self, method: &str, key: &str) -> Result<Value, String> {
        let mut result = self.call(method, Value::Object(Default::default()))?;
        Ok(result[key].take())
    }

    fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        self.client
            .request(method, params)
            .map_err(|e| e.to_string())
    }
}

impl<R: Runner + Send + Sync> Backend for HerdrBackend<R> {
    fn ensure_running(&self, auto_start: bool) -> Result<Outcome, StartError> {
        self.start_if_down(auto_start)
    }

    fn info(&self) -> Option<BackendInfo> {
        self.client.info()
    }

    fn snapshot(&self) -> Result<Snapshot, String> {
        Ok(herdr::map_snapshot(
            &self.list("workspace.list", "workspaces")?,
            &self.list("tab.list", "tabs")?,
            &self.list("pane.list", "panes")?,
        ))
    }

    fn create_space(&self, cwd: &str, label: &str) -> Result<NewSpace, String> {
        let result = self.call("workspace.create", spaces::create_params(cwd, label))?;
        let space_id = spaces::created_workspace_id(&result)
            .ok_or("herdr did not report the new workspace")?;
        Ok(NewSpace {
            space_id,
            root_terminal: herdr::created_pane_id(&result),
        })
    }

    fn focus_space(&self, space_id: &str) -> Result<(), String> {
        self.call("workspace.focus", spaces::focus_params(space_id))
            .map(drop)
    }

    fn close_space(&self, space_id: &str) -> Result<(), String> {
        self.call("workspace.close", herdr::workspace_close_params(space_id))
            .map(drop)
    }

    fn rename_space(&self, space_id: &str, label: &str) -> Result<(), String> {
        self.call("workspace.rename", spaces::rename_params(space_id, label))
            .map(drop)
    }

    fn create_terminal(&self, space_id: &str, cwd: Option<&str>) -> Result<Option<String>, String> {
        let result = self.call("tab.create", herdr::tab_create_params(space_id, cwd))?;
        Ok(herdr::created_pane_id(&result))
    }

    fn split(
        &self,
        target: &str,
        direction: SplitDirection,
        cwd: Option<&str>,
        ratio: Option<f64>,
    ) -> Result<Option<String>, String> {
        let params = herdr::split_params_ratio(target, direction, cwd, ratio);
        let result = self.call("pane.split", params)?;
        Ok(herdr::created_pane_id(&result))
    }

    fn close_terminal(&self, terminal_id: &str) -> Result<(), String> {
        self.call("pane.close", herdr::close_params(terminal_id))
            .map(drop)
    }

    fn rename_terminal(&self, terminal_id: &str, label: Option<&str>) -> Result<(), String> {
        self.call("pane.rename", herdr::pane_rename_params(terminal_id, label))
            .map(drop)
    }

    fn send_input(&self, terminal_id: &str, text: &str) -> Result<(), String> {
        self.call(
            "pane.send_input",
            herdr::send_input_params(terminal_id, text),
        )
        .map(drop)
    }

    fn agent_start(
        &self,
        terminal_id: &str,
        kind: &str,
        name: Option<&str>,
        args: &[String],
    ) -> Result<(), String> {
        let params = herdr::agent_start_params(terminal_id, kind, name, args);
        self.call("agent.start", params).map(drop)
    }

    fn agent_rename(&self, target: &str, name: Option<&str>) -> Result<(), String> {
        self.call("agent.rename", herdr::agent_rename_params(target, name))
            .map(drop)
    }

    fn agent_prompt(&self, target: &str, text: &str) -> PromptOutcome {
        herdr::prompt_outcome(
            self.client
                .request("agent.prompt", herdr::agent_prompt_params(target, text)),
        )
    }

    fn agent_session(&self, terminal_id: &str) -> Result<Option<AgentSession>, String> {
        let panes = self.call("pane.list", Value::Object(Default::default()))?;
        Ok(herdr::parse_agent_session(&panes, terminal_id))
    }

    fn open_stream(
        &self,
        request: StreamRequest<'_>,
        on_record: OnRecord,
    ) -> Result<OpenedStream, String> {
        let mapper = std::sync::Mutex::new(stream::FrameMapper::new(request.mode));
        let id = self.sessions.open(
            stream::OpenRequest {
                bin: &self.bin,
                mode: request.mode,
                target: request.target,
                cols: request.cols,
                rows: request.rows,
                takeover: request.takeover,
            },
            move |id, record| {
                let records = mapper.lock().unwrap_or_else(|e| e.into_inner()).map(record);
                for record in records {
                    on_record(id, record);
                }
            },
        )?;
        // herdr streams finished frames, not PTY bytes.
        Ok(OpenedStream {
            id,
            emulated: false,
        })
    }

    fn emulated(&self) -> bool {
        false
    }

    fn stream_input(&self, id: u32, text: &str) -> Result<(), String> {
        self.sessions.send(id, &stream::input_command(text))
    }

    fn stream_resize(&self, id: u32, cols: u16, rows: u16) -> Result<(), String> {
        self.sessions.send(id, &stream::resize_command(cols, rows))
    }

    fn stream_scroll(&self, id: u32, up: bool, lines: u16) -> Result<(), String> {
        self.sessions.send(id, &stream::scroll_command(up, lines))
    }

    fn stream_mouse(
        &self,
        id: u32,
        action: &str,
        button: &str,
        column: u16,
        row: u16,
        modifiers: u8,
    ) -> Result<(), String> {
        stream::validate_mouse(action, button)?;
        self.sessions.send(
            id,
            &stream::mouse_command(action, button, column, row, modifiers),
        )
    }

    fn close_stream(&self, id: u32) {
        self.sessions.close(id);
    }

    fn close_streams(&self) {
        self.sessions.close_all();
    }

    fn watch(&self, emit: &dyn Fn(&BackendEvent)) {
        watch::run(self.client.clone(), |event| emit(&herdr_event(event)));
    }
}

/// A herdr event as a backend event: herdr's `pane.agent_status_changed` becomes the neutral
/// [`AGENT_STATUS_CHANGED`] with its `pane_id` as `terminal_id`; every other event passes as is
/// (the frontend only invalidates its snapshot on them).
pub fn herdr_event(event: &herdr::Event) -> BackendEvent {
    if event.kind == "pane.agent_status_changed" {
        let mut data = event.data.clone();
        if let Some(fields) = data.as_object_mut()
            && let Some(id) = fields.remove("pane_id")
        {
            fields.insert("terminal_id".into(), id);
        }
        return BackendEvent {
            kind: AGENT_STATUS_CHANGED.into(),
            data,
        };
    }
    BackendEvent {
        kind: event.kind.clone(),
        data: event.data.clone(),
    }
}

/// The result sent to the frontend (command `ensure_backend`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendStatus {
    /// `running`, `started`, `disabled`, `outdated`, `incompatible` or `failed`.
    pub state: &'static str,
    /// Why the start failed; `None` otherwise.
    pub reason: Option<String>,
    /// The version of the running server (`outdated`, `incompatible`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<String>,
    /// The version poimen bundles (`outdated`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bundled: Option<String>,
    /// The protocol version of the running server (`incompatible`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u32>,
}

impl From<Result<Outcome, StartError>> for BackendStatus {
    fn from(result: Result<Outcome, StartError>) -> Self {
        let base = Self {
            state: "failed",
            reason: None,
            running: None,
            bundled: None,
            protocol: None,
        };
        match result {
            Ok(Outcome::AlreadyRunning) => Self {
                state: "running",
                ..base
            },
            Ok(Outcome::Started) => Self {
                state: "started",
                ..base
            },
            Ok(Outcome::Disabled) => Self {
                state: "disabled",
                ..base
            },
            Ok(Outcome::Outdated { running, bundled }) => Self {
                state: "outdated",
                running: Some(running),
                bundled: Some(bundled),
                ..base
            },
            Ok(Outcome::Incompatible { running, protocol }) => Self {
                state: "incompatible",
                running: Some(running),
                protocol: Some(protocol),
                ..base
            },
            Err(err) => Self {
                reason: Some(err.to_string()),
                ..base
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::{Cell, RefCell};

    fn herdr(kind: &str, data: Value) -> herdr::Event {
        herdr::Event {
            kind: kind.into(),
            data,
        }
    }

    #[test]
    fn a_herdr_agent_status_event_becomes_the_neutral_one_keyed_by_terminal() {
        let event = herdr_event(&herdr(
            "pane.agent_status_changed",
            json!({"pane_id": "w1:p1", "workspace_id": "w1", "agent_status": "blocked"}),
        ));
        assert_eq!(event.kind, "terminal.agent_status_changed");
        assert_eq!(
            event.data,
            json!({"terminal_id": "w1:p1", "workspace_id": "w1", "agent_status": "blocked"})
        );
    }

    #[test]
    fn a_herdr_agent_status_event_without_a_pane_keeps_its_data() {
        let event = herdr_event(&herdr("pane.agent_status_changed", Value::Null));
        assert_eq!(event.kind, "terminal.agent_status_changed");
        assert_eq!(event.data, Value::Null);
        let event = herdr_event(&herdr(
            "pane.agent_status_changed",
            json!({"agent_status": "idle"}),
        ));
        assert_eq!(event.data, json!({"agent_status": "idle"}));
    }

    #[test]
    fn other_herdr_events_pass_unchanged() {
        let data = json!({"pane_id": "w1:p1"});
        let event = herdr_event(&herdr("pane.closed", data.clone()));
        assert_eq!(event.kind, "pane.closed");
        assert_eq!(event.data, data);
    }

    /// Answers `Down` for `down_probes` probes, then `up`; records what happened.
    struct Fake {
        down_probes: Cell<u32>,
        up: Probe,
        binary: Option<PathBuf>,
        spawn_error: bool,
        spawned: RefCell<Vec<PathBuf>>,
        slept: Cell<u32>,
        /// Fake clock; every probe and sleep advances it.
        clock: Cell<Duration>,
        probe_cost: Duration,
        exited: bool,
        /// Probe answers served after the `Down` ones and before `up`.
        script: RefCell<std::collections::VecDeque<Probe>>,
    }

    impl Fake {
        fn new(down_probes: u32, up: Probe) -> Self {
            Self {
                down_probes: Cell::new(down_probes),
                up,
                binary: Some(PathBuf::from("/opt/bin/herdr")),
                spawn_error: false,
                spawned: RefCell::new(Vec::new()),
                slept: Cell::new(0),
                clock: Cell::new(Duration::ZERO),
                probe_cost: Duration::ZERO,
                exited: false,
                script: RefCell::new(Default::default()),
            }
        }
    }

    impl Runner for &Fake {
        fn probe(&self) -> Probe {
            self.clock.set(self.clock.get() + self.probe_cost);
            if self.down_probes.get() > 0 {
                self.down_probes.set(self.down_probes.get() - 1);
                return Probe::Down;
            }
            if let Some(next) = self.script.borrow_mut().pop_front() {
                return next;
            }
            match &self.up {
                Probe::Down => Probe::Down,
                Probe::Unresponsive => Probe::Unresponsive,
                Probe::Up(info) => Probe::Up(info.as_ref().map(|i| ServerInfo {
                    version: i.version.clone(),
                    protocol: i.protocol,
                    supported: i.supported,
                })),
            }
        }
        fn resolve(&self) -> Option<PathBuf> {
            self.binary.clone()
        }
        fn spawn(&self, program: &Path) -> io::Result<()> {
            if self.spawn_error {
                return Err(io::Error::other("denied"));
            }
            self.spawned.borrow_mut().push(program.to_path_buf());
            Ok(())
        }
        fn sleep(&self, _d: Duration) {
            self.slept.set(self.slept.get() + 1);
            self.clock.set(self.clock.get() + _d);
        }
        fn now(&self) -> Duration {
            self.clock.get()
        }
        fn exited(&self) -> bool {
            self.exited
        }
    }

    fn up(protocol: Option<u32>) -> Probe {
        Probe::Up(Some(ServerInfo {
            version: "0.9.5".into(),
            protocol,
            supported: SUPPORTED_PROTOCOL,
        }))
    }

    fn run(fake: &Fake, auto_start: bool) -> Result<Outcome, StartError> {
        HerdrBackend::new(fake, Client::from_env(), PathBuf::new()).start_if_down(auto_start)
    }

    #[test]
    fn a_running_server_is_not_started_again() {
        let fake = Fake::new(0, up(Some(22)));
        assert_eq!(run(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_running_server_with_an_old_protocol_is_left_alone() {
        let fake = Fake::new(0, up(Some(5)));
        assert_eq!(run(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_missing_server_is_started_and_awaited() {
        let fake = Fake::new(3, up(Some(22)));
        assert_eq!(run(&fake, true), Ok(Outcome::Started));
        assert_eq!(*fake.spawned.borrow(), [PathBuf::from("/opt/bin/herdr")]);
        assert!(fake.slept.get() >= 1);
    }

    #[test]
    fn a_bound_but_silent_socket_is_waited_for_without_spawning() {
        let fake = Fake::new(0, up(Some(22)));
        fake.script
            .borrow_mut()
            .extend([Probe::Unresponsive, Probe::Unresponsive]);
        assert_eq!(run(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.spawned.borrow().is_empty());
        assert_eq!(fake.slept.get(), 2);
    }

    #[test]
    fn a_socket_that_never_answers_times_out_without_spawning() {
        let fake = Fake::new(0, Probe::Unresponsive);
        let err = run(&fake, true).unwrap_err();
        assert_eq!(err, StartError::Timeout(Duration::from_secs(10)));
        assert!(fake.spawned.borrow().is_empty());
        assert_eq!(fake.slept.get(), 100);
    }

    #[test]
    fn slow_probes_count_against_the_deadline() {
        let mut fake = Fake::new(0, Probe::Unresponsive);
        fake.probe_cost = Duration::from_millis(1500);
        assert_eq!(run(&fake, true), Err(StartError::Timeout(START_TIMEOUT)));
        // 1.6 s per round: the 10 s deadline passes after 7 rounds, not after 100 attempts.
        assert_eq!(fake.slept.get(), 6);
        assert!(fake.clock.get() < START_TIMEOUT + Duration::from_millis(1500) + POLL_INTERVAL);
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_slow_server_that_is_ready_at_eight_seconds_is_accepted() {
        let mut fake = Fake::new(0, up(Some(22)));
        fake.probe_cost = Duration::from_millis(1500);
        // Five silent probes, the sixth answers at about 9.6 s of fake time.
        fake.script
            .borrow_mut()
            .extend([const { Probe::Unresponsive }; 5]);
        assert_eq!(run(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.clock.get() >= Duration::from_secs(8));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_started_server_that_exits_fails_fast() {
        let mut fake = Fake::new(u32::MAX, Probe::Down);
        fake.exited = true;
        assert_eq!(run(&fake, true), Err(StartError::Exited));
        assert_eq!(fake.spawned.borrow().len(), 1);
        assert!(fake.clock.get() < Duration::from_secs(1));
    }

    #[test]
    fn a_silent_socket_with_auto_start_off_says_the_server_is_not_answering() {
        let fake = Fake::new(0, Probe::Unresponsive);
        let err = run(&fake, false).unwrap_err();
        assert_eq!(err, StartError::NotAnswering);
        assert!(!err.to_string().contains("auto start"));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_refused_connection_spawns_exactly_once() {
        let fake = Fake::new(5, up(Some(22)));
        assert_eq!(run(&fake, true), Ok(Outcome::Started));
        assert_eq!(fake.spawned.borrow().len(), 1);
    }

    #[test]
    fn a_server_that_appears_before_the_spawn_is_not_started_again() {
        let fake = Fake::new(1, up(Some(22)));
        assert_eq!(run(&fake, true), Ok(Outcome::AlreadyRunning));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_server_without_a_reported_protocol_is_accepted() {
        let fake = Fake::new(2, up(None));
        assert_eq!(run(&fake, true), Ok(Outcome::Started));
    }

    #[test]
    fn auto_start_off_starts_nothing() {
        let fake = Fake::new(u32::MAX, Probe::Down);
        assert_eq!(run(&fake, false), Ok(Outcome::Disabled));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn auto_start_off_still_reports_a_running_server() {
        let fake = Fake::new(0, up(Some(22)));
        assert_eq!(run(&fake, false), Ok(Outcome::AlreadyRunning));
    }

    #[test]
    fn a_missing_binary_is_reported_without_spawning() {
        let mut fake = Fake::new(u32::MAX, Probe::Down);
        fake.binary = None;
        let err = run(&fake, true).unwrap_err();
        assert_eq!(err, StartError::NotFound);
        assert!(err.to_string().contains("not found on PATH"));
        assert!(fake.spawned.borrow().is_empty());
    }

    #[test]
    fn a_spawn_failure_carries_the_reason() {
        let mut fake = Fake::new(u32::MAX, Probe::Down);
        fake.spawn_error = true;
        let err = run(&fake, true).unwrap_err();
        assert_eq!(err, StartError::Spawn("denied".into()));
        assert!(err.to_string().contains("denied"));
    }

    #[test]
    fn a_server_that_never_answers_times_out_after_one_start() {
        let fake = Fake::new(u32::MAX, Probe::Down);
        let err = run(&fake, true).unwrap_err();
        assert_eq!(err, StartError::Timeout(Duration::from_secs(10)));
        assert!(err.to_string().contains("10 s"));
        assert_eq!(fake.spawned.borrow().len(), 1);
        assert_eq!(fake.slept.get(), 100);
    }

    #[test]
    fn a_started_server_with_an_old_protocol_is_an_error() {
        let fake = Fake::new(2, up(Some(21)));
        let err = run(&fake, true).unwrap_err();
        assert_eq!(
            err,
            StartError::ProtocolTooOld {
                version: "0.9.5".into(),
                protocol: 21
            }
        );
        assert!(err.to_string().contains("protocol 21"));
    }

    #[test]
    fn the_frontend_status_names_the_state_and_reason() {
        let ok: BackendStatus = Ok(Outcome::Started).into();
        assert_eq!((ok.state, ok.reason), ("started", None));
        let off: BackendStatus = Ok(Outcome::Disabled).into();
        assert_eq!(off.state, "disabled");
        let old: BackendStatus = Ok(Outcome::Outdated {
            running: "0.1.0".into(),
            bundled: "0.2.0".into(),
        })
        .into();
        assert_eq!(
            serde_json::to_value(old).unwrap(),
            serde_json::json!({"state": "outdated", "reason": null, "running": "0.1.0", "bundled": "0.2.0"})
        );
        let other: BackendStatus = Ok(Outcome::Incompatible {
            running: "0.1.0".into(),
            protocol: 2,
        })
        .into();
        assert_eq!((other.state, other.protocol), ("incompatible", Some(2)));
        let bad: BackendStatus = Err(StartError::NotFound).into();
        assert_eq!(bad.state, "failed");
        assert!(bad.reason.unwrap().contains("herdr"));
        let json = serde_json::to_value(BackendStatus::from(Ok(Outcome::AlreadyRunning))).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"state": "running", "reason": null})
        );
    }

    #[test]
    fn the_real_runner_finds_a_program_on_the_login_path_or_not_at_all() {
        // `sh` (`cmd` on Windows, found through PATHEXT) exists on every supported system; this
        // only reads the PATH, it starts nothing.
        let program = if cfg!(windows) { "cmd" } else { "sh" };
        let found = gui_path::resolve_program(program, &gui_path::child_path());
        assert!(found.is_some());
    }
}
