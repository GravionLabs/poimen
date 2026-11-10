//! The `poimen-server` backend (ADR 0003, #153): spaces and terminals of the server mapped onto the
//! tree the frontend builds (`workspaces`, `tabs`, `panes`), live terminals through
//! `terminal.attach`.
//!
//! The server owns the PTYs and, since protocol 4, the tabs and split trees of each space; this
//! backend does not read them yet (#513): every terminal is its own tab, splits and tabs are the
//! UI's business. A stream is emulated: it carries the snapshot and raw PTY bytes, so
//! xterm.js tracks the modes itself (mouse, scrollback). poimen stops the server only after the user confirmed (`restart_server`, `stop_server`).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use poimen_client::agent::{self, PromptResult};
use poimen_client::protocol::{
    AgentStatus, AttachMode, DetachReason, ErrorCode, Event, PROTOCOL_VERSION, Space, Terminal,
    TerminalCreate,
};
use poimen_client::{AttachmentWriter, Client, ClientError};
use serde_json::{Value, json};

use crate::backend::{
    Backend, BackendEvent, BackendInfo, CloseCode, Mode, NewSpace, OnRecord, OpenedStream, Outcome,
    Snapshot, SplitDirection, StartError, StreamRecord, StreamRequest,
};
use crate::herdr::{AgentSession, PromptOutcome, ServerInfo};
use crate::poimen_server;

const NO_RENDERED_INPUT: &str = "xterm.js handles the mouse and the scrollback on this backend";

/// One open stream: the write half of its attachment.
struct Stream {
    writer: Mutex<AttachmentWriter>,
}

pub struct PoimenBackend {
    client: Client,
    next_id: AtomicU32,
    streams: Arc<Mutex<HashMap<u32, Arc<Stream>>>>,
}

impl PoimenBackend {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            next_id: AtomicU32::new(0),
            streams: Arc::default(),
        }
    }

    fn stream(&self, id: u32) -> Result<Arc<Stream>, String> {
        lock(&self.streams)
            .get(&id)
            .cloned()
            .ok_or_else(|| "the terminal stream is closed".to_owned())
    }

    fn terminal(&self, terminal_id: &str) -> Result<Terminal, String> {
        self.client
            .list_terminals(None)
            .map_err(text)?
            .into_iter()
            .find(|t| t.id == terminal_id)
            .ok_or_else(|| format!("terminal {terminal_id} not found"))
    }

    fn new_terminal(&self, space_id: &str, cwd: Option<&str>) -> Result<Terminal, String> {
        self.client
            .create_terminal(&TerminalCreate {
                space_id: space_id.into(),
                cwd: cwd.map(str::to_owned),
                ..TerminalCreate::default()
            })
            .map_err(text)
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn text(err: ClientError) -> String {
    err.to_string()
}

/// The tree the frontend builds from: a space is a workspace, a terminal is a tab with one pane
/// (the server has no tabs). Numbers follow the server's order. Agent status is what the agent's
/// hooks reported (`unknown` without an agent); a workspace shows its most urgent terminal and
/// carries the space's `cwd` (herdr workspaces have none).
pub fn map_snapshot(spaces: &[Space], terminals: &[Terminal]) -> Snapshot {
    let mut workspaces = Vec::new();
    let mut tabs = Vec::new();
    let mut panes = Vec::new();
    for (i, space) in spaces.iter().enumerate() {
        let space_status = terminals
            .iter()
            .filter(|t| t.space_id == space.id)
            .map(wire_status)
            .max_by_key(|s| urgency(s))
            .unwrap_or("unknown");
        let mut workspace = json!({
            "workspace_id": space.id,
            "label": space.name,
            "number": i + 1,
            "agent_status": space_status,
        });
        // The space's own directory, so a space without terminals is still found by it.
        if !space.cwd.is_empty() {
            workspace["cwd"] = json!(space.cwd);
        }
        workspaces.push(workspace);
        for (n, terminal) in terminals
            .iter()
            .filter(|t| t.space_id == space.id)
            .enumerate()
        {
            tabs.push(json!({
                "tab_id": terminal.id,
                "workspace_id": space.id,
                "label": terminal.name,
                "number": n + 1,
                "agent_status": wire_status(terminal),
            }));
            panes.push(wire_pane(terminal, &space.id));
        }
    }
    Snapshot {
        workspaces: Value::Array(workspaces),
        tabs: Value::Array(tabs),
        panes: Value::Array(panes),
    }
}

/// The pane of a terminal in the shape of herdr's `pane.list`: `agent` (the name the hooks
/// report, `claude` or `copilot`) and `agent_session.value` are set only when known.
fn wire_pane(terminal: &Terminal, space_id: &str) -> Value {
    let mut pane = json!({
        "pane_id": terminal.id,
        "tab_id": terminal.id,
        "workspace_id": space_id,
        "cwd": terminal.cwd,
        "agent_status": wire_status(terminal),
        "focused": false,
    });
    // The name of a terminal is its label only once it was renamed: the default is the directory
    // name, which the sidebar already shows for a pane without a label.
    if terminal.name != default_name(&terminal.cwd) {
        pane["label"] = json!(terminal.name);
    }
    if let Some(info) = &terminal.agent {
        if let Some(agent) = info.agent.as_deref().filter(|a| !a.is_empty()) {
            pane["agent"] = json!(agent);
        }
        if let Some(session) = &info.session_id {
            pane["agent_session"] = json!({"value": session});
        }
        if let Some(message) = &info.message {
            pane["agent_message"] = json!(message);
            pane["agent_message_at"] = json!(info.message_at);
        }
    }
    if !terminal.tokens.is_empty() {
        // The same `tokens` map herdr's `pane.list` has, so `$<name>` row tokens work unchanged.
        pane["tokens"] = json!(terminal.tokens);
    }
    pane
}

/// The `agent_status` of the frontend for a terminal.
fn wire_status(terminal: &Terminal) -> &'static str {
    terminal
        .agent
        .as_ref()
        .map_or("unknown", |a| status_name(a.status))
}

