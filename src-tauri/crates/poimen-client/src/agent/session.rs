//! One operation for scripted and tool-driven agent sessions (ADR 0016, #744): run a harness
//! session in a new terminal and report its end. The MCP tools, the CLI and the host module of a
//! script call it, so the permission check, the argv and the wait have one code path.
//!
//! The permission rule is ADR 0012's: free in the space of the calling terminal, any other space
//! needs the user's yes in the app per call. The check runs here, on the space id the server
//! resolved, so no caller can skip it.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use poimen_core::{harness, settings};
use poimen_protocol::{
    AgentConfirm, AgentGrantCheck, AgentStatus, AgentWriteAction, ConfirmReason, Event,
    GRANT_WORKTREE_PROTOCOL, GrantVerdict, Placement, TerminalCreate,
};
use serde_json::Value;

use super::{AgentError, PromptResult, START_TIMEOUT};
use crate::{Client, ClientError};

/// How long a write into another space waits for the user by default.
pub const CONFIRM_WAIT: Duration = Duration::from_secs(60);

/// How long a typed prompt waits for the agent to react.
const PROMPT_WAIT: Duration = Duration::from_secs(10);

/// The environment variable that names the agent-events run of a terminal.
pub const RUN_ENV: &str = "AGENT_EVENTS_RUN";

/// The words of the prompt that name a session when the caller gives no title.
const TITLE_WORDS: usize = 6;

/// Why a write into a space was not allowed.
#[derive(Debug)]
pub enum WriteError {
    /// Refused, with the reason to show.
    Refused(String),
    /// Refused without a question: the `grant` the caller's lead run records is unknown to the
    /// server (#1454). `message` starts with `grant_lost`.
    GrantLost { grant: String, message: String },
    /// A request failed.
    Client(ClientError),
}

impl From<ClientError> for WriteError {
    fn from(e: ClientError) -> Self {
        WriteError::Client(e)
    }
}

/// The write an agent wants to make.
#[derive(Debug, Clone, Copy)]
pub struct WriteAsk<'a> {
    pub space_id: &'a str,
    pub action: AgentWriteAction,
    /// The terminal prompted; `None` for a start in a new terminal.
    pub terminal_id: Option<&'a str>,
    /// The prompt or command line, shown to the user as is.
    pub text: &'a str,
    /// Ask even in the caller's own space.
    pub always_ask: bool,
    /// A `start_agent` in a new worktree of `space_id` (#1452): asked as with `always_ask`,
    /// unless a grant of the caller with `worktrees` covers it.
    pub worktree: bool,
    /// The grant id the caller's lead run records (#1454), if any; an unknown one refuses with
    /// [`WriteError::GrantLost`] instead of asking.
    pub grant: Option<&'a str>,
}

/// How a write was allowed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Authorized {
    /// In the caller's own space, without a question.
    Free,
    /// The user said yes in the app.
    Confirmed,
    /// The caller's grant covers it (ADR 0018); nobody was asked.
    Covered { grant: String },
}

/// Allows a write into `space_id` for the agent in `caller`: freely in the caller's own space
/// (unless `always_ask`), else only with the user's yes in the app. Returns whether the user
/// confirmed (a write under a grant counts as not confirmed: the user answered nothing). No calling terminal, an unknown one, a declined or unanswered question and a missing
/// app all refuse with the reason.
pub fn authorize_write(
    client: &Client,
    caller: Option<&str>,
    ask: &WriteAsk<'_>,
    confirm_wait: Duration,
) -> Result<bool, WriteError> {
    authorize_write_as(client, caller, ask, confirm_wait).map(|a| a == Authorized::Confirmed)
}

