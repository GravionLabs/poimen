//! The `poimen-server` backend (ADR 0003, #153): spaces, tabs and terminals of the server in the
//! backend-neutral snapshot (`poimen_core::snapshot`, #586), live terminals through
//! `terminal.attach`.
//!
//! The server owns the PTYs and, since protocol 4, the tabs and split trees of each space (#513):
//! the snapshot maps its tabs with their trees, a split is `tab.split`, a new space is
//! `space.create` with the layout the server resolves and builds (and its trust question).
//! A stream is emulated: it carries the snapshot and raw PTY bytes, so
//! xterm.js tracks the modes itself (mouse, scrollback). poimen stops the server only after the user confirmed (`restart_server`, `stop_server`).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use poimen_client::agent::{self, PromptResult};
use poimen_client::protocol::{
    self as wire, AgentStatus, AttachMode, DetachReason, ErrorCode, Event, Node, PROTOCOL_VERSION,
    Space, SpaceApplyLayout, SpaceApplyLayoutResult, SpaceCreate, SpaceCreateResult, SpaceCreated,
    SpaceList, Tab, TabSplit, Terminal, TerminalCreate, TrustAnswer, TrustRequired,
};
use poimen_client::{AttachmentWriter, Client, ClientError};
use serde_json::{Value, json};

use crate::backend::{
    ApplyRequest, Backend, BackendEvent, BackendInfo, CloseCode, LaidOut, LayoutRequest, Mode,
    NewSpace, OnRecord, OpenedStream, Outcome, Snapshot, SplitDirection, StartError, StreamRecord,
    StreamRequest,
};
use crate::herdr::{AgentSession, PromptOutcome, ServerInfo};
use crate::poimen_server;
use crate::trust;
use poimen_core::snapshot as snap;

const NO_RENDERED_INPUT: &str = "xterm.js handles the mouse and the scrollback on this backend";

/// One open stream: the write half of its attachment.
struct Stream {
    writer: Mutex<AttachmentWriter>,
}

pub struct PoimenBackend {
    client: Client,
    /// The server of a machine (`machines.rs`): reached over SSH, never started or stopped here.
    remote: bool,
    next_id: AtomicU32,
    streams: Arc<Mutex<HashMap<u32, Arc<Stream>>>>,
}

impl PoimenBackend {
    /// Gives the tab whose only terminal is `terminal_id` the name `name`; no such tab is Ok.
    fn rename_lone_tab(&self, terminal_id: &str, name: &str) -> Result<(), String> {
        let alone = Node::terminal(terminal_id);
        let layout = self.client.list_layout().map_err(text)?;
        match layout.tabs.iter().find(|t| t.root == alone) {
            Some(tab) => self.rename_tab(&tab.tab_id, name),
            None => Ok(()),
        }
    }

    pub fn new(client: Client) -> Self {
        Self {
            client,
            remote: false,
            next_id: AtomicU32::new(0),
            streams: Arc::default(),
        }
    }

    /// The backend of a machine's server: `client` reaches it over its connector, and poimen never
    /// starts, restarts or stops that server.
    pub fn remote(client: Client) -> Self {
        Self {
            remote: true,
            ..Self::new(client)
        }
    }

    /// `machine.check` of this backend's server (#281).
    pub fn machine_check(&self) -> Result<poimen_client::protocol::MachineCheckResult, String> {
        self.client.machine_check().map_err(text)
    }

    /// `repo.ensure` of this backend's server: the clone of `owner/repo` on its machine (#283).
    pub fn ensure_repo(
        &self,
        owner: &str,
        repo: &str,
    ) -> Result<poimen_client::protocol::RepoEnsureResult, String> {
        self.client.ensure_repo(owner, repo).map_err(text)
    }

    /// `worktree.create` of this backend's server: the worktree of a task issue (#283).
    pub fn create_worktree(
        &self,
        repo_dir: &str,
        issue: poimen_client::protocol::TaskIssue,
        mode: &str,
    ) -> Result<poimen_client::protocol::WorktreeCreateResult, String> {
        self.client
            .create_worktree(repo_dir, issue, mode)
            .map_err(text)
    }

    /// `worktree.cleanup` of this backend's server: "Clean up merged tasks" in its clone (#284).
    pub fn cleanup_worktrees(
        &self,
        params: poimen_client::protocol::WorktreeCleanup,
    ) -> Result<poimen_client::protocol::WorktreeCleanupResult, String> {
        self.client.cleanup_worktrees(params).map_err(text)
    }

    fn local_only(&self) -> Result<(), StartError> {
        if self.remote {
            return Err(StartError::Other(
                "poimen never starts or stops the poimen-server of a machine".into(),
            ));
        }
        Ok(())
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

/// The neutral snapshot of the server's spaces (#586): a server tab is a tab with its terminals in
/// the order of its split tree (the first leaf first) and the tree as `root`. A terminal without
/// a leaf (an exited one, or every terminal of a protocol 3 server) is a tab of its own with the
/// terminal's id, so no terminal is lost. Tabs follow the space's `tabs` order and are numbered by
/// it. Agent status is what the agent's hooks reported (`unknown` without an agent); a tab and a
/// space show their most urgent terminal, and a space carries its own `cwd`.
pub fn map_snapshot(layout: &SpaceList, terminals: &[Terminal]) -> Snapshot {
    let mut snapshot = Snapshot::default();
    for (i, space) in layout.spaces.iter().enumerate() {
        let own: Vec<&Terminal> = terminals
            .iter()
            .filter(|t| t.space_id == space.id)
            .collect();
        snapshot.spaces.push(snap::Space {
            space_id: space.id.clone(),
            label: space.name.clone(),
            number: number(i),
            agent_status: most_urgent(own.iter().copied()),
            // The space's own directory, so a space without terminals is still found by it.
            cwd: Some(space.cwd.clone()).filter(|c| !c.is_empty()),
        });
        let mut placed = HashSet::new();
        let mut count = 0;
        for tab in tabs_of(layout, space) {
            let members: Vec<&Terminal> = tab
                .root
                .terminal_ids()
                .into_iter()
                .filter_map(|id| own.iter().copied().find(|t| t.id == id))
                .collect();
            // None of its leaves is in the terminal list (they closed between the two reads):
            // the tab is gone by the next snapshot.
            if members.is_empty() {
                continue;
            }
            snapshot.tabs.push(snap::Tab {
                tab_id: tab.tab_id.clone(),
                space_id: space.id.clone(),
                label: tab.name.clone(),
                number: number(count),
                agent_status: most_urgent(members.iter().copied()),
                root: Some(tab.root.clone()),
                active_terminal: Some(tab.active_terminal.clone()).filter(|t| !t.is_empty()),
            });
            count += 1;
            for terminal in members {
                placed.insert(terminal.id.as_str());
                snapshot
                    .terminals
                    .push(snapshot_terminal(terminal, &space.id, &tab.tab_id));
            }
        }
        for terminal in own.into_iter().filter(|t| !placed.contains(t.id.as_str())) {
            snapshot.tabs.push(snap::Tab {
                tab_id: terminal.id.clone(),
                space_id: space.id.clone(),
                label: terminal.name.clone(),
                number: number(count),
                agent_status: status_of(terminal),
                root: None,
                active_terminal: None,
            });
            count += 1;
            snapshot
                .terminals
                .push(snapshot_terminal(terminal, &space.id, &terminal.id));
        }
    }
    snapshot
}

/// The 1-based number of the record at `index`.
fn number(index: usize) -> u32 {
    u32::try_from(index + 1).unwrap_or(u32::MAX)
}

/// The tabs of `space`: those of its `tabs` order first, then any other tab of the space.
fn tabs_of<'a>(layout: &'a SpaceList, space: &Space) -> Vec<&'a Tab> {
    let mine = |t: &&Tab| t.space_id == space.id;
    let mut ordered: Vec<&Tab> = space
        .tabs
        .iter()
        .filter_map(|id| layout.tabs.iter().filter(mine).find(|t| &t.tab_id == id))
        .collect();
    for tab in layout.tabs.iter().filter(mine) {
        if !ordered.iter().any(|t| t.tab_id == tab.tab_id) {
            ordered.push(tab);
        }
    }
    ordered
}

