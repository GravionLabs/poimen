//! `report_done` (#980): the agent that was started with `start_agent` tells its starter that it is
//! finished and what came out. The server stores the report on the terminal and types one line into
//! the starting agent when it is idle (`poimen-server` `reports.rs`); here the report also becomes
//! the `run.finished` of the agent's run in the repository's agent-events, which the Runs view shows.

use std::io::Write;
use std::path::Path;

use poimen_client::Client;
use poimen_client::protocol::Terminal;
use poimen_core::agent_events::{self, MAX_EVENTS_BYTES, Reducer};
use poimen_core::git;
use poimen_protocol::files::{now_secs, utc};
use serde_json::{Value, json};

use super::{Env, Fail, server_error, text_arg};

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
    let run = write_run_finished(own, outcome, &stored["summary"], &stored["files"]);
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
/// file with `run.started` and `run.finished`. The run id, or `null` when it could not be written
/// (the terminal is not in a git repository).
fn write_run_finished(t: &Terminal, outcome: &str, summary: &Value, files: &Value) -> Value {
    let Some((_, common)) = git::roots(&t.cwd) else {
        return Value::Null;
    };
    let dir = Path::new(&common).join(agent_events::EVENTS_PATH);
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
    if let Some((run, last_seq)) = running_run_of(&dir, &t.id) {
        return append(&dir, &run, &[finished(&run, last_seq + 1)])
            .map_or(Value::Null, |()| json!(run));
    }
    let run = format!("agent-{}-{}", t.id, now_secs());
    let mut host = serde_json::Map::new();
    host.insert("poimen.terminal".into(), json!(t.id));
    let started = json!({
        "v": 1, "seq": 1, "ts": utc(now_secs()), "run": run, "type": "run.started",
        "tone": "neutral", "host": host,
        "agent": {"harness": t.program},
        "subject": {"kind": "poimen.agent", "id": t.id, "title": t.name},
    });
    append(&dir, &run, &[started, finished(&run, 2)]).map_or(Value::Null, |()| json!(run))
}

pub(super) fn append(dir: &Path, run: &str, events: &[Value]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(format!("{run}.jsonl")))?;
    let mut text = String::new();
    for e in events {
        text.push_str(&e.to_string());
        text.push('\n');
    }
    file.write_all(text.as_bytes())
}

/// The id and last `seq` of the run still running whose `host["poimen.terminal"]` is `terminal`.
fn running_run_of(dir: &Path, terminal: &str) -> Option<(String, i64)> {
    let now = now_secs() as f64;
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| agent_events::is_run_file(p))
        .collect();
    files.sort();
    files.into_iter().rev().find_map(|path| {
        let regular =
            std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_EVENTS_BYTES);
        let text = regular
            .then(|| std::fs::read_to_string(&path).ok())
            .flatten()?;
        let complete = text.rfind('\n').map_or("", |i| &text[..=i]);
        let mut reducer = Reducer::new();
        for line in complete.lines().filter(|l| !l.trim().is_empty()) {
            reducer.push_line(line);
        }
        let run = reducer.snapshot(now, &agent_events::pid_alive)?;
        (run.state.status == "running"
            && run.host.get("poimen.terminal").map(String::as_str) == Some(terminal))
        .then(|| (run.state.run.clone(), run.state.last_seq))
    })
}