/// [`authorize_write`], saying how the write was allowed: a covered worktree start needs the
/// grant id to add its new space to the grant (#1452).
pub fn authorize_write_as(
    client: &Client,
    caller: Option<&str>,
    ask: &WriteAsk<'_>,
    confirm_wait: Duration,
) -> Result<Authorized, WriteError> {
    let WriteAsk {
        space_id,
        action,
        terminal_id,
        text,
        always_ask,
        worktree,
        grant,
    } = *ask;
    let always_ask = always_ask || worktree;
    let Some(caller) = caller.filter(|i| !i.is_empty()) else {
        return Err(WriteError::Refused(
            "refused: this agent does not run in a poimen terminal (POIMEN_TERMINAL_ID is not set), \
             so there is no space of its own to write into"
                .into(),
        ));
    };
    let terminals = client.list_terminals(None)?;
    let own = terminals.iter().find(|t| t.id == caller).ok_or_else(|| {
        WriteError::Refused(format!("terminal {caller} is not known to the server"))
    })?;
    let own_space = own.space_id == space_id && !always_ask;
    // The recorded grant ended for a known reason: ask as without a grant and name the reason.
    let mut ended: Option<(String, &str)> = None;
    // A grant of this caller (ADR 0018) covers the write without a question; a limit it reached
    // refuses, also in the caller's own space; no grant, or a server that does not know grants,
    // falls back to the question (or, in the own space, to free). Of the `always_ask` writes only
    // a worktree start is checked: the server covers it only with `worktrees` (#1452).
    if !always_ask || worktree {
        let check = AgentGrantCheck {
            caller_terminal_id: caller.to_owned(),
            action,
            space_id: space_id.to_owned(),
            worktree,
            grant: grant.map(str::to_owned),
        };
        match client.check_agent_grant(&check) {
            // An older server ignores `worktree` and may cover a worktree start it does not
            // know: that answer is not trusted, the start asks (#1452).
            Ok(GrantVerdict::Covered { grant })
                if !worktree || server_knows_worktree_grants(client) =>
            {
                return Ok(Authorized::Covered { grant });
            }
            Ok(GrantVerdict::Covered { .. }) => {}
            Ok(GrantVerdict::Refused { reason }) => return Err(WriteError::Refused(reason)),
            Ok(GrantVerdict::Lost { grant }) => {
                return Err(WriteError::GrantLost {
                    message: format!(
                        "grant_lost: the grant {grant} your lead run records is not known to \
                         poimen-server (its state was not kept, for example after a restart); \
                         the write was not made and nobody was asked: call request_grant again"
                    ),
                    grant,
                });
            }
            Ok(GrantVerdict::Ended { grant, reason }) => {
                ended = Some((grant, reason.as_str()));
            }
            Ok(GrantVerdict::NeedsConfirmation) | Err(_) => {}
        }
    }
    if own_space {
        return Ok(Authorized::Free);
    }
    let asked = AgentConfirm {
        caller_terminal_id: caller.to_owned(),
        action,
        space_id: space_id.to_owned(),
        terminal_id: terminal_id.map(str::to_owned),
        text: text.to_owned(),
        wait_ms: None,
    };
    let answer = client.confirm_agent_write(&asked, confirm_wait)?;
    let refusal = match answer.reason {
        ConfirmReason::Approved if answer.approved => return Ok(Authorized::Confirmed),
        ConfirmReason::Declined => "refused: the user declined the write into another space",
        ConfirmReason::Timeout => {
            "refused: the user did not answer in time (the write into another space was not made)"
        }
        ConfirmReason::NoApp => {
            "refused: poimen is not open, so nobody can confirm a write into another space"
        }
        ConfirmReason::Approved => "refused: the server did not approve",
    };
    Err(WriteError::Refused(match ended {
        Some((grant, reason)) => format!(
            "{refusal}; your grant {grant} ended ({reason}): call request_grant for a new one"
        ),
        None => refusal.into(),
    }))
}

/// Whether the server's `agent.grant_check` knows worktree starts; unknown counts as no.
fn server_knows_worktree_grants(client: &Client) -> bool {
    client
        .ping()
        .is_ok_and(|p| p.protocol >= GRANT_WORKTREE_PROTOCOL)
}

#[cfg(test)]
#[path = "session_grant_tests.rs"]
mod grant_tests;

/// The harnesses of the user's `config.json` under `home`, the defaults without. A project config
/// contributes none: only the user's templates start a session, trusted or not (#255).
pub fn harnesses_of(home: Option<&Path>) -> Vec<harness::Harness> {
    let config = home
        .and_then(|h| std::fs::read_to_string(settings::config_path(h)).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    match config.as_ref().and_then(|c| c.get("harnesses")) {
        Some(v) => harness::resolve(v, &mut Vec::new()),
        None => harness::default_harnesses(),
    }
}

/// What a wait on an agent ended with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    /// The last status seen (`gone` for a closed or exited terminal).
    pub status: AgentStatus,
    /// The blocked message, while the status is `blocked`.
    pub message: Option<String>,
    /// The deadline ended the wait.
    pub timed_out: bool,
}