/// The status of the most urgent terminal, `unknown` for none.
fn most_urgent<'a>(terminals: impl Iterator<Item = &'a Terminal>) -> snap::AgentStatus {
    snap::AgentStatus::most_urgent(terminals.map(status_of))
}

/// A server terminal in the snapshot: `agent` (the name the hooks report, `claude` or `copilot`)
/// and `agent_session.value` are set only when known.
fn snapshot_terminal(terminal: &Terminal, space_id: &str, tab_id: &str) -> snap::Terminal {
    let agent = terminal.agent.as_ref();
    let message = agent.and_then(|a| a.message.clone());
    snap::Terminal {
        terminal_id: terminal.id.clone(),
        tab_id: tab_id.to_owned(),
        space_id: space_id.to_owned(),
        agent: agent
            .and_then(|a| a.agent.clone())
            .filter(|a| !a.is_empty()),
        agent_session: agent
            .and_then(|a| a.session_id.clone())
            .map(|value| snap::AgentSessionRef { value: Some(value) }),
        agent_status: status_of(terminal),
        agent_message_at: message.as_ref().and(agent.and_then(|a| a.message_at)),
        agent_message: message,
        cwd: Some(terminal.cwd.clone()),
        // The name of a terminal is its label only once it was renamed: the default is the
        // directory name, which the sidebar already shows for a terminal without a label.
        label: (terminal.name != default_name(&terminal.cwd)).then(|| terminal.name.clone()),
        // The same `tokens` map herdr's panes have, so `$<name>` row tokens work unchanged.
        tokens: terminal.tokens.clone(),
        // The restart confirmation names only the terminals a restart ends.
        survives_restart: terminal.survives_restart,
        program: terminal.program.clone(),
        // The mark on a terminal whose host is older than the bundled one (#606).
        host_version: terminal
            .host_bundled_version
            .as_ref()
            .and(terminal.host_version.clone()),
        host_bundled_version: terminal
            .host_version
            .as_ref()
            .and(terminal.host_bundled_version.clone()),
        ..snap::Terminal::default()
    }
}

/// The `agent_status` of the snapshot for a terminal.
fn status_of(terminal: &Terminal) -> snap::AgentStatus {
    terminal
        .agent
        .as_ref()
        .map_or(snap::AgentStatus::Unknown, |a| snap_status(a.status))
}

/// The status name of the frontend's events and prompt outcomes (`gone` is `unknown`).
fn status_name(status: AgentStatus) -> &'static str {
    match snap_status(status) {
        snap::AgentStatus::Idle => "idle",
        snap::AgentStatus::Working => "working",
        snap::AgentStatus::Blocked => "blocked",
        snap::AgentStatus::Done => "done",
        snap::AgentStatus::Unknown => "unknown",
    }
}