fn status_name(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "idle",
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Gone => "unknown",
    }
}

/// Orders statuses for the workspace row: blocked first, then working, done, idle.
fn urgency(status: &str) -> u8 {
    match status {
        "blocked" => 4,
        "working" => 3,
        "done" => 2,
        "idle" => 1,
        _ => 0,
    }
}

/// The name of a terminal without a label: the last segment of its directory.
fn default_name(cwd: &str) -> String {
    cwd.trim_end_matches('/')
        .rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or("terminal")
        .to_owned()
}

/// A stream record for an event of an attached connection; `None` for events that are not
/// stream records. `Closed` is always the last one.
pub fn map_event(event: Event) -> Option<StreamRecord> {
    match event {
        Event::TerminalOutput { data, .. } => Some(StreamRecord::Output { bytes: data }),
        Event::TerminalResized { cols, rows, .. } => Some(StreamRecord::Resized { cols, rows }),
        Event::TerminalDetached { reason, .. } => Some(match reason {
            DetachReason::TakenOver => StreamRecord::Closed {
                reason: "another client took over the controller".into(),
                exit_code: None,
                code: Some(CloseCode::ControllerHeld),
            },
            DetachReason::Overflow => StreamRecord::Closed {
                reason: "the terminal produced output faster than poimen could read it".into(),
                exit_code: None,
                code: None,
            },
            DetachReason::Closed => closed_silently(),
        }),
        Event::TerminalExited { exit_code, .. } => Some(StreamRecord::Closed {
            reason: match exit_code {
                Some(code) => format!("the process exited with code {code}"),
                None => "the process was killed".into(),
            },
            exit_code: exit_code.and_then(|c| i32::try_from(c).ok()),
            code: Some(CloseCode::Exited),
        }),
        _ => None,
    }
}

/// `detached`: the view closes without a banner.
fn closed_silently() -> StreamRecord {
    StreamRecord::Closed {
        reason: "detached".into(),
        exit_code: None,
        code: None,
    }
}

fn controller_held() -> StreamRecord {
    StreamRecord::Closed {
        reason: "another client controls this terminal".into(),
        exit_code: None,
        code: Some(CloseCode::ControllerHeld),
    }
}

impl Backend for PoimenBackend {
    fn ensure_running(&self, auto_start: bool) -> Result<Outcome, StartError> {
        poimen_server::ensure_running(&poimen_server::SystemRunner::new(), auto_start)
            .map_err(|e| StartError::Other(e.to_string()))
    }

    fn restart_server(&self) -> Result<Outcome, StartError> {
        poimen_server::restart(&poimen_server::SystemRunner::new())
            .map_err(|e| StartError::Other(e.to_string()))
    }

    fn stop_server(&self) -> Result<(), StartError> {
        poimen_server::stop(&poimen_server::SystemRunner::new())
            .map_err(|e| StartError::Other(e.to_string()))
    }

    fn info(&self) -> Option<BackendInfo> {
        let ping = self.client.ping().ok()?;
        Some(ServerInfo {
            version: ping.version,
            protocol: Some(ping.protocol),
            supported: PROTOCOL_VERSION,
        })
    }

    fn snapshot(&self) -> Result<Snapshot, String> {
        let spaces = self.client.list_spaces().map_err(text)?;
        let terminals = self.client.list_terminals(None).map_err(text)?;
        Ok(map_snapshot(&spaces, &terminals))
    }

    fn create_space(&self, cwd: &str, label: &str) -> Result<NewSpace, String> {
        let space = self.client.create_space(label, cwd).map_err(text)?;
        let terminal = self.new_terminal(&space.id, None)?;
        Ok(NewSpace {
            space_id: space.id,
            root_terminal: Some(terminal.id),
        })
    }

    /// The server has no focus; the UI keeps the selection.
    fn focus_space(&self, _space_id: &str) -> Result<(), String> {
        Ok(())
    }

    fn close_space(&self, space_id: &str) -> Result<(), String> {
        self.client.close_space(space_id).map_err(text)
    }

    fn rename_space(&self, space_id: &str, label: &str) -> Result<(), String> {
        self.client
            .rename_space(space_id, label)
            .map(drop)
            .map_err(text)
    }

    fn create_terminal(&self, space_id: &str, cwd: Option<&str>) -> Result<Option<String>, String> {
        Ok(Some(self.new_terminal(space_id, cwd)?.id))
    }

    /// A split is a new terminal in the space of `target`; direction and ratio are the UI's.
    fn split(
        &self,
        target: &str,
        _direction: SplitDirection,
        cwd: Option<&str>,
        _ratio: Option<f64>,
    ) -> Result<Option<String>, String> {
        let space_id = self.terminal(target)?.space_id;
        Ok(Some(self.new_terminal(&space_id, cwd)?.id))
    }

    fn close_terminal(&self, terminal_id: &str) -> Result<(), String> {
        self.client.close_terminal(terminal_id).map_err(text)
    }

    fn rename_terminal(&self, terminal_id: &str, label: Option<&str>) -> Result<(), String> {
        let name = match label.map(str::trim).filter(|l| !l.is_empty()) {
            Some(label) => label.to_owned(),
            None => default_name(&self.terminal(terminal_id)?.cwd),
        };
        self.client
            .rename_terminal(terminal_id, &name)
            .map(drop)
            .map_err(text)
    }