/// Waits for the agent of `terminal` to reach a status `is_final` accepts, or for the terminal to
/// close or exit (`gone`), or for `deadline`. It subscribes to the server's events before it reads
/// the status, so no change is missed, and never polls. `on_status` sees the status now and every
/// change after it. `None` when the terminal is unknown.
pub fn watch_agent(
    client: &Client,
    terminal: &str,
    deadline: Option<Instant>,
    is_final: impl Fn(AgentStatus) -> bool,
    mut on_status: impl FnMut(AgentStatus),
) -> Result<Option<Watched>, ClientError> {
    let stream = client.subscribe()?;
    let Some(t) = client
        .list_terminals(None)?
        .into_iter()
        .find(|t| t.id == terminal)
    else {
        return Ok(None);
    };
    let now = t.agent.as_ref().map_or(AgentStatus::Gone, |a| a.status);
    let message_of = |status: AgentStatus, message: Option<String>| {
        message.filter(|_| status == AgentStatus::Blocked)
    };
    let mut last = Watched {
        status: now,
        message: message_of(now, t.agent.and_then(|a| a.message)),
        timed_out: false,
    };
    on_status(now);
    if is_final(now) {
        return Ok(Some(last));
    }
    // The stream blocks in a thread of its own so the wait can end at the deadline; the thread
    // ends at the next event after that (or when the process does).
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for event in stream {
            if tx.send(event).is_err() {
                break;
            }
        }
    });
    loop {
        let event = match deadline {
            Some(at) => rx.recv_timeout(at.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        let event = match event {
            Ok(Ok(event)) => event,
            Ok(Err(e)) => return Err(e),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                last.timed_out = true;
                return Ok(Some(last));
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(ClientError::Protocol("the event stream ended".into()));
            }
        };
        match event {
            Event::AgentStatusChanged {
                terminal_id,
                status,
                message,
                ..
            } if terminal_id == terminal => {
                on_status(status);
                last = Watched {
                    status,
                    message: message_of(status, message),
                    timed_out: false,
                };
                if is_final(status) {
                    return Ok(Some(last));
                }
            }
            Event::TerminalClosed { terminal_id, .. }
            | Event::TerminalExited { terminal_id, .. }
                if terminal_id == terminal =>
            {
                on_status(AgentStatus::Gone);
                return Ok(Some(Watched {
                    status: AgentStatus::Gone,
                    message: None,
                    timed_out: false,
                }));
            }
            _ => {}
        }
    }
}

/// What to run: the input of [`run_session`].
#[derive(Debug, Clone, Default)]
pub struct SessionRequest {
    /// The id of a harness of the user's config (`claude`, `copilot`, ...).
    pub harness: String,
    pub model: Option<String>,
    pub prompt: String,
    /// A directory on the machine of the server; the space's by default.
    pub cwd: Option<String>,
    /// The space to start in; the space of the calling terminal by default.
    pub space: Option<String>,
    /// The agent-events run; becomes `AGENT_EVENTS_RUN` and `Terminal.run`.
    pub run: Option<String>,
    /// The task of the run (`Terminal.task`, #745).
    pub task: Option<String>,
    /// Added to the terminal's environment.
    pub env: BTreeMap<String, String>,
    /// The name of the tab; `<run id> <title>` by default.
    pub name: Option<String>,
    /// The part of the default name after the run id (the PBI); the first words of the prompt
    /// when absent.
    pub title: Option<String>,
    /// `editor` by default.
    pub placement: Option<Placement>,
}

/// Who runs the session and how it is allowed.
#[derive(Debug, Clone)]
pub struct SessionContext<'a> {
    /// `POIMEN_TERMINAL_ID` of the caller; `None` outside a poimen terminal.
    pub caller: Option<&'a str>,
    /// The home directory whose `config.json` lists the `harnesses`; the defaults without.
    pub home: Option<&'a Path>,
    /// How long a confirmation in the app may take.
    pub confirm_wait: Duration,
}

