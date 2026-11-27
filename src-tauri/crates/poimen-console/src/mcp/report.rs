//! `report_done` (#980): the agent that was started with `start_agent` tells its starter that it is
//! finished and what came out. The server stores the report on the terminal and types one line into
//! the starting agent when it is idle (`poimen-server` `reports.rs`); here the report also becomes
//! the `run.finished` of the agent's run in the user's agent-events, which the Runs view shows.

use std::path::Path;

use poimen_client::Client;
use poimen_client::protocol::Terminal;
use poimen_core::agent_events::{self, RepoRef};
use poimen_protocol::files::{now_secs, utc};
use serde_json::{Value, json};

use super::{Env, Fail, run_files, run_writer, server_error, text_arg};

/// Appended to the prompt `start_agent` gives an agent, so it knows how to end.
pub(super) const PREAMBLE: &str = "When you are finished, call the poimen MCP tool `report_done` once, with `outcome` (succeeded, failed or cancelled), a `summary` of at most three sentences and the `files` you changed. Report once and do not call it again.";

/// The prompt with the preamble below it.
pub(super) fn with_preamble(prompt: &str) -> String {
    format!("{}\n\n{PREAMBLE}", prompt.trim_end())
}

const OUTCOMES: [&str; 3] = ["succeeded", "failed", "cancelled"];

/// The longest `reason` written to the run (the summary is kept whole on the terminal).
const REASON_CHARS: usize = 600;

/// `report_done {outcome, summary, files?}`.
pub(super) fn report_done(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    let caller = env.terminal_id.filter(|i| !i.is_empty()).ok_or_else(|| {
        Fail::Tool(
            "refused: this agent does not run in a poimen terminal (POIMEN_TERMINAL_ID is not set)"
                .into(),
        )
    })?;
    let outcome = text_arg(args, "outcome")
        .filter(|o| OUTCOMES.contains(o))
        .ok_or_else(|| Fail::Tool("`outcome` is succeeded, failed or cancelled".into()))?;
    let summary =
        text_arg(args, "summary").ok_or_else(|| Fail::Tool("missing argument `summary`".into()))?;
    let files: Vec<String> = match args.get("files") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|f| f.as_str().map(str::to_owned))
            .collect::<Option<_>>()
            .ok_or_else(|| Fail::Tool("`files` is a list of paths".into()))?,
        Some(_) => return Err(Fail::Tool("`files` is a list of paths".into())),
    };
    let terminals = client.list_terminals(None).map_err(server_error)?;
    let own = terminals
        .iter()
        .find(|t| t.id == caller)
        .ok_or_else(|| Fail::Tool(format!("terminal {caller} is not known to the server")))?;
    if own.report.is_some() {
        return Err(Fail::Tool(
            "refused: this agent has already reported; report once".into(),
        ));
    }
    let stored = client
        .request(
            "terminal.report_done",
            json!({"terminal_id": caller, "outcome": outcome, "summary": summary, "files": files}),
        )
        .map_err(server_error)?;
    let run = env.events_dir.as_deref().map_or(Value::Null, |shared| {
        write_run_finished(shared, own, outcome, &stored["summary"], &stored["files"])
    });
    // A lead's grant ends with its run (ADR 0018); for any other agent there is none to revoke.
    let _ = client.revoke_agent_grant(&poimen_protocol::AgentGrantRevoke {
        id: None,
        caller_terminal_id: Some(caller.to_owned()),
    });
    Ok(json!({
        "terminal": caller,
        "reported": true,
        "outcome": outcome,
        "starter": own.started_by,
        "run": run,
    }))
}

/// `wait_agent`'s answer with the stored report of the terminal when there is one.
pub(super) fn with_report(client: &Client, answer: Value) -> Value {
    let mut answer = answer;
    let Some(id) = answer.get("terminal").and_then(Value::as_str) else {
        return answer;
    };
    let report = client
        .list_terminals(None)
        .ok()
        .and_then(|ts| ts.into_iter().find(|t| t.id == id))
        .and_then(|t| t.report);
    if let (Some(report), Some(o)) = (report, answer.as_object_mut()) {
        o.insert("report".into(), json!(report));
    }
    answer
}

