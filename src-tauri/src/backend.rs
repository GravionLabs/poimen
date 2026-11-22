//! The terminal backend poimen talks to, and starting it when it is not running (ADR 0003).
//!
//! poimen never owns pane state and stops the server only on a confirmed restart or quit: `ensure_running` only starts a missing
//! server, detached from poimen, and waits until it answers. A server that already answers (also
//! one started by another client) is left alone. `PoimenBackend` is the only implementation; the
//! trait is the seam the tests use (`backend_fake.rs`).

use std::fmt;

use serde::Serialize;
use serde_json::Value;

pub use crate::agent_session::{AgentSession, PromptOutcome};

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
    /// The start failed; the text is the whole message.
    Other(String),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
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

/// Version and protocol of the running backend, from the `ping` reply.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct BackendInfo {
    pub version: String,
    /// `None` when the server reports a version but no protocol: shown, but not checked.
    pub protocol: Option<u32>,
    /// The protocol poimen supports, so the frontend can flag a mismatch.
    pub supported: u32,
}

/// Spaces, tabs and terminals in one read, in the backend-neutral shape (#586) every backend maps
/// its own model onto; the wire shape of the `snapshot` command.
pub use poimen_core::snapshot::Snapshot;

/// A space whose layout the backend resolves and builds itself (poimen-server's `space.create`).
#[derive(Debug, Clone, Copy)]
pub struct LayoutRequest<'a> {
    pub cwd: &'a str,
    pub label: &'a str,
    /// The user's answer to an earlier [`LaidOut::Trust`]; `None` on the first send.
    pub decision: Option<&'a crate::trust::Decision>,
    /// Replaces the command of the layout's root pane (a task's command).
    pub command: Option<&'a str>,
    /// Extra environment of the new terminals (a task's `POIMEN_ISSUE` and `POIMEN_BRANCH`).
    pub env: Option<&'a std::collections::BTreeMap<String, String>>,
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
/// backend (poimen-server's `agent.status_changed` is mapped onto it).
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
    pub takeover: bool,
}

/// An opened stream. It carries raw PTY bytes: xterm.js tracks the terminal modes itself, so the
/// mouse and the wheel reach the application as escape sequences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct OpenedStream {
    pub id: u32,
}

/// Receives the records of a stream, from a reader thread, with the stream id.
pub type OnRecord = Box<dyn Fn(u32, StreamRecord) + Send + 'static>;

/// A terminal backend: what the commands of poimen need from the server. `PoimenBackend` is the
/// only implementation; the trait is the seam of the tests (`backend_fake.rs`). Errors are text
/// for the user.
pub trait Backend: Send + Sync {
    /// `agents.claude.classicRenderer`: while on, terminals created afterwards get
    /// `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1`. Running terminals keep their environment. Only
    /// the poimen backend supports it; the default ignores it.
    fn set_claude_classic_renderer(&self, _on: bool) {}
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
    /// Creates a space with the layout the backend resolves and builds itself, asking for trust
    /// in the project file first; `None` when the backend does not build layouts (the fake of the
    /// tests).
    fn create_space_with_layout(
        &self,
        _request: LayoutRequest<'_>,
    ) -> Option<Result<LaidOut, String>> {
        None
    }
    /// Builds a layout into an existing space as new tabs, asking for trust in the project file
    /// first; `None` when the backend does not build layouts (the fake of the
    /// tests).
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
    /// The user's answer to an agent's request to write into another space
    /// (`agent.confirm_requested`, #615). Only the poimen backend has such requests.
    fn agent_confirm_answer(&self, _id: &str, _approve: bool) -> Result<(), String> {
        Err("this backend has no confirmations".into())
    }
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
    fn stream_input(&self, id: u32, text: &str) -> Result<(), String>;
    fn stream_resize(&self, id: u32, cols: u16, rows: u16) -> Result<(), String>;
    /// Ends one stream (releases a held controller); the terminal keeps running.
    fn close_stream(&self, id: u32);
    fn close_streams(&self);
    /// Subscribes to events and calls `emit` for each one, reconnecting when the connection is
    /// lost. Runs forever on the calling thread.
    fn watch(&self, emit: &dyn Fn(&BackendEvent));
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
        let bad: BackendStatus = Err(StartError::Other("no luck".into())).into();
        assert_eq!(bad.state, "failed");
        assert_eq!(bad.reason.as_deref(), Some("no luck"));
        let json = serde_json::to_value(BackendStatus::from(Ok(Outcome::AlreadyRunning))).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"state": "running", "reason": null})
        );
    }
}