/// What `run_session` reports while the session runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    /// The terminal exists and the agent reported its start.
    Started { terminal: String, space: String },
    /// The agent's status now or after a change.
    Status(AgentStatus),
    /// The last event: `done`, `error` or `gone`.
    Ended(AgentStatus),
}

/// A session that ended normally.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEnd {
    pub terminal: String,
    pub space: String,
    pub status: AgentStatus,
}

/// Why `run_session` did not end in `done`.
#[derive(Debug)]
pub enum SessionError {
    /// Nothing ran: no such harness or space, a harness without hook reports, a spawn failure, no
    /// `SessionStart` report.
    StartFailed(String),
    /// The session ended with `error` or `gone`.
    Ended {
        status: AgentStatus,
        terminal: String,
        space: String,
    },
    /// The connection to the server failed.
    ConnectionLost(String),
    /// A permission was refused, with the reason.
    PermissionRefused(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionError::StartFailed(m)
            | SessionError::ConnectionLost(m)
            | SessionError::PermissionRefused(m) => f.write_str(m),
            SessionError::Ended {
                status, terminal, ..
            } => write!(f, "the session in terminal {terminal} ended: {status:?}"),
        }
    }
}

impl std::error::Error for SessionError {}

/// A request that failed on the wire (the server did not answer) or at the server (it refused).
fn failed(e: ClientError) -> SessionError {
    match e {
        ClientError::Server { .. } => SessionError::StartFailed(e.to_string()),
        _ => SessionError::ConnectionLost(e.to_string()),
    }
}

fn start_error(e: AgentError) -> SessionError {
    match e {
        AgentError::Client(e) => failed(e),
        other => SessionError::StartFailed(other.to_string()),
    }
}

/// The name of the tab: `<run> <title>`, the title being the given one or the first words of the
/// prompt; without a run only the title.
fn tab_name(req: &SessionRequest) -> String {
    let title = req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            let words: Vec<&str> = req.prompt.split_whitespace().take(TITLE_WORDS).collect();
            if words.is_empty() {
                req.harness.clone()
            } else {
                words.join(" ")
            }
        });
    match req.run.as_deref().filter(|r| !r.is_empty()) {
        Some(run) => format!("{run} {title}"),
        None => title,
    }
}