fn snap_status(status: AgentStatus) -> snap::AgentStatus {
    match status {
        AgentStatus::Idle => snap::AgentStatus::Idle,
        AgentStatus::Working => snap::AgentStatus::Working,
        AgentStatus::Blocked => snap::AgentStatus::Blocked,
        AgentStatus::Done => snap::AgentStatus::Done,
        AgentStatus::Gone => snap::AgentStatus::Unknown,
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

/// The trust question of the server, for the dialog.
fn trust_request(t: TrustRequired) -> LaidOut {
    LaidOut::Trust(trust::TrustRequest {
        root: t.root,
        files: t.files,
        hash: t.hash,
        commands: t
            .commands
            .into_iter()
            .map(|c| trust::ListedCommand {
                what: c.what,
                command: c.command,
            })
            .collect(),
    })
}

fn built(created: SpaceCreated) -> LaidOut {
    LaidOut::Created {
        space_id: created.space.id,
        warnings: created.warnings,
        setup_started: created.setup_started,
    }
}

/// The neutral answer of `space.create`: the space, or the trust question for the dialog.
fn laid_out(result: SpaceCreateResult) -> LaidOut {
    match result {
        SpaceCreateResult::TrustRequired { trust_required } => trust_request(trust_required),
        SpaceCreateResult::Created(created) => built(created),
    }
}

/// The neutral answer of `space.apply_layout`: the space it built into, or the trust question.
fn applied(result: SpaceApplyLayoutResult) -> LaidOut {
    match result {
        SpaceApplyLayoutResult::TrustRequired { trust_required } => trust_request(trust_required),
        SpaceApplyLayoutResult::Applied(created) => built(created),
    }
}

fn trust_answer(decision: &trust::Decision) -> TrustAnswer {
    match decision {
        trust::Decision::Trust { hash } => TrustAnswer::Trust { hash: hash.clone() },
        trust::Decision::Without => TrustAnswer::Without,
    }
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

/// The connection of an attached stream ended without a record.
fn lost(err: &ClientError) -> StreamRecord {
    StreamRecord::Closed {
        reason: format!("the connection to poimen-server ended: {err}"),
        exit_code: None,
        code: Some(CloseCode::Lost),
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
        self.local_only()?;
        poimen_server::ensure_running(&poimen_server::SystemRunner::new(), auto_start)
            .map_err(|e| StartError::Other(e.to_string()))
    }

    fn restart_server(&self) -> Result<Outcome, StartError> {
        self.local_only()?;
        poimen_server::restart(&poimen_server::SystemRunner::new())
            .map_err(|e| StartError::Other(e.to_string()))
    }

    fn stop_server(&self) -> Result<(), StartError> {
        self.local_only()?;
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
        let layout = self.client.list_layout().map_err(text)?;
        let terminals = self.client.list_terminals(None).map_err(text)?;
        Ok(map_snapshot(&layout, &terminals))
    }

    fn create_space(&self, cwd: &str, label: &str) -> Result<NewSpace, String> {
        let space = self.client.create_space(label, cwd).map_err(text)?;
        let terminal = self.new_terminal(&space.id, None)?;
        Ok(NewSpace {
            space_id: space.id,
            root_terminal: Some(terminal.id),
        })
    }

    /// `space.create` with `layout`: the server resolves the layout of `cwd`, builds its tabs and
    /// splits, starts the setup and asks for trust in the project file first.
    fn create_space_with_layout(
        &self,
        request: LayoutRequest<'_>,
    ) -> Option<Result<LaidOut, String>> {
        let params = SpaceCreate {
            trust: request.decision.map(trust_answer),
            command: request.command.map(str::to_owned),
            ..SpaceCreate::with_layout(request.label, request.cwd)
        };
        Some(
            self.client
                .create_space_with(&params)
                .map(laid_out)
                .map_err(text),
        )
    }

    /// `space.apply_layout`: the server builds the space's resolved layout (or the preset) into
    /// it as new tabs and asks for trust in the project file first.
    fn apply_layout(&self, request: ApplyRequest<'_>) -> Option<Result<LaidOut, String>> {
        let params = SpaceApplyLayout {
            space_id: request.space_id.to_owned(),
            cwd: Some(request.cwd.to_owned()),
            preset: request.preset.map(str::to_owned),
            trust: request.decision.map(trust_answer),
        };
        Some(self.client.apply_layout(&params).map(applied).map_err(text))
    }

    /// The server's focus is shared by every client and changes only through `tab.focus`; the UI
    /// keeps its own selection.
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

    /// `tab.split`: the leaf of `target` becomes a split of it (first) and the new terminal.
    fn split(
        &self,
        target: &str,
        direction: SplitDirection,
        cwd: Option<&str>,
        ratio: Option<f64>,
    ) -> Result<Option<String>, String> {
        let (_, terminal) = self
            .client
            .split(&TabSplit {
                terminal_id: target.into(),
                direction: match direction {
                    SplitDirection::Right => wire::SplitDirection::Right,
                    SplitDirection::Down => wire::SplitDirection::Down,
                },
                ratio,
                cwd: cwd.map(str::to_owned),
                name: None,
                command: None,
            })
            .map_err(text)?;
        Ok(Some(terminal.id))
    }

    fn close_terminal(&self, terminal_id: &str) -> Result<(), String> {
        self.client.close_terminal(terminal_id).map_err(text)
    }

    fn restart_terminal(&self, terminal_id: &str) -> Result<Option<String>, String> {
        let old = self.terminal(terminal_id)?;
        // The new terminal starts beside the old one; closing the old one collapses the split,
        // so the new one takes its place in the tree.
        let (_, new) = self
            .client
            .split(&TabSplit {
                terminal_id: old.id.clone(),
                direction: wire::SplitDirection::Right,
                ratio: None,
                cwd: Some(old.cwd.clone()),
                name: Some(old.name.clone()),
                command: old.program.clone().map(|p| {
                    let mut argv = vec![p];
                    argv.extend(old.args.iter().cloned());
                    argv
                }),
            })
            .map_err(text)?;
        self.client.close_terminal(&old.id).map_err(text)?;
        Ok(Some(new.id))
    }

    fn rename_terminal(&self, terminal_id: &str, label: Option<&str>) -> Result<(), String> {
        let name = match label.map(str::trim).filter(|l| !l.is_empty()) {
            Some(label) => label.to_owned(),
            None => default_name(&self.terminal(terminal_id)?.cwd),
        };
        self.client
            .rename_terminal(terminal_id, &name)
            .map_err(text)?;
        // A tab that holds only this terminal is the terminal to the user: it takes the name
        // too, so every client shows the same. The terminal is renamed at this point, so a
        // failure here is logged and never fails the call.
        if let Err(err) = self.rename_lone_tab(terminal_id, &name) {
            eprintln!("poimen: terminal {terminal_id} renamed, its tab not: {err}");
        }
        Ok(())
    }

    fn close_tab(&self, tab_id: &str) -> Result<(), String> {
        self.client.close_tab(tab_id).map_err(text)
    }

    fn rename_tab(&self, tab_id: &str, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("The name of a tab cannot be empty.".into());
        }
        self.client.rename_tab(tab_id, name).map(drop).map_err(text)
    }

    fn resize_split(&self, tab_id: &str, split_id: &str, ratio: f64) -> Result<(), String> {
        if !(ratio.is_finite() && ratio > 0.0 && ratio < 1.0) {
            return Err("The ratio of a split must be between 0 and 1.".into());
        }
        self.client
            .resize_split(tab_id, split_id, ratio)
            .map(drop)
            .map_err(text)
    }

    fn focus_tab(&self, tab_id: &str, terminal_id: &str) -> Result<(), String> {
        self.client
            .focus_tab(tab_id, Some(terminal_id))
            .map(drop)
            .map_err(text)
    }

    fn send_input(&self, terminal_id: &str, text_: &str) -> Result<(), String> {
        self.client
            .input(terminal_id, &format!("{text_}\r"))
            .map_err(text)
    }

    /// Runs the agent in the terminal and returns when its `SessionStart` report arrives (30 s); a kind
    /// without hooks returns once the command is typed (it shows no status).
    /// A name renames the terminal; without one the terminal keeps its name.
    fn agent_start(
        &self,
        terminal_id: &str,
        kind: &str,
        name: Option<&str>,
        args: &[String],
    ) -> Result<(), String> {
        // Only a kind with hooks reports `SessionStart`; any other would time out although it runs.
        if poimen_protocol::HOOK_KINDS.contains(&kind) {
            self.client
                .start_agent(terminal_id, kind, args, agent::START_TIMEOUT)
                .map(drop)
        } else {
            self.client.start_unreported(terminal_id, kind, args)
        }
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
        Ok(terminals
            .iter()
            .find(|t| t.id == terminal_id)
            .and_then(|t| {
                let agent = t.agent.as_ref()?;
                crate::herdr::session_of(
                    agent.agent.as_deref(),
                    agent.session_id.as_deref(),
                    &t.cwd,
                )
            }))
    }

    fn agent_transcript(
        &self,
        terminal_id: &str,
        follower: &str,
        restart: bool,
    ) -> Result<wire::AgentTranscriptResult, String> {
        self.client
            .agent_transcript(terminal_id, follower, restart)
            .map_err(text)
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
                    // The server went away (a restart, a crash): the terminal may live on in its
                    // host and come back with the next server (ADR 0009). (A stream closed by
                    // poimen ends here too, but its view no longer listens.)
                    Err(err) => break lost(&err),
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
pub fn backend_event(event: &Event) -> BackendEvent {
    // The frontend applies status changes from the neutral event, keyed by terminal.
    if let Event::AgentStatusChanged {
        terminal_id,
        status,
        message,
        message_at,
        ..
    } = event
    {
        let mut data = json!({"terminal_id": terminal_id, "agent_status": status_name(*status)});
        if let Some(message) = message {
            data["agent_message"] = json!(message);
            data["agent_message_at"] = json!(message_at);
        }
        return BackendEvent {
            kind: crate::backend::AGENT_STATUS_CHANGED.into(),
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

    /// The snapshot of a server without tabs (protocol 3), as the frontend receives it: every
    /// terminal is a tab of its own.
    fn snap(spaces: &[Space], terminals: &[Terminal]) -> Value {
        wire(
            &SpaceList {
                spaces: spaces.to_vec(),
                tabs: Vec::new(),
            },
            terminals,
        )
    }

    /// The snapshot of `layout` and `terminals` as the frontend receives it.
    fn wire(layout: &SpaceList, terminals: &[Terminal]) -> Value {
        serde_json::to_value(map_snapshot(layout, terminals)).unwrap()
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
            args: Vec::new(),
            agent: None,
            tokens: Default::default(),
            survives_restart: false,
            host_version: None,
            host_bundled_version: None,
        }
    }

    #[test]
    fn without_server_tabs_each_terminal_is_a_tab_of_its_own() {
        let snapshot = snap(
            &[space("s1", "api"), space("s2", "web")],
            &[
                terminal("t1", "s1", "zsh"),
                terminal("t3", "s2", "dev"),
                terminal("t2", "s1", "tests"),
            ],
        );
        assert_eq!(
            snapshot["spaces"],
            json!([
                {"space_id": "s1", "label": "api", "number": 1, "agent_status": "unknown", "cwd": "/work/api"},
                {"space_id": "s2", "label": "web", "number": 2, "agent_status": "unknown", "cwd": "/work/web"},
            ])
        );
        assert_eq!(
            snapshot["tabs"],
            json!([
                {"tab_id": "t1", "space_id": "s1", "label": "zsh", "number": 1, "agent_status": "unknown"},
                {"tab_id": "t2", "space_id": "s1", "label": "tests", "number": 2, "agent_status": "unknown"},
                {"tab_id": "t3", "space_id": "s2", "label": "dev", "number": 1, "agent_status": "unknown"},
            ])
        );
        assert_eq!(
            snapshot["terminals"][1],
            json!({
                "terminal_id": "t2", "tab_id": "t2", "space_id": "s1", "cwd": "/work/tests",
                "agent_status": "unknown", "focused": false,
            })
        );
    }

    fn leaf(id: &str) -> Node {
        Node::terminal(id)
    }

    fn split(id: &str, direction: wire::SplitDirection, first: Node, second: Node) -> Node {
        Node::Split {
            split_id: id.into(),
            direction,
            ratio: 0.6,
            children: Box::new([first, second]),
        }
    }

    fn tab(id: &str, space_id: &str, name: &str, root: Node) -> Tab {
        Tab {
            tab_id: id.into(),
            space_id: space_id.into(),
            name: name.into(),
            active_terminal: root.first_terminal().to_owned(),
            root,
        }
    }

    /// Space s1 with the tabs `tab1` (agent right of tests above shell) and `tab2` (logs), in the
    /// order tab2, tab1.
    fn split_layout() -> SpaceList {
        let mut s1 = space("s1", "api");
        s1.tabs = vec!["tab2".into(), "tab1".into()];
        s1.active_tab = Some("tab1".into());
        SpaceList {
            spaces: vec![s1],
            tabs: vec![
                tab(
                    "tab1",
                    "s1",
                    "agent",
                    split(
                        "split1",
                        wire::SplitDirection::Right,
                        leaf("t1"),
                        split("split2", wire::SplitDirection::Down, leaf("t3"), leaf("t2")),
                    ),
                ),
                tab("tab2", "s1", "logs", leaf("t4")),
            ],
        }
    }

    #[test]
    fn server_tabs_become_tabs_with_their_terminals_in_tree_order() {
        let terminals = [
            terminal("t1", "s1", "agent"),
            terminal("t2", "s1", "shell"),
            terminal("t3", "s1", "tests"),
            terminal("t4", "s1", "logs"),
        ];
        let snapshot = wire(&split_layout(), &terminals);
        let ids = |v: &Value, key: &str| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|x| x[key].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        // The space's order, numbered by it.
        assert_eq!(ids(&snapshot["tabs"], "tab_id"), ["tab2", "tab1"]);
        assert_eq!(snapshot["tabs"][0]["number"], 1);
        assert_eq!(snapshot["tabs"][1]["number"], 2);
        assert_eq!(snapshot["tabs"][1]["label"], "agent");
        // The panes of a split tab follow its leaves, first leaf first.
        assert_eq!(
            ids(&snapshot["terminals"], "terminal_id"),
            ["t4", "t1", "t3", "t2"]
        );
        assert_eq!(
            ids(&snapshot["terminals"], "tab_id"),
            ["tab2", "tab1", "tab1", "tab1"]
        );
        // The tree and the focused leaf reach the frontend as the server sent them.
        assert_eq!(snapshot["tabs"][1]["active_terminal"], "t1");
        assert_eq!(
            snapshot["tabs"][1]["root"],
            json!({"kind": "split", "split_id": "split1", "direction": "right", "ratio": 0.6,
            "children": [
                {"kind": "terminal", "terminal_id": "t1"},
                {"kind": "split", "split_id": "split2", "direction": "down", "ratio": 0.6,
                    "children": [
                        {"kind": "terminal", "terminal_id": "t3"},
                        {"kind": "terminal", "terminal_id": "t2"},
                    ]},
            ]})
        );
    }

    /// The fixture the frontend specs read (`core/snapshot.spec.ts`): a split tab, a plain tab and an
    /// exited terminal without a leaf.
    const SPLIT_SNAPSHOT: &str =
        include_str!("../../src/app/core/fixtures/poimen-snapshot-split-tab.json");

    #[test]
    fn the_snapshot_of_a_split_tab_matches_the_frontend_fixture() {
        let mut agent = terminal("t1", "s1", "agent");
        agent.agent = Some(AgentInfo {
            status: AgentStatus::Working,
            agent: Some("claude".into()),
            session_id: Some("abc-1".into()),
            message: None,
            message_at: None,
        });
        let mut exited = terminal("t5", "s1", "build");
        exited.status = TerminalStatus::Exited;
        exited.exit_code = Some(1);
        let terminals = [
            agent,
            terminal("t2", "s1", "shell"),
            terminal("t3", "s1", "tests"),
            terminal("t4", "s1", "logs"),
            exited,
        ];
        let snapshot = map_snapshot(&split_layout(), &terminals);
        let expected: Value = serde_json::from_str(SPLIT_SNAPSHOT).unwrap();
        assert_eq!(serde_json::to_value(&snapshot).unwrap(), expected);
    }

    #[test]
    fn a_split_tab_shows_its_most_urgent_terminal() {
        let mut blocked = terminal("t2", "s1", "shell");
        blocked.agent = Some(AgentInfo {
            status: AgentStatus::Blocked,
            agent: Some("claude".into()),
            session_id: None,
            message: None,
            message_at: None,
        });
        let terminals = [
            terminal("t1", "s1", "agent"),
            blocked,
            terminal("t3", "s1", "tests"),
            terminal("t4", "s1", "logs"),
        ];
        let snapshot = wire(&split_layout(), &terminals);
        assert_eq!(snapshot["tabs"][0]["agent_status"], "unknown");
        assert_eq!(snapshot["tabs"][1]["agent_status"], "blocked");
        assert_eq!(snapshot["spaces"][0]["agent_status"], "blocked");
    }

    #[test]
    fn a_terminal_without_a_leaf_keeps_a_tab_of_its_own() {
        let mut exited = terminal("t5", "s1", "build");
        exited.status = TerminalStatus::Exited;
        exited.exit_code = Some(1);
        let terminals = [
            terminal("t1", "s1", "agent"),
            terminal("t2", "s1", "shell"),
            terminal("t3", "s1", "tests"),
            terminal("t4", "s1", "logs"),
            exited,
        ];
        let snapshot = wire(&split_layout(), &terminals);
        assert_eq!(
            snapshot["tabs"][2],
            json!({"tab_id": "t5", "space_id": "s1", "label": "build", "number": 3,
                "agent_status": "unknown"})
        );
        assert_eq!(snapshot["terminals"][4]["terminal_id"], "t5");
        assert_eq!(snapshot["terminals"][4]["tab_id"], "t5");
    }

    #[test]
    fn leaves_of_terminals_that_are_gone_are_skipped_and_an_empty_tab_is_left_out() {
        // t3 and t4 closed between `space.list` and `terminal.list`.
        let terminals = [terminal("t1", "s1", "agent"), terminal("t2", "s1", "shell")];
        let snapshot = wire(&split_layout(), &terminals);
        assert_eq!(snapshot["tabs"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["tabs"][0]["tab_id"], "tab1");
        assert_eq!(snapshot["tabs"][0]["number"], 1);
        let panes: Vec<_> = snapshot["terminals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["terminal_id"].as_str().unwrap())
            .collect();
        assert_eq!(panes, ["t1", "t2"]);
    }

    #[test]
    fn a_tab_missing_from_the_space_order_still_comes_after_the_ordered_ones() {
        let mut layout = split_layout();
        layout.spaces[0].tabs = vec!["tab1".into()];
        let terminals = [
            terminal("t1", "s1", "agent"),
            terminal("t2", "s1", "shell"),
            terminal("t3", "s1", "tests"),
            terminal("t4", "s1", "logs"),
        ];
        let snapshot = wire(&layout, &terminals);
        assert_eq!(snapshot["tabs"][0]["tab_id"], "tab1");
        assert_eq!(snapshot["tabs"][1]["tab_id"], "tab2");
    }

    #[test]
    fn a_tab_of_another_space_is_not_listed_in_this_one() {
        let mut layout = split_layout();
        layout.spaces.push(space("s2", "web"));
        layout.tabs[1].space_id = "s2".into();
        let mut logs = terminal("t4", "s2", "logs");
        logs.space_id = "s2".into();
        let terminals = [
            terminal("t1", "s1", "agent"),
            terminal("t2", "s1", "shell"),
            terminal("t3", "s1", "tests"),
            logs,
        ];
        let snapshot = wire(&layout, &terminals);
        let of = |tab: &Value| tab["space_id"].as_str().unwrap().to_owned();
        assert_eq!(
            snapshot["tabs"]
                .as_array()
                .unwrap()
                .iter()
                .map(of)
                .collect::<Vec<_>>(),
            ["s1", "s2"]
        );
        assert_eq!(snapshot["terminals"][3]["tab_id"], "tab2");
        assert_eq!(snapshot["terminals"][3]["space_id"], "s2");
    }

    #[test]
    fn a_tab_change_event_reaches_the_frontend_by_its_kind() {
        let event = backend_event(&Event::TabChanged {
            tab: tab("tab1", "s1", "agent", leaf("t1")),
        });
        assert_eq!(event.kind, "tab.changed");
        assert_eq!(event.data["tab"]["tab_id"], "tab1");
        let closed = backend_event(&Event::TabClosed {
            tab_id: "tab1".into(),
            space_id: "s1".into(),
        });
        assert_eq!(closed.kind, "tab.closed");
    }

    #[test]
    fn an_empty_server_gives_an_empty_snapshot() {
        let snapshot = snap(&[], &[]);
        assert_eq!(snapshot["spaces"], json!([]));
        assert_eq!(snapshot["terminals"], json!([]));
    }

    #[test]
    fn a_space_without_terminals_keeps_its_directory() {
        let snapshot = snap(&[space("s1", "app-7")], &[]);
        assert_eq!(snapshot["spaces"][0]["cwd"], "/work/app-7");
        assert_eq!(snapshot["terminals"], json!([]));
    }

    #[test]
    fn a_space_without_a_directory_has_no_cwd() {
        let mut s = space("s1", "a");
        s.cwd = String::new();
        let snapshot = snap(&[s], &[]);
        assert!(snapshot["spaces"][0].get("cwd").is_none());
    }

    #[test]
    fn a_terminal_of_an_unknown_space_is_left_out() {
        let snapshot = snap(&[space("s1", "a")], &[terminal("t1", "gone", "x")]);
        assert_eq!(snapshot["tabs"], json!([]));
    }

    #[test]
    fn exited_terminals_stay_in_the_tree() {
        let mut t = terminal("t1", "s1", "zsh");
        t.status = TerminalStatus::Exited;
        t.exit_code = Some(1);
        assert_eq!(
            snap(&[space("s1", "a")], &[t])["terminals"][0]["terminal_id"],
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
        let snapshot = snap(
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
        assert_eq!(
            status(&snapshot["terminals"]),
            ["working", "blocked", "unknown"]
        );
        assert_eq!(status(&snapshot["tabs"]), ["working", "blocked", "unknown"]);
        assert_eq!(status(&snapshot["spaces"]), ["blocked", "unknown"]);
    }

    #[test]
    fn an_agent_status_event_becomes_the_neutral_event_the_frontend_applies() {
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
        assert_eq!(event.kind, "terminal.agent_status_changed");
        assert_eq!(
            event.data,
            json!({"terminal_id": "t1", "agent_status": "blocked"})
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
            json!({"terminal_id": "t1", "agent_status": "blocked",
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
        let snapshot = snap(&[space("s1", "api")], &[t, terminal("t2", "s1", "b")]);
        assert_eq!(snapshot["terminals"][0]["agent_message"], "Allow Bash?");
        assert_eq!(snapshot["terminals"][0]["agent_message_at"], 1700);
        assert!(snapshot["terminals"][1].get("agent_message").is_none());
    }

    #[test]
    fn a_terminal_that_survives_a_restart_says_so_on_its_pane() {
        let mut t = terminal("t1", "s1", "a");
        t.survives_restart = true;
        let snapshot = snap(&[space("s1", "api")], &[t, terminal("t2", "s1", "b")]);
        assert_eq!(snapshot["terminals"][0]["survives_restart"], true);
        assert!(snapshot["terminals"][1].get("survives_restart").is_none());
    }

    #[test]
    fn the_host_versions_reach_the_pane_only_when_the_server_reports_them() {
        let mut t = terminal("t1", "s1", "a");
        t.host_version = Some("0.15.0".into());
        t.host_bundled_version = Some("0.16.0".into());
        let snapshot = snap(&[space("s1", "api")], &[t, terminal("t2", "s1", "b")]);
        assert_eq!(snapshot["terminals"][0]["host_version"], "0.15.0");
        assert_eq!(snapshot["terminals"][0]["host_bundled_version"], "0.16.0");
        assert!(snapshot["terminals"][1].get("host_version").is_none());
    }

    #[test]
    fn tokens_reach_the_pane_as_herdr_tokens_and_their_event_passes_through() {
        let mut t = terminal("t1", "s1", "a");
        t.tokens = [("deliver_step".to_owned(), "build".to_owned())].into();
        let snapshot = snap(&[space("s1", "api")], &[t, terminal("t2", "s1", "b")]);
        assert_eq!(
            snapshot["terminals"][0]["tokens"],
            json!({"deliver_step": "build"})
        );
        assert!(snapshot["terminals"][1].get("tokens").is_none());
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
                                // The server goes away (a restart or a crash).
                                if line == GONE {
                                    let _ = stream.shutdown();
                                    return;
                                }
                                writeln!(stream, "{line}").unwrap();
                            }
                        }
                        line.clear();
                    }
                });
            }
        });
    }

    /// An `extra` line of [`serve`]: the server ends the connection without a record.
    const GONE: &str = "<gone>";

    fn tab_json(tab_id: &str, space: &str, root: Value) -> Value {
        json!({"tab_id": tab_id, "space_id": space, "name": "zsh", "root": root, "active_terminal": "t1"})
    }

    fn split_json(first: &str, second: &str) -> Value {
        json!({"kind": "split", "split_id": "split7", "direction": "right", "ratio": 0.5, "children": [
            {"kind": "terminal", "terminal_id": first},
            {"kind": "terminal", "terminal_id": second},
        ]})
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
                "space.create" if params["layout"] == false => {
                    Ok(json!({"space": {"id": "s9", "name": params["name"], "cwd": params["cwd"]}}))
                }
                "space.create"
                    if params["cwd"] == "/work/untrusted" && params["trust"].is_null() =>
                {
                    Ok(json!({"trust_required": {
                        "root": "/work/untrusted", "files": ["/work/untrusted/.poimen/config.json"],
                        "hash": "abc", "commands": [{"what": "setup", "command": "make"}],
                        "layout": [{"name": "agent", "command": "claude"}],
                    }}))
                }
                "space.create" => Ok(json!({
                    "space": {"id": "s9", "name": params["name"], "cwd": params["cwd"], "tabs": ["tab4"], "active_tab": "tab4"},
                    "tabs": [tab_json("tab4", "s9", json!({"kind": "terminal", "terminal_id": "t9"}))],
                    "warnings": ["template app: harness codex is not installed"],
                    "setup_started": true,
                })),
                "space.apply_layout" if params["trust"].is_null() && params["preset"] == "duo" => {
                    Ok(json!({"trust_required": {
                        "root": "/work/api", "files": [], "hash": "abc", "commands": [],
                    }}))
                }
                "space.apply_layout" => Ok(json!({
                    "space": {"id": params["space_id"], "name": "api", "cwd": "/work/api", "tabs": ["tab1", "tab5"]},
                    "tabs": [tab_json("tab5", "s1", json!({"kind": "terminal", "terminal_id": "t9"}))],
                    "warnings": ["w"],
                    "setup_started": true,
                })),
                "space.list" => Ok(json!({
                    "spaces": [{"id": "s1", "name": "api", "cwd": "/work/api", "tabs": ["tab1", "tab2"], "active_tab": "tab1"}],
                    "tabs": [
                        tab_json("tab1", "s1", json!({"kind": "terminal", "terminal_id": "t1"})),
                        tab_json("tab2", "s1", split_json("t2", "t3")),
                    ],
                })),
                "tab.split" if params["terminal_id"] == "t1" => Ok(json!({
                    "tab": tab_json("tab1", "s1", split_json("t1", "t9")),
                    "terminal": term_json("t9", "s1"),
                })),
                "tab.split" => Err(("not_found", "terminal nope not found")),
                "tab.rename" => Ok(json!({"tab": tab_json(
                    params["tab_id"].as_str().unwrap(), "s1",
                    json!({"kind": "terminal", "terminal_id": "t1"}),
                )})),
                "tab.close" => Ok(json!({})),
                "tab.resize_split" | "tab.focus" => Ok(json!({"tab": tab_json(
                    "tab1",
                    "s1",
                    json!({"kind": "terminal", "terminal_id": "t1"})
                )})),
                "terminal.create" => {
                    Ok(json!({"terminal": term_json("t9", params["space_id"].as_str().unwrap())}))
                }
                "terminal.list" => {
                    let mut t1 = term_json("t1", "s1");
                    t1["program"] = json!("claude");
                    t1["args"] = json!(["--model", "opus"]);
                    Ok(json!({"terminals": [t1]}))
                }
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
    fn a_restart_splits_beside_the_terminal_then_closes_it() {
        let (backend, log) = backend_for("restart", vec![], false);
        let id = backend.restart_terminal("t1").unwrap();
        assert_eq!(id.as_deref(), Some("t9"));
        let params = params_of(&log, "tab.split");
        assert_eq!(params["terminal_id"], "t1");
        assert_eq!(params["direction"], "right");
        assert_eq!(params["command"], json!(["claude", "--model", "opus"]));
        let lines = lock(&log).clone();
        let split = lines
            .iter()
            .position(|l| l.starts_with("tab.split"))
            .unwrap();
        let close = lines
            .iter()
            .position(|l| l.starts_with("terminal.close"))
            .unwrap();
        assert!(
            split < close,
            "the new terminal exists before the old one ends"
        );
        assert_eq!(params_of(&log, "terminal.close")["terminal_id"], "t1");
    }

    #[test]
    fn a_restart_of_an_unknown_terminal_fails_and_closes_nothing() {
        let (backend, log) = backend_for("restart-unknown", vec![], false);
        assert!(backend.restart_terminal("nope").is_err());
        assert!(!lock(&log).iter().any(|l| l.starts_with("terminal.close")));
    }

    #[test]
    fn a_split_goes_to_the_server_as_tab_split_and_returns_the_new_terminal() {
        let (backend, log) = backend_for("split", vec![], false);
        let id = backend
            .split("t1", SplitDirection::Down, Some("/work/api"), Some(0.6))
            .unwrap();
        assert_eq!(id.as_deref(), Some("t9"));
        assert_eq!(
            params_of(&log, "tab.split"),
            json!({"terminal_id": "t1", "direction": "down", "ratio": 0.6, "cwd": "/work/api"})
        );
        assert!(!lock(&log).iter().any(|l| l.starts_with("terminal.create")));
    }

    #[test]
    fn a_split_without_ratio_or_directory_leaves_both_to_the_server() {
        let (backend, log) = backend_for("split-defaults", vec![], false);
        backend
            .split("t1", SplitDirection::Right, None, None)
            .unwrap();
        assert_eq!(
            params_of(&log, "tab.split"),
            json!({"terminal_id": "t1", "direction": "right"})
        );
    }

    #[test]
    fn a_split_of_an_unknown_terminal_reports_the_server_error() {
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
    fn renaming_the_only_terminal_of_a_tab_renames_the_tab_too() {
        let (backend, log) = backend_for("rename-alone", vec![], false);
        backend.rename_terminal("t1", Some(" build ")).unwrap();
        assert_eq!(
            params_of(&log, "tab.rename"),
            json!({"tab_id": "tab1", "name": "build"})
        );
    }

    /// A backend whose server renames terminals but fails `failing` (`space.list` or
    /// `tab.rename`).
    fn backend_failing(
        name: &str,
        failing: &'static str,
    ) -> (PoimenBackend, Arc<Mutex<Vec<String>>>) {
        let path = temp_socket(name);
        let log = Arc::new(Mutex::new(Vec::new()));
        serve(
            &path,
            log.clone(),
            move |method, _| match method {
                m if m == failing => Err(("internal", "server hiccup")),
                "terminal.rename" => Ok(json!({"terminal": term_json("t1", "s1")})),
                "space.list" => Ok(json!({
                    "spaces": [{"id": "s1", "name": "api", "cwd": "/work/api", "tabs": ["tab1"], "active_tab": "tab1"}],
                    "tabs": [tab_json("tab1", "s1", json!({"kind": "terminal", "terminal_id": "t1"}))],
                })),
                _ => Err(("unknown_method", "no")),
            },
            vec![],
        );
        (PoimenBackend::new(Client::new(path)), log)
    }

    #[test]
    fn a_failed_tab_lookup_after_the_terminal_rename_does_not_fail_the_rename() {
        let (backend, log) = backend_failing("rename-list-fails", "space.list");
        assert_eq!(backend.rename_terminal("t1", Some("build")), Ok(()));
        assert_eq!(params_of(&log, "terminal.rename")["name"], "build");
        assert!(!lock(&log).iter().any(|l| l.starts_with("tab.rename")));
    }

    #[test]
    fn a_failed_tab_rename_after_the_terminal_rename_does_not_fail_the_rename() {
        let (backend, log) = backend_failing("rename-tab-fails", "tab.rename");
        assert_eq!(backend.rename_terminal("t1", Some("build")), Ok(()));
        assert_eq!(params_of(&log, "terminal.rename")["name"], "build");
        assert_eq!(
            params_of(&log, "tab.rename"),
            json!({"tab_id": "tab1", "name": "build"})
        );
    }

    #[test]
    fn renaming_a_terminal_of_a_split_tab_leaves_the_tab_name() {
        let (backend, log) = backend_for("rename-split", vec![], false);
        backend.rename_terminal("t2", Some("tests")).unwrap();
        assert_eq!(params_of(&log, "terminal.rename")["name"], "tests");
        assert!(!lock(&log).iter().any(|l| l.starts_with("tab.rename")));
    }

    #[test]
    fn a_tab_is_closed_and_renamed_through_the_tab_requests() {
        let (backend, log) = backend_for("tab-ops", vec![], false);
        backend.close_tab("tab2").unwrap();
        assert_eq!(params_of(&log, "tab.close"), json!({"tab_id": "tab2"}));
        backend.rename_tab("tab1", " logs ").unwrap();
        assert_eq!(
            params_of(&log, "tab.rename"),
            json!({"tab_id": "tab1", "name": "logs"})
        );
    }

    #[test]
    fn a_divider_drag_goes_to_the_server_as_tab_resize_split() {
        let (backend, log) = backend_for("resize", vec![], false);
        backend.resize_split("tab1", "split5", 0.7).unwrap();
        assert_eq!(
            params_of(&log, "tab.resize_split"),
            json!({"tab_id": "tab1", "split_id": "split5", "ratio": 0.7})
        );
    }

    #[test]
    fn a_ratio_outside_zero_and_one_is_refused_before_the_server() {
        let (backend, log) = backend_for("resize-bad", vec![], false);
        for ratio in [0.0, 1.0, -0.2, f64::NAN] {
            assert!(backend.resize_split("tab1", "split5", ratio).is_err());
        }
        assert!(!lock(&log).iter().any(|l| l.starts_with("tab.resize_split")));
    }

    #[test]
    fn focusing_a_pane_goes_to_the_server_as_tab_focus() {
        let (backend, log) = backend_for("focus", vec![], false);
        backend.focus_tab("tab1", "t2").unwrap();
        assert_eq!(
            params_of(&log, "tab.focus"),
            json!({"tab_id": "tab1", "terminal_id": "t2"})
        );
    }

    #[test]
    fn a_blank_tab_name_is_refused_before_the_server() {
        let (backend, log) = backend_for("tab-blank", vec![], false);
        assert!(backend.rename_tab("tab1", "  ").is_err());
        assert!(!lock(&log).iter().any(|l| l.starts_with("tab.rename")));
    }

    fn layout_request<'a>(
        cwd: &'a str,
        decision: Option<&'a trust::Decision>,
        command: Option<&'a str>,
    ) -> LayoutRequest<'a> {
        LayoutRequest {
            cwd,
            label: "api",
            decision,
            command,
        }
    }

    #[test]
    fn a_new_space_with_layout_is_built_by_the_server() {
        let (backend, log) = backend_for("space-layout", vec![], false);
        let request = layout_request("/work/api", None, Some("claude 'x'"));
        let outcome = backend.create_space_with_layout(request).unwrap().unwrap();
        assert_eq!(
            outcome,
            LaidOut::Created {
                space_id: "s9".into(),
                warnings: vec!["template app: harness codex is not installed".into()],
                setup_started: true,
            }
        );
        assert_eq!(
            params_of(&log, "space.create"),
            json!({"name": "api", "cwd": "/work/api", "layout": true, "command": "claude 'x'"})
        );
        // The server built the space: no terminal, split or input from the app.
        assert_eq!(lock(&log).len(), 1);
    }

    #[test]
    fn an_untrusted_project_is_the_trust_question_and_creates_nothing() {
        let (backend, log) = backend_for("space-trust", vec![], false);
        let request = layout_request("/work/untrusted", None, None);
        let outcome = backend.create_space_with_layout(request).unwrap().unwrap();
        assert_eq!(
            outcome,
            LaidOut::Trust(trust::TrustRequest {
                root: "/work/untrusted".into(),
                files: vec!["/work/untrusted/.poimen/config.json".into()],
                hash: "abc".into(),
                commands: vec![trust::ListedCommand {
                    what: "setup".into(),
                    command: "make".into(),
                }],
            })
        );
        assert_eq!(lock(&log).len(), 1);
    }

    #[test]
    fn the_trust_answer_goes_to_the_server_with_the_next_create() {
        for (n, (decision, wire)) in [
            (
                trust::Decision::Trust { hash: "abc".into() },
                json!({"kind": "trust", "hash": "abc"}),
            ),
            (trust::Decision::Without, json!({"kind": "without"})),
        ]
        .into_iter()
        .enumerate()
        {
            let (backend, log) = backend_for(&format!("space-answer-{n}"), vec![], false);
            let request = layout_request("/work/untrusted", Some(&decision), None);
            let outcome = backend.create_space_with_layout(request).unwrap().unwrap();
            assert!(matches!(outcome, LaidOut::Created { .. }));
            assert_eq!(params_of(&log, "space.create")["trust"], wire);
        }
    }

    #[test]
    fn apply_layout_goes_to_the_server_with_space_cwd_preset_and_answer() {
        let (backend, log) = backend_for("apply", vec![], false);
        let request = ApplyRequest {
            space_id: "s1",
            cwd: "/work/api",
            preset: None,
            decision: None,
        };
        assert_eq!(
            backend.apply_layout(request).unwrap().unwrap(),
            LaidOut::Created {
                space_id: "s1".into(),
                warnings: vec!["w".into()],
                setup_started: true,
            }
        );
        assert_eq!(
            params_of(&log, "space.apply_layout"),
            json!({"space_id": "s1", "cwd": "/work/api"})
        );
        // The server built the tabs: nothing else was sent.
        assert_eq!(lock(&log).len(), 1);

        let (backend, log) = backend_for("apply-preset", vec![], false);
        let preset = ApplyRequest {
            preset: Some("duo"),
            ..request
        };
        let LaidOut::Trust(asked) = backend.apply_layout(preset).unwrap().unwrap() else {
            panic!("expected the trust question")
        };
        assert_eq!(asked.hash, "abc");
        let decision = trust::Decision::Trust { hash: "abc".into() };
        let answered = ApplyRequest {
            decision: Some(&decision),
            ..preset
        };
        assert!(matches!(
            backend.apply_layout(answered).unwrap().unwrap(),
            LaidOut::Created { .. }
        ));
        let sent: Vec<Value> = lock(&log)
            .iter()
            .map(|l| serde_json::from_str(&l["space.apply_layout ".len()..]).unwrap())
            .collect();
        assert_eq!(
            sent[1],
            json!({"space_id": "s1", "cwd": "/work/api", "preset": "duo",
                   "trust": {"kind": "trust", "hash": "abc"}})
        );
    }

    #[test]
    fn a_failing_space_create_is_an_error() {
        let path = temp_socket("space-fail");
        serve(
            &path,
            Arc::default(),
            |_, _| Err(("invalid_params", "cwd /gone is not a directory")),
            vec![],
        );
        let backend = PoimenBackend::new(Client::new(path));
        let err = backend
            .create_space_with_layout(layout_request("/gone", None, None))
            .unwrap()
            .unwrap_err();
        assert!(err.contains("/gone"), "{err}");
    }

    #[test]
    fn the_snapshot_reads_the_server_tabs() {
        let (backend, _) = backend_for("snapshot-tabs", vec![], false);
        let snapshot = serde_json::to_value(backend.snapshot().unwrap()).unwrap();
        // tab1 holds t1; the leaves t2 and t3 of tab2 are not in the terminal list (closed).
        assert_eq!(snapshot["tabs"].as_array().unwrap().len(), 1);
        assert_eq!(snapshot["tabs"][0]["tab_id"], "tab1");
        assert_eq!(snapshot["terminals"][0]["tab_id"], "tab1");
    }

    #[test]
    fn info_reports_the_server_version_and_protocol() {
        let (backend, _) = backend_for("info", vec![], false);
        let info = backend.info().unwrap();
        assert_eq!(
            (info.version.as_str(), info.protocol, info.supported),
            ("0.9.0", Some(6), 6)
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
                "space.list" => Ok(json!({"spaces": [], "tabs": []})),
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
        let pane = |backend: &PoimenBackend| {
            serde_json::to_value(backend.snapshot().unwrap()).unwrap()["terminals"][0].clone()
        };
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
        let panes = snap(&[space("s1", "api")], &[with, plain])["terminals"].clone();
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
    fn a_server_that_goes_away_ends_the_stream_as_lost_so_the_view_attaches_again() {
        let extra = vec![
            r#"{"event":"terminal.output","data":{"terminal_id":"t1","seq":1,"data":"YQ=="}}"#
                .into(),
            GONE.into(),
        ];
        let (backend, _) = backend_for("gone", extra, false);
        let (opened, rx) = collect(&backend, Mode::Control, false);
        opened.unwrap();
        let last = rx.iter().map(|(_, r)| r).last().unwrap();
        assert!(
            matches!(
                &last,
                StreamRecord::Closed {
                    code: Some(CloseCode::Lost),
                    exit_code: None,
                    ..
                }
            ),
            "{last:?}"
        );
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