/// The `run.finished` for the run of the terminal: appended to its running run, else a new run
/// file with `run.started` and `run.finished`, in the shared directory. The run id, or `null` when
/// it could not be written.
fn write_run_finished(
    shared: &Path,
    t: &Terminal,
    outcome: &str,
    summary: &Value,
    files: &Value,
) -> Value {
    let repo = RepoRef::of(&t.cwd);
    let origin = run_writer::origin(&t.cwd, repo.as_ref());
    let tone = match outcome {
        "succeeded" => "success",
        "failed" => "danger",
        _ => "neutral",
    };
    let reason: String = summary
        .as_str()
        .unwrap_or_default()
        .chars()
        .take(REASON_CHARS)
        .collect();
    let finished = |run: &str, seq: i64| {
        json!({
            "v": 1, "seq": seq, "ts": utc(now_secs()), "run": run, "type": "run.finished",
            "tone": tone, "outcome": outcome, "reason": reason, "files": files,
        })
    };
    if let Some(r) = running_run_of(shared, repo.as_ref(), t) {
        let end = finished(&r.run, r.last_seq + 1);
        return run_writer::append(shared, &r.run, &r.events, &origin, &[end])
            .map_or(Value::Null, |()| json!(r.run));
    }
    let run = run_writer::new_run_id("agent-");
    let mut host = serde_json::Map::new();
    host.insert("poimen.terminal".into(), json!(t.id));
    let started = json!({
        "v": 1, "seq": 1, "ts": utc(now_secs()), "run": run, "type": "run.started",
        "tone": "neutral", "host": host, "origin": origin,
        "agent": {"harness": t.program},
        "subject": {"kind": "poimen.agent", "id": t.id, "title": t.name},
    });
    let end = finished(&run, 2);
    run_writer::append(shared, &run, &[], &origin, &[started, end])
        .map_or(Value::Null, |()| json!(run))
}

/// A run that is still running, with its events so far.
struct Running {
    run: String,
    last_seq: i64,
    events: Vec<Value>,
}

/// The run still running whose `host["poimen.terminal"]` is the terminal. Terminal ids are only
/// unique per server, so a run of the shared directory counts when the server gave the terminal
/// that run, or its origin is this machine and the terminal's repository (outside one: an origin
/// without a repository). The legacy directory of the repository holds runs started before the
/// upgrade.
fn running_run_of(shared: &Path, repo: Option<&RepoRef>, t: &Terminal) -> Option<Running> {
    let now = now_secs() as f64;
    let mut attribution = run_files::Attribution::default();
    let host = run_files::host_name();
    let mut candidates = match repo {
        Some(repo) => run_files::of_repository(Some(shared), repo, &mut attribution),
        None => run_files::list(shared)
            .into_iter()
            .filter(|(_, p)| {
                run_files::origin_of(p).is_some_and(|o| o.repo.is_none() && o.worktree.is_none())
            })
            .map(|(run, p)| (run, vec![p]))
            .collect(),
    };
    if let Some(run) = &t.run
        && let Some(path) = run_files::path_of(shared, run).filter(|p| p.is_file())
    {
        candidates.entry(run.clone()).or_insert_with(|| vec![path]);
    }
    candidates.into_iter().rev().find_map(|(id, files)| {
        let events = run_files::events(&files);
        let run = run_files::fold(&events).snapshot(now, &agent_events::pid_alive)?;
        let here = run
            .origin
            .as_ref()
            .and_then(|o| o.host.as_deref())
            .is_none_or(|h| host.is_empty() || h.eq_ignore_ascii_case(&host));
        (run.state.status == "running"
            && (here || t.run.as_deref() == Some(id.as_str()))
            && run.host.get("poimen.terminal") == Some(&t.id))
        .then_some(Running {
            run: id,
            last_seq: run.state.last_seq,
            events,
        })
    })
}