/// Runs a harness session in a new terminal and waits for its end (ADR 0016).
///
/// `on_event` sees `Started` once the agent reported its start, then every status change, then
/// `Ended`. `Done` returns `Ok`; `error`, `gone`, a closed or exited terminal and every failure
/// are a [`SessionError`]. A `blocked` agent does not end the wait: the user answers it in the
/// pane. The terminal is left open when the session ends.
pub fn run_session(
    client: &Client,
    ctx: &SessionContext<'_>,
    req: &SessionRequest,
    mut on_event: impl FnMut(SessionEvent),
) -> Result<SessionEnd, SessionError> {
    let harnesses = harnesses_of(ctx.home);
    let found = harnesses
        .iter()
        .find(|h| h.id == req.harness)
        .ok_or_else(|| {
            let known: Vec<_> = harnesses.iter().map(|h| h.id.as_str()).collect();
            SessionError::StartFailed(format!(
                "unknown harness `{}`; known: {}",
                req.harness,
                known.join(", ")
            ))
        })?;
    if !poimen_protocol::HOOK_KINDS.contains(&found.id.as_str()) {
        return Err(SessionError::StartFailed(format!(
            "harness `{}` reports no agent status (no hooks), so its session cannot be followed; \
             use one of: {}",
            found.id,
            poimen_protocol::HOOK_KINDS.join(", ")
        )));
    }
    // The prompt is one argument per `promptArgs` element, or typed after `SessionStart`.
    let by_args = !found.prompt_args.is_empty();
    let prompt = Some(req.prompt.as_str()).filter(|p| !p.trim().is_empty());
    let argv = harness::build_argv(found, req.model.as_deref(), prompt.filter(|_| by_args));
    let (program, rest) = argv.split_first().ok_or_else(|| {
        SessionError::StartFailed(format!("harness `{}` has no command", found.id))
    })?;

    let space_id = match req.space.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => s.to_owned(),
        None => {
            let caller = ctx.caller.filter(|i| !i.is_empty()).ok_or_else(|| {
                SessionError::PermissionRefused(
                    "refused: no space given and no calling terminal to take it from".into(),
                )
            })?;
            client
                .list_terminals(None)
                .map_err(failed)?
                .into_iter()
                .find(|t| t.id == caller)
                .map(|t| t.space_id)
                .ok_or_else(|| {
                    SessionError::PermissionRefused(format!(
                        "terminal {caller} is not known to the server"
                    ))
                })?
        }
    };
    if !client
        .list_spaces()
        .map_err(failed)?
        .iter()
        .any(|s| s.id == space_id)
    {
        return Err(SessionError::StartFailed(format!(
            "space {space_id} not found"
        )));
    }
    let command_line = argv.join(" ");
    let confirmed = authorize_write(
        client,
        ctx.caller,
        &WriteAsk {
            space_id: &space_id,
            action: AgentWriteAction::StartAgent,
            terminal_id: None,
            text: &command_line,
            always_ask: false,
            worktree: false,
            grant: None,
        },
        ctx.confirm_wait,
    )
    .map_err(|e| match e {
        WriteError::Refused(m) | WriteError::GrantLost { message: m, .. } => {
            SessionError::PermissionRefused(m)
        }
        WriteError::Client(e) => failed(e),
    })?;

    let mut env = req.env.clone();
    if let Some(run) = req.run.as_deref().filter(|r| !r.is_empty()) {
        env.insert(RUN_ENV.into(), run.to_owned());
    }
    let terminal = client
        .create_terminal(&TerminalCreate {
            space_id: space_id.clone(),
            cwd: req.cwd.clone().filter(|c| !c.is_empty()),
            name: Some(
                req.name
                    .clone()
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| tab_name(req)),
            ),
            env,
            started_by: ctx.caller.map(str::to_owned),
            run: req.run.clone().filter(|r| !r.is_empty()),
            task: req.task.clone().filter(|t| !t.is_empty()),
            placement: req.placement,
            ..Default::default()
        })
        .map_err(failed)?;
    if let Err(e) = client.start_agent(&terminal.id, program, rest, START_TIMEOUT) {
        // The agent never started: the empty terminal is not left behind.
        let _ = client.close_terminal(&terminal.id);
        return Err(start_error(e));
    }
    if let Some(caller) = ctx.caller.filter(|i| !i.is_empty()) {
        // A failed notice does not undo the start.
        let _ = client.notice_agent_write(&poimen_protocol::AgentWriteNotice {
            caller_terminal_id: caller.to_owned(),
            action: AgentWriteAction::StartAgent,
            space_id: space_id.clone(),
            terminal_id: Some(terminal.id.clone()),
            text: command_line,
            confirmed,
        });
    }
    on_event(SessionEvent::Started {
        terminal: terminal.id.clone(),
        space: space_id.clone(),
    });
    if let (false, Some(text)) = (by_args, prompt) {
        match client.prompt_agent(&terminal.id, text, PROMPT_WAIT) {
            Ok(PromptResult::Started(_) | PromptResult::Stalled) => {}
            Ok(refused) => {
                return Err(SessionError::StartFailed(format!(
                    "the prompt was not delivered: {refused:?}"
                )));
            }
            Err(e) => return Err(start_error(e)),
        }
    }
    let watched = watch_agent(
        client,
        &terminal.id,
        None,
        |s| {
            matches!(
                s,
                AgentStatus::Done | AgentStatus::Error | AgentStatus::Gone
            )
        },
        |s| on_event(SessionEvent::Status(s)),
    )
    .map_err(failed_while_waiting)?
    .ok_or_else(|| {
        SessionError::ConnectionLost(format!(
            "terminal {} is not known to the server",
            terminal.id
        ))
    })?;
    on_event(SessionEvent::Ended(watched.status));
    match watched.status {
        AgentStatus::Done => Ok(SessionEnd {
            terminal: terminal.id,
            space: space_id,
            status: AgentStatus::Done,
        }),
        status => Err(SessionError::Ended {
            status,
            terminal: terminal.id,
            space: space_id,
        }),
    }
}

/// Once the session runs, any failed request is a lost connection.
fn failed_while_waiting(e: ClientError) -> SessionError {
    SessionError::ConnectionLost(e.to_string())
}
