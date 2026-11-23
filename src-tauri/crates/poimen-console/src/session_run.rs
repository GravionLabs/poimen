//! `poimen session run` (#1257, ADR 0016): the CLI side of `poimen_client::agent::run_session`.
//! One JSON object per line on stdout while the session runs, diagnostics on stderr, an exit
//! code per outcome (the table is in `docs/agents/console.md` and the `poimen` help).

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use poimen_client::agent::{
    SessionContext, SessionEnd, SessionError, SessionEvent, SessionRequest, harnesses_of,
    run_session,
};
use poimen_client::protocol::{AgentStatus, TerminalStatus};
use poimen_client::{Client, ClientError};
use serde_json::json;

use crate::cli::SessionRunArgs;

pub const EXIT_DONE: u8 = 0;
pub const EXIT_START_FAILED: u8 = 3;
pub const EXIT_ENDED: u8 = 4;
pub const EXIT_CONNECTION: u8 = 5;
pub const EXIT_REFUSED: u8 = 6;
pub const EXIT_SIGINT: u8 = 130;
pub const EXIT_SIGTERM: u8 = 143;

/// How long `--close-on-end` waits for the agent to leave after its `exitInput`.
const EXIT_WAIT: Duration = Duration::from_secs(3);
/// How often the wait looks at the signal flags.
const TICK: Duration = Duration::from_millis(50);

/// Who runs the command: the caller terminal, the home of the harness config, the confirm wait.
#[derive(Debug, Clone)]
pub struct Env {
    pub caller: Option<String>,
    pub home: Option<PathBuf>,
    pub confirm_wait: Duration,
}

/// The signals that end the wait, set by the process's handlers.
#[derive(Debug, Default)]
pub struct Signals {
    pub int: Arc<AtomicBool>,
    pub term: Arc<AtomicBool>,
}

enum Msg {
    Event(SessionEvent),
    Finished(Result<SessionEnd, SessionError>),
}

fn word(s: AgentStatus) -> &'static str {
    match s {
        AgentStatus::Idle => "idle",
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Error => "error",
        AgentStatus::Gone => "gone",
    }
}

fn record(out: &mut dyn Write, value: serde_json::Value) {
    // A reader that went away does not stop the session.
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

/// Runs the session and returns the exit code.
pub fn run(
    client: &Client,
    env: &Env,
    args: &SessionRunArgs,
    signals: &Signals,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let request = SessionRequest {
        harness: args.harness.clone(),
        model: args.model.clone(),
        prompt: args.prompt.clone(),
        cwd: args.cwd.clone(),
        space: args.space.clone(),
        run: args.run.clone(),
        task: args.task.clone(),
        env: args.env.iter().cloned().collect(),
        name: args.name.clone(),
        title: None,
        placement: args.placement,
    };
    let (tx, rx) = mpsc::channel();
    {
        let (client, env) = (client.clone(), env.clone());
        std::thread::spawn(move || {
            let ctx = SessionContext {
                caller: env.caller.as_deref(),
                home: env.home.as_deref(),
                confirm_wait: env.confirm_wait,
            };
            let events = tx.clone();
            let result = run_session(&client, &ctx, &request, |e| {
                let _ = events.send(Msg::Event(e));
            });
            let _ = tx.send(Msg::Finished(result));
        });
    }
    let mut terminal: Option<String> = None;
    let mut last: Option<AgentStatus> = None;
    let result = loop {
        let signal = if signals.int.load(Ordering::SeqCst) {
            Some(EXIT_SIGINT)
        } else if signals.term.load(Ordering::SeqCst) {
            Some(EXIT_SIGTERM)
        } else {
            None
        };
        if let Some(code) = signal {
            let _ = writeln!(err, "poimen: interrupted; the session keeps running");
            if args.close_on_end {
                close(client, env, &args.harness, terminal.as_deref(), err);
            }
            return code;
        }
        match rx.recv_timeout(TICK) {
            Ok(Msg::Event(SessionEvent::Started { terminal: t, space })) => {
                record(
                    out,
                    json!({"type": "started", "terminal": t, "space": space}),
                );
                terminal = Some(t);
            }
            Ok(Msg::Event(SessionEvent::Status(s))) => {
                if s != AgentStatus::Idle && last != Some(s) {
                    record(out, json!({"type": "status", "status": word(s)}));
                }
                last = Some(s);
            }
            Ok(Msg::Event(SessionEvent::Ended(s))) => {
                record(
                    out,
                    json!({"type": "ended", "status": word(s), "terminal": terminal}),
                );
            }
            Ok(Msg::Finished(r)) => break r,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                break Err(SessionError::ConnectionLost(
                    "the session thread ended".into(),
                ));
            }
        }
    };
    let code = match &result {
        Ok(_) => EXIT_DONE,
        Err(SessionError::StartFailed(m)) => {
            let _ = writeln!(err, "poimen: {m}");
            EXIT_START_FAILED
        }
        Err(e @ SessionError::Ended { .. }) => {
            let _ = writeln!(err, "poimen: {e}");
            EXIT_ENDED
        }
        Err(SessionError::ConnectionLost(m)) => {
            let _ = writeln!(err, "poimen: connection to the server lost: {m}");
            EXIT_CONNECTION
        }
        Err(SessionError::PermissionRefused(m)) => {
            let _ = writeln!(err, "poimen: {m}");
            EXIT_REFUSED
        }
    };
    // A failed start closed its own terminal already.
    if args.close_on_end && !matches!(result, Err(SessionError::StartFailed(_))) {
        close(client, env, &args.harness, terminal.as_deref(), err);
    }
    code
}

/// Ends the agent with the harness's `exitInput`, waits briefly for it to leave, closes the
/// terminal. Failures are diagnostics: the exit code is the session's.
fn close(client: &Client, env: &Env, harness: &str, terminal: Option<&str>, err: &mut dyn Write) {
    let Some(terminal) = terminal else { return };
    let exit_input = harnesses_of(env.home.as_deref())
        .into_iter()
        .find(|h| h.id == harness)
        .and_then(|h| h.exit_input);
    if let Some(text) = exit_input
        && client.input(terminal, &format!("{text}\r")).is_ok()
    {
        let deadline = Instant::now() + EXIT_WAIT;
        while Instant::now() < deadline && !agent_left(client, terminal) {
            std::thread::sleep(TICK);
        }
    }
    match client.close_terminal(terminal) {
        Ok(()) | Err(ClientError::Server { .. }) => {}
        Err(e) => {
            let _ = writeln!(err, "poimen: could not close terminal {terminal}: {e}");
        }
    }
}

fn agent_left(client: &Client, terminal: &str) -> bool {
    match client.list_terminals(None) {
        Ok(all) => all.iter().find(|t| t.id == terminal).is_none_or(|t| {
            t.status == TerminalStatus::Exited
                || t.agent
                    .as_ref()
                    .is_none_or(|a| a.status == AgentStatus::Gone)
        }),
        Err(_) => true,
    }
}