    fn send_input(&self, terminal_id: &str, text_: &str) -> Result<(), String> {
        self.client
            .input(terminal_id, &format!("{text_}\r"))
            .map_err(text)
    }

    /// Runs the agent in the terminal and returns when its `SessionStart` report arrives (30 s).
    /// A name renames the terminal; without one the terminal keeps its name.
    fn agent_start(
        &self,
        terminal_id: &str,
        kind: &str,
        name: Option<&str>,
        args: &[String],
    ) -> Result<(), String> {
        self.client
            .start_agent(terminal_id, kind, args, agent::START_TIMEOUT)
            .map_err(|e| e.to_string())?;
        match name.map(str::trim).filter(|n| !n.is_empty()) {
            Some(name) => self.agent_rename(terminal_id, Some(name)),
            None => Ok(()),
        }
    }

    /// The agent's name is the name of its terminal; a blank name restores the default.
    fn agent_rename(&self, target: &str, name: Option<&str>) -> Result<(), String> {
        self.rename_terminal(target, name)
    }

    fn agent_prompt(&self, target: &str, text: &str) -> PromptOutcome {
        let failed = |message: &str| PromptOutcome::Failed {
            message: message.into(),
        };
        match self.client.prompt_agent(target, text, agent::PROMPT_WAIT) {
            Ok(PromptResult::Started(status)) => PromptOutcome::Started {
                status: status_name(status).into(),
            },
            Ok(PromptResult::Stalled) => PromptOutcome::Stalled,
            Ok(PromptResult::Blocked) => PromptOutcome::Blocked,
            Ok(PromptResult::Working) => failed("the agent is working; wait until it is idle"),
            Ok(PromptResult::NoAgent) => failed("no agent reported in this terminal"),
            Err(e) => failed(&e.to_string()),
        }
    }

    fn agent_session(&self, terminal_id: &str) -> Result<Option<AgentSession>, String> {
        let terminals = self.client.list_terminals(None).map_err(text)?;
        let panes: Vec<Value> = terminals
            .iter()
            .map(|t| wire_pane(t, &t.space_id))
            .collect();
        Ok(crate::herdr::parse_agent_session(
            &json!({"panes": panes}),
            terminal_id,
        ))
    }

    fn open_stream(
        &self,
        request: StreamRequest<'_>,
        on_record: OnRecord,
    ) -> Result<OpenedStream, String> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed) + 1;
        let opened = OpenedStream { id, emulated: true };
        let mode = match request.mode {
            Mode::Observe => AttachMode::Observe,
            Mode::Control => AttachMode::Control,
        };
        let mut attachment = match self.client.attach(request.target, mode, request.takeover) {
            Ok(attachment) => attachment,
            // Another client controls the terminal: the view falls back to observing.
            Err(ClientError::Server {
                code: ErrorCode::Conflict,
                ..
            }) => {
                on_record(id, controller_held());
                return Ok(opened);
            }
            Err(err) => return Err(text(err)),
        };
        let writer = attachment.writer().map_err(text)?;
        lock(&self.streams).insert(
            id,
            Arc::new(Stream {
                writer: Mutex::new(writer),
            }),
        );
        let streams = Arc::clone(&self.streams);
        let info = attachment.info.clone();
        let controls = request.mode == Mode::Control;
        std::thread::spawn(move || {
            on_record(
                id,
                StreamRecord::Reset {
                    cols: info.cols,
                    rows: info.rows,
                    bytes: info.snapshot,
                },
            );
            if controls {
                on_record(id, StreamRecord::Controller { attached: true });
            }
            let last = loop {
                match attachment.next_record() {
                    Ok(event) => {
                        if let Some(record) = map_event(event) {
                            if matches!(record, StreamRecord::Closed { .. }) {
                                break record;
                            }
                            on_record(id, record);
                        }
                    }
                    // Closed by poimen (the writer shut the socket down) or the server went away.
                    Err(err) => {
                        break StreamRecord::Closed {
                            reason: err.to_string(),
                            exit_code: None,
                            code: None,
                        };
                    }
                }
            };
            lock(&streams).remove(&id);
            on_record(id, last);
        });
        Ok(opened)
    }

    fn emulated(&self) -> bool {
        true
    }

    fn stream_input(&self, id: u32, text_: &str) -> Result<(), String> {
        let stream = self.stream(id)?;
        lock(&stream.writer).input(text_).map_err(text)
    }

    fn stream_resize(&self, id: u32, cols: u16, rows: u16) -> Result<(), String> {
        let stream = self.stream(id)?;
        lock(&stream.writer).resize(cols, rows).map_err(text)
    }

    fn stream_scroll(&self, _id: u32, _up: bool, _lines: u16) -> Result<(), String> {
        Err(NO_RENDERED_INPUT.into())
    }

    fn stream_mouse(
        &self,
        _id: u32,
        _action: &str,
        _button: &str,
        _column: u16,
        _row: u16,
        _modifiers: u8,
    ) -> Result<(), String> {
        Err(NO_RENDERED_INPUT.into())
    }

    /// Detaches; the terminal keeps running.
    fn close_stream(&self, id: u32) {
        if let Some(stream) = lock(&self.streams).remove(&id) {
            lock(&stream.writer).close();
        }
    }

    fn close_streams(&self) {
        for (_, stream) in lock(&self.streams).drain() {
            lock(&stream.writer).close();
        }
    }

    fn watch(&self, emit: &dyn Fn(&BackendEvent)) {
        let synthetic = |kind: &str, data: Value| BackendEvent {
            kind: kind.into(),
            data,
        };
        let mut connected = None;
        loop {
            match self
                .client
                .subscribe_as(Some(poimen_client::protocol::ClientInfo {
                    kind: poimen_client::protocol::ClientKind::App,
                    pid: std::process::id(),
                })) {
                Ok(events) => {
                    if connected != Some(true) {
                        emit(&synthetic("poimen.connected", Value::Null));
                    }
                    connected = Some(true);
                    // Something may have changed before the subscription was in place.
                    emit(&synthetic("poimen.resync", Value::Null));
                    for event in events {
                        match event {
                            Ok(event) => emit(&backend_event(&event)),
                            Err(err) => {
                                emit(&synthetic(
                                    "poimen.resync",
                                    json!({"reason": err.to_string()}),
                                ));
                                break;
                            }
                        }
                    }
                }
                Err(err) => {
                    if connected != Some(false) {
                        emit(&synthetic(
                            "poimen.disconnected",
                            json!({"reason": err.to_string()}),
                        ));
                    }
                    connected = Some(false);
                }
            }
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

/// `{"event": kind, "data": ...}` of the wire form as a backend event.
fn backend_event(event: &Event) -> BackendEvent {
    // The frontend applies status changes from herdr's event, keyed by pane.
    if let Event::AgentStatusChanged {
        terminal_id,
        status,
        message,
        message_at,
        ..
    } = event
    {
        let mut data = json!({"pane_id": terminal_id, "agent_status": status_name(*status)});
        if let Some(message) = message {
            data["agent_message"] = json!(message);
            data["agent_message_at"] = json!(message_at);
        }
        return BackendEvent {
            kind: "pane.agent_status_changed".into(),
            data,
        };
    }
    let mut value = serde_json::to_value(event).unwrap_or(Value::Null);
    BackendEvent {
        kind: value["event"].as_str().unwrap_or("unknown").to_owned(),
        data: value["data"].take(),
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::path::PathBuf;
    use std::sync::mpsc;

    use poimen_client::protocol::{AgentInfo, TerminalStatus};

    fn space(id: &str, name: &str) -> Space {
        Space {
            id: id.into(),
            name: name.into(),
            cwd: format!("/work/{name}"),
            ..Default::default()
        }
    }

    fn terminal(id: &str, space_id: &str, name: &str) -> Terminal {
        Terminal {
            id: id.into(),
            space_id: space_id.into(),
            name: name.into(),
            cwd: format!("/work/{name}"),
            cols: 80,
            rows: 24,
            status: TerminalStatus::Running,
            exit_code: None,
            program: None,
            agent: None,
            tokens: Default::default(),
        }
    }

    #[test]
    fn the_snapshot_maps_spaces_to_workspaces_and_terminals_to_tabs_with_one_pane() {
        let snapshot = map_snapshot(
            &[space("s1", "api"), space("s2", "web")],
            &[
                terminal("t1", "s1", "zsh"),
                terminal("t3", "s2", "dev"),
                terminal("t2", "s1", "tests"),
            ],
        );
        assert_eq!(
            snapshot.workspaces,
            json!([
                {"workspace_id": "s1", "label": "api", "number": 1, "agent_status": "unknown", "cwd": "/work/api"},
                {"workspace_id": "s2", "label": "web", "number": 2, "agent_status": "unknown", "cwd": "/work/web"},
            ])
        );
        assert_eq!(
            snapshot.tabs,
            json!([
                {"tab_id": "t1", "workspace_id": "s1", "label": "zsh", "number": 1, "agent_status": "unknown"},
                {"tab_id": "t2", "workspace_id": "s1", "label": "tests", "number": 2, "agent_status": "unknown"},
                {"tab_id": "t3", "workspace_id": "s2", "label": "dev", "number": 1, "agent_status": "unknown"},
            ])
        );
        assert_eq!(
            snapshot.panes[1],
            json!({
                "pane_id": "t2", "tab_id": "t2", "workspace_id": "s1", "cwd": "/work/tests",
                "agent_status": "unknown", "focused": false,
            })
        );
    }

    #[test]
    fn an_empty_server_gives_an_empty_snapshot() {
        let snapshot = map_snapshot(&[], &[]);
        assert_eq!(snapshot.workspaces, json!([]));
        assert_eq!(snapshot.panes, json!([]));
    }

    #[test]
    fn a_space_without_terminals_keeps_its_directory() {
        let snapshot = map_snapshot(&[space("s1", "app-7")], &[]);
        assert_eq!(snapshot.workspaces[0]["cwd"], "/work/app-7");
        assert_eq!(snapshot.panes, json!([]));
    }

    #[test]
    fn a_space_without_a_directory_has_no_cwd() {
        let mut s = space("s1", "a");
        s.cwd = String::new();
        let snapshot = map_snapshot(&[s], &[]);
        assert!(snapshot.workspaces[0].get("cwd").is_none());
    }

    #[test]
    fn a_terminal_of_an_unknown_space_is_left_out() {
        let snapshot = map_snapshot(&[space("s1", "a")], &[terminal("t1", "gone", "x")]);
        assert_eq!(snapshot.tabs, json!([]));
    }

    #[test]
    fn exited_terminals_stay_in_the_tree() {
        let mut t = terminal("t1", "s1", "zsh");
        t.status = TerminalStatus::Exited;
        t.exit_code = Some(1);
        assert_eq!(
            map_snapshot(&[space("s1", "a")], &[t]).panes[0]["pane_id"],
            "t1"
        );
    }

    #[test]
    fn default_names_come_from_the_directory() {
        assert_eq!(default_name("/home/me/api"), "api");
        assert_eq!(default_name("/home/me/api/"), "api");
        assert_eq!(default_name("/"), "terminal");
        assert_eq!(default_name(""), "terminal");
    }

    fn id() -> String {
        "t1".into()
    }

    #[test]
    fn output_and_resize_records_map_to_stream_records() {
        assert_eq!(
            map_event(Event::TerminalOutput {
                terminal_id: id(),
                seq: 1,
                data: "YQ==".into()
            }),
            Some(StreamRecord::Output {
                bytes: "YQ==".into()
            })
        );
        assert_eq!(
            map_event(Event::TerminalResized {
                terminal_id: id(),
                seq: 2,
                cols: 100,
                rows: 30
            }),
            Some(StreamRecord::Resized {
                cols: 100,
                rows: 30
            })
        );
    }

    #[test]
    fn a_takeover_is_a_held_controller_and_a_close_is_silent() {
        let taken = map_event(Event::TerminalDetached {
            terminal_id: id(),
            reason: DetachReason::TakenOver,
        });
        assert!(matches!(
            taken,
            Some(StreamRecord::Closed {
                code: Some(CloseCode::ControllerHeld),
                ..
            })
        ));
        assert_eq!(
            map_event(Event::TerminalDetached {
                terminal_id: id(),
                reason: DetachReason::Closed
            }),
            Some(closed_silently())
        );
        let overflow = map_event(Event::TerminalDetached {
            terminal_id: id(),
            reason: DetachReason::Overflow,
        });
        assert!(matches!(
            overflow,
            Some(StreamRecord::Closed { code: None, .. })
        ));
    }

    #[test]
    fn an_exit_carries_the_code_and_marks_the_close() {
        let record = map_event(Event::TerminalExited {
            terminal_id: id(),
            exit_code: Some(3),
        });
        assert_eq!(
            record,
            Some(StreamRecord::Closed {
                reason: "the process exited with code 3".into(),
                exit_code: Some(3),
                code: Some(CloseCode::Exited),
            })
        );
        let killed = map_event(Event::TerminalExited {
            terminal_id: id(),
            exit_code: None,
        });
        assert!(matches!(
            killed,
            Some(StreamRecord::Closed {
                exit_code: None,
                code: Some(CloseCode::Exited),
                ..
            })
        ));
    }

    #[test]
    fn events_that_are_not_stream_records_are_skipped() {
        assert_eq!(
            map_event(Event::TerminalControllerAttached { terminal_id: id() }),
            None
        );
        assert_eq!(
            map_event(Event::SpaceClosed {
                space_id: "s1".into()
            }),
            None
        );
    }

    #[test]
    fn hook_reported_status_reaches_panes_tabs_and_the_workspace() {
        let mut working = terminal("t1", "s1", "a");
        working.agent = Some(AgentInfo {
            status: AgentStatus::Working,
            agent: Some("claude".into()),
            session_id: None,
            message: None,
            message_at: None,
        });
        let mut blocked = terminal("t2", "s1", "b");
        blocked.agent = Some(AgentInfo {
            status: AgentStatus::Blocked,
            agent: None,
            session_id: None,
            message: None,
            message_at: None,
        });
        let snapshot = map_snapshot(
            &[space("s1", "api"), space("s2", "web")],
            &[working, blocked, terminal("t3", "s2", "c")],
        );
        let status = |v: &Value| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|x| x["agent_status"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(status(&snapshot.panes), ["working", "blocked", "unknown"]);
        assert_eq!(status(&snapshot.tabs), ["working", "blocked", "unknown"]);
        assert_eq!(status(&snapshot.workspaces), ["blocked", "unknown"]);
    }

    #[test]
    fn an_agent_status_event_becomes_the_pane_event_the_frontend_applies() {
        let changed = |status| {
            backend_event(&Event::AgentStatusChanged {
                terminal_id: "t1".into(),
                status,
                agent: Some("claude".into()),
                session_id: None,
                message: None,
                message_at: None,
            })
        };
        let event = changed(AgentStatus::Blocked);
        assert_eq!(event.kind, "pane.agent_status_changed");
        assert_eq!(
            event.data,
            json!({"pane_id": "t1", "agent_status": "blocked"})
        );
        assert_eq!(changed(AgentStatus::Gone).data["agent_status"], "unknown");
    }

    #[test]
    fn the_notification_text_reaches_the_pane_and_its_event() {
        let event = backend_event(&Event::AgentStatusChanged {
            terminal_id: "t1".into(),
            status: AgentStatus::Blocked,
            agent: None,
            session_id: None,
            message: Some("Allow Bash?".into()),
            message_at: Some(1700),
        });
        assert_eq!(
            event.data,
            json!({"pane_id": "t1", "agent_status": "blocked",
                "agent_message": "Allow Bash?", "agent_message_at": 1700})
        );
        let mut t = terminal("t1", "s1", "a");
        t.agent = Some(AgentInfo {
            status: AgentStatus::Blocked,
            agent: None,
            session_id: None,
            message: Some("Allow Bash?".into()),
            message_at: Some(1700),
        });
        let snapshot = map_snapshot(&[space("s1", "api")], &[t, terminal("t2", "s1", "b")]);
        assert_eq!(snapshot.panes[0]["agent_message"], "Allow Bash?");
        assert_eq!(snapshot.panes[0]["agent_message_at"], 1700);
        assert!(snapshot.panes[1].get("agent_message").is_none());
    }

    #[test]
    fn tokens_reach_the_pane_as_herdr_tokens_and_their_event_passes_through() {
        let mut t = terminal("t1", "s1", "a");
        t.tokens = [("deliver_step".to_owned(), "build".to_owned())].into();
        let snapshot = map_snapshot(&[space("s1", "api")], &[t, terminal("t2", "s1", "b")]);
        assert_eq!(
            snapshot.panes[0]["tokens"],
            json!({"deliver_step": "build"})
        );
        assert!(snapshot.panes[1].get("tokens").is_none());
        let event = backend_event(&Event::TerminalTokensChanged {
            terminal_id: "t1".into(),
            tokens: Default::default(),
        });
        assert_eq!(event.kind, "terminal.tokens_changed");
        assert_eq!(event.data["terminal_id"], "t1");
    }

    #[test]
    fn a_wire_event_becomes_a_backend_event() {
        let event = backend_event(&Event::SpaceClosed {
            space_id: "s1".into(),
        });
        assert_eq!(event.kind, "space.closed");
        assert_eq!(event.data, json!({"space_id": "s1"}));
    }

    // --- against a scripted server on a temporary socket (never a real poimen-server) ---------

    fn temp_socket(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("poimen-backend-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::set_permissions(&dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .unwrap();
        dir.join("s.sock")
    }

    /// Answers requests with `reply(method, params)`; `Err` is an error response. A connection
    /// stays open until the client closes it. Requests are logged as `method params`.
    fn serve(
        path: &std::path::Path,
        log: Arc<Mutex<Vec<String>>>,
        reply: impl Fn(&str, &Value) -> Result<Value, (&'static str, &'static str)>
        + Send
        + Sync
        + 'static,
        extra_lines: Vec<String>,
    ) {
        let listener = poimen_client::transport::Listener::bind(path).unwrap();
        let reply = Arc::new(reply);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let (log, reply, extra) = (log.clone(), reply.clone(), extra_lines.clone());
                std::thread::spawn(move || {
                    let mut reader = BufReader::new(stream.clone());
                    let mut line = String::new();
                    while reader.read_line(&mut line).unwrap_or(0) > 0 {
                        let request: Value = serde_json::from_str(line.trim()).unwrap();
                        let (method, params) = (
                            request["method"].as_str().unwrap().to_owned(),
                            request["params"].clone(),
                        );
                        lock(&log).push(format!("{method} {params}"));
                        let response = match reply(&method, &params) {
                            Ok(result) => json!({"id": request["id"], "result": result}),
                            Err((code, message)) => {
                                json!({"id": request["id"], "error": {"code": code, "message": message}})
                            }
                        };
                        writeln!(stream, "{response}").unwrap();
                        if method == "terminal.attach" {
                            for line in &extra {
                                writeln!(stream, "{line}").unwrap();
                            }
                        }
                        line.clear();
                    }
                });
            }
        });
    }

    fn term_json(id: &str, space: &str) -> Value {
        json!({"id": id, "space_id": space, "name": "zsh", "cwd": "/work/api", "cols": 80, "rows": 24, "status": "running"})
    }

    fn backend_for(
        name: &str,
        extra: Vec<String>,
        conflict: bool,
    ) -> (PoimenBackend, Arc<Mutex<Vec<String>>>) {
        let path = temp_socket(name);
        let log = Arc::new(Mutex::new(Vec::new()));
        serve(
            &path,
            log.clone(),
            move |method, params| match method {
                "space.create" => {
                    Ok(json!({"space": {"id": "s9", "name": params["name"], "cwd": params["cwd"]}}))
                }
                "terminal.create" => {
                    Ok(json!({"terminal": term_json("t9", params["space_id"].as_str().unwrap())}))
                }
                "terminal.list" => Ok(json!({"terminals": [term_json("t1", "s1")]})),
                "terminal.rename" => Ok(json!({"terminal": term_json("t1", "s1")})),
                "terminal.input" | "terminal.close" | "space.close" => Ok(json!({})),
                "ping" => Ok(json!({"version": "0.9.0", "protocol": PROTOCOL_VERSION, "pid": 1})),
                "terminal.attach" if conflict => Err(("conflict", "controlled")),
                "terminal.attach" => Ok(
                    json!({"terminal_id": params["terminal_id"], "seq": 0, "cols": 100, "rows": 30, "mode": params["mode"], "snapshot": "c25hcA=="}),
                ),
                _ => Err(("unknown_method", "no")),
            },
            extra,
        );
        (PoimenBackend::new(Client::new(path)), log)
    }

    fn params_of(log: &Arc<Mutex<Vec<String>>>, method: &str) -> Value {
        let line = lock(log)
            .iter()
            .find(|l| l.starts_with(method))
            .cloned()
            .unwrap();
        serde_json::from_str(&line[method.len() + 1..]).unwrap()
    }

    #[test]
    fn a_new_space_gets_its_first_terminal() {
        let (backend, log) = backend_for("space", vec![], false);
        let created = backend.create_space("/work/api", "api").unwrap();
        assert_eq!(created.space_id, "s9");
        assert_eq!(created.root_terminal.as_deref(), Some("t9"));
        assert_eq!(params_of(&log, "terminal.create")["space_id"], "s9");
    }

    #[test]
    fn a_split_creates_a_terminal_in_the_space_of_the_target() {
        let (backend, log) = backend_for("split", vec![], false);
        let id = backend
            .split("t1", SplitDirection::Right, Some("/work/api"), Some(0.5))
            .unwrap();
        assert_eq!(id.as_deref(), Some("t9"));
        let params = params_of(&log, "terminal.create");
        assert_eq!(params["space_id"], "s1");
        assert_eq!(params["cwd"], "/work/api");
    }

    #[test]
    fn a_split_of_an_unknown_terminal_fails_without_creating() {
        let (backend, log) = backend_for("split-missing", vec![], false);
        let err = backend
            .split("nope", SplitDirection::Down, None, None)
            .unwrap_err();
        assert!(err.contains("nope"));
        assert!(!lock(&log).iter().any(|l| l.starts_with("terminal.create")));
    }

    #[test]
    fn sent_text_ends_with_enter_and_a_blank_label_falls_back_to_the_directory() {
        let (backend, log) = backend_for("input", vec![], false);
        backend.send_input("t1", "claude").unwrap();
        assert_eq!(params_of(&log, "terminal.input")["data"], "claude\r");
        backend.rename_terminal("t1", Some("  ")).unwrap();
        assert_eq!(params_of(&log, "terminal.rename")["name"], "api");
    }

    #[test]
    fn info_reports_the_server_version_and_protocol() {
        let (backend, _) = backend_for("info", vec![], false);
        let info = backend.info().unwrap();
        assert_eq!(
            (info.version.as_str(), info.protocol, info.supported),
            ("0.9.0", Some(4), 4)
        );
    }

    /// A server whose terminal t1 has an agent with `status` (none: no agent).
    fn agent_backend(
        name: &str,
        status: Option<&'static str>,
    ) -> (PoimenBackend, Arc<Mutex<Vec<String>>>) {
        let path = temp_socket(name);
        let log = Arc::new(Mutex::new(Vec::new()));
        serve(
            &path,
            log.clone(),
            move |method, _| match method {
                "terminal.list" => {
                    let mut t = term_json("t1", "s1");
                    if let Some(status) = status {
                        t["agent"] = json!({"status": status, "agent": "claude"});
                    }
                    Ok(json!({"terminals": [t]}))
                }
                "terminal.input" => Ok(json!({})),
                "terminal.rename" => Ok(json!({"terminal": term_json("t1", "s1")})),
                _ => Err(("unknown_method", "no")),
            },
            vec![],
        );
        (PoimenBackend::new(Client::new(path)), log)
    }

    fn typed(log: &Arc<Mutex<Vec<String>>>) -> bool {
        lock(log).iter().any(|l| l.starts_with("terminal.input"))
    }

    #[test]
    fn a_prompt_is_refused_while_the_agent_works_or_is_blocked_and_nothing_is_typed() {
        for (status, expected) in [("working", "working"), ("blocked", "blocked")] {
            let (backend, log) = agent_backend(&format!("prompt-{status}"), Some(status));
            let outcome = backend.agent_prompt("t1", "hi");
            match status {
                "blocked" => assert_eq!(outcome, PromptOutcome::Blocked),
                _ => assert!(
                    matches!(&outcome, PromptOutcome::Failed { message } if message.contains(expected))
                ),
            }
            assert!(!typed(&log));
        }
    }

    #[test]
    fn a_prompt_without_an_agent_fails_and_types_nothing() {
        let (backend, log) = agent_backend("prompt-none", None);
        assert!(matches!(
            backend.agent_prompt("t1", "hi"),
            PromptOutcome::Failed { .. }
        ));
        assert!(!typed(&log));
    }

    #[test]
    fn a_prompt_to_an_idle_agent_is_typed_with_enter() {
        let (backend, log) = agent_backend("prompt-idle", Some("idle"));
        // The fake never reports work, so the wait ends; the text was typed all the same.
        let _ = backend
            .client
            .prompt_agent("t1", "hi", Duration::from_millis(100));
        assert_eq!(params_of(&log, "terminal.input")["data"], "hi\r");
    }

    #[test]
    fn starting_in_a_terminal_that_has_an_agent_is_refused_untouched() {
        let (backend, log) = agent_backend("start-busy", Some("idle"));
        let err = backend.agent_start("t1", "claude", None, &[]).unwrap_err();
        assert!(err.contains("already has an agent"));
        assert!(!typed(&log));
    }

    #[test]
    fn the_agent_name_is_the_terminal_name() {
        let (backend, log) = agent_backend("agent-rename", Some("idle"));
        backend.agent_rename("t1", Some(" review ")).unwrap();
        assert_eq!(params_of(&log, "terminal.rename")["name"], "review");
        let (backend, log) = agent_backend("agent-rename-blank", Some("idle"));
        backend.agent_rename("t1", None).unwrap();
        assert_eq!(params_of(&log, "terminal.rename")["name"], "api");
    }

    #[test]
    fn a_renamed_agent_pane_shows_its_name_in_the_snapshot() {
        let path = temp_socket("agent-rename-snapshot");
        let log = Arc::new(Mutex::new(Vec::new()));
        // The server keeps the name a rename sets, as it does for the next snapshot.
        let name = Arc::new(Mutex::new("api".to_owned()));
        let shared = name.clone();
        serve(
            &path,
            log.clone(),
            move |method, params| match method {
                "space.list" => {
                    Ok(json!({"spaces": [{"id": "s1", "name": "api", "cwd": "/work/api"}]}))
                }
                "terminal.list" => {
                    let mut t = term_json("t1", "s1");
                    t["name"] = json!(lock(&shared).clone());
                    t["agent"] = json!({"status": "idle", "agent": "claude"});
                    Ok(json!({"terminals": [t]}))
                }
                "terminal.rename" => {
                    *lock(&shared) = params["name"].as_str().unwrap().to_owned();
                    Ok(json!({"terminal": term_json("t1", "s1")}))
                }
                _ => Err(("unknown_method", "no")),
            },
            vec![],
        );
        let backend = PoimenBackend::new(Client::new(path));
        let pane = |backend: &PoimenBackend| backend.snapshot().unwrap().panes[0].clone();
        assert_eq!(pane(&backend)["agent"], "claude");
        assert!(
            pane(&backend).get("label").is_none(),
            "default name is not a label"
        );
        backend.agent_rename("t1", Some(" review ")).unwrap();
        assert_eq!(pane(&backend)["label"], "review");
        assert_eq!(pane(&backend)["agent"], "claude");
        backend.agent_rename("t1", None).unwrap();
        assert!(
            pane(&backend).get("label").is_none(),
            "a cleared name falls back"
        );
    }

    #[test]
    fn the_agent_session_is_the_id_and_directory_the_hooks_reported() {
        let path = temp_socket("session");
        let log = Arc::new(Mutex::new(Vec::new()));
        serve(
            &path,
            log,
            |method, _| match method {
                "terminal.list" => {
                    let mut a = term_json("t1", "s1");
                    a["agent"] =
                        json!({"status": "idle", "agent": "claude", "session_id": "abc-1"});
                    let mut b = term_json("t2", "s1");
                    b["agent"] =
                        json!({"status": "idle", "agent": "copilot", "session_id": "0b7e"});
                    let mut c = term_json("t3", "s1");
                    c["agent"] = json!({"status": "idle", "agent": "claude"});
                    Ok(json!({"terminals": [a, b, c, term_json("t4", "s1")]}))
                }
                _ => Err(("unknown_method", "no")),
            },
            vec![],
        );
        let backend = PoimenBackend::new(Client::new(path));
        let claude = backend.agent_session("t1").unwrap().unwrap();
        assert_eq!(
            (claude.agent, claude.session.as_str()),
            (crate::herdr::TranscriptAgent::Claude, "abc-1")
        );
        assert!(!claude.cwd.is_empty());
        let copilot = backend.agent_session("t2").unwrap().unwrap();
        assert_eq!(
            (copilot.agent, copilot.session.as_str()),
            (crate::herdr::TranscriptAgent::Copilot, "0b7e")
        );
        assert_eq!(backend.agent_session("t3"), Ok(None));
        assert_eq!(backend.agent_session("t4"), Ok(None));
    }

    #[test]
    fn a_pane_carries_the_agent_and_session_only_when_reported() {
        let mut with = terminal("t1", "s1", "zsh");
        with.agent = Some(AgentInfo {
            status: AgentStatus::Idle,
            agent: Some("copilot".into()),
            session_id: Some("s-9".into()),
            message: None,
            message_at: None,
        });
        let plain = terminal("t2", "s1", "zsh");
        let panes = map_snapshot(&[space("s1", "api")], &[with, plain]).panes;
        assert_eq!(panes[0]["agent"], "copilot");
        assert_eq!(panes[0]["agent_session"]["value"], "s-9");
        assert!(panes[1].get("agent").is_none());
        assert!(panes[1].get("agent_session").is_none());
    }

    fn collect(
        backend: &PoimenBackend,
        mode: Mode,
        takeover: bool,
    ) -> (
        Result<OpenedStream, String>,
        mpsc::Receiver<(u32, StreamRecord)>,
    ) {
        let (tx, rx) = mpsc::channel();
        let opened = backend.open_stream(
            StreamRequest {
                mode,
                target: "t1",
                cols: 80,
                rows: 24,
                takeover,
            },
            Box::new(move |id, record| {
                let _ = tx.send((id, record));
            }),
        );
        (opened, rx)
    }

    #[test]
    fn a_controller_stream_starts_with_the_snapshot_then_streams_until_the_exit() {
        let extra = vec![
            r#"{"event":"terminal.output","data":{"terminal_id":"t1","seq":1,"data":"YQ=="}}"#.into(),
            r#"{"event":"terminal.resized","data":{"terminal_id":"t1","seq":2,"cols":90,"rows":20}}"#.into(),
            r#"{"event":"terminal.exited","data":{"terminal_id":"t1","exit_code":0}}"#.into(),
        ];
        let (backend, log) = backend_for("stream", extra, false);
        let (opened, rx) = collect(&backend, Mode::Control, false);
        let opened = opened.unwrap();
        assert!(opened.emulated);
        let records: Vec<_> = rx.iter().map(|(_, r)| r).collect();
        assert_eq!(
            records,
            [
                StreamRecord::Reset {
                    cols: 100,
                    rows: 30,
                    bytes: "c25hcA==".into()
                },
                StreamRecord::Controller { attached: true },
                StreamRecord::Output {
                    bytes: "YQ==".into()
                },
                StreamRecord::Resized { cols: 90, rows: 20 },
                StreamRecord::Closed {
                    reason: "the process exited with code 0".into(),
                    exit_code: Some(0),
                    code: Some(CloseCode::Exited),
                },
            ]
        );
        let attach = params_of(&log, "terminal.attach");
        assert_eq!(
            (attach["mode"].as_str(), attach["take_over"].as_bool()),
            (Some("control"), Some(false))
        );
    }

    #[test]
    fn an_observer_gets_no_controller_record_and_input_goes_through_the_attachment() {
        let (backend, log) = backend_for("observer", vec![], false);
        let (opened, rx) = collect(&backend, Mode::Observe, false);
        let id = opened.unwrap().id;
        assert!(matches!(rx.recv().unwrap().1, StreamRecord::Reset { .. }));
        backend.stream_input(id, "x").unwrap();
        backend.stream_resize(id, 50, 10).unwrap();
        // Closing detaches and ends the reader with a last record.
        backend.close_stream(id);
        assert!(matches!(rx.recv().unwrap().1, StreamRecord::Closed { .. }));
        assert!(backend.stream_input(id, "y").is_err());
        let _ = log;
    }

    #[test]
    fn a_held_controller_is_reported_as_a_closed_record_not_an_error() {
        let (backend, _) = backend_for("held", vec![], true);
        let (opened, rx) = collect(&backend, Mode::Control, false);
        assert!(opened.unwrap().emulated);
        let (_, record) = rx.recv().unwrap();
        assert!(matches!(
            record,
            StreamRecord::Closed {
                code: Some(CloseCode::ControllerHeld),
                ..
            }
        ));
    }

    #[test]
    fn scroll_and_mouse_commands_are_refused() {
        let (backend, _) = backend_for("refuse", vec![], false);
        assert!(backend.stream_scroll(1, true, 3).is_err());
        assert!(backend.stream_mouse(1, "down", "left", 0, 0, 0).is_err());
    }
}
