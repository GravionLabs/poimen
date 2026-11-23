//! `stop_terminal` (#1183): ends what a terminal runs, by Ctrl-C or by closing the pane. The
//! permission model is the one of `start_agent`.

use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, AgentWriteAction, Terminal, TerminalStatus};
use serde_json::{Value, json};

use super::{Env, Fail, announce, authorize, runs, server_error, text_arg};

/// How long an interrupt may take to end the process before the answer says it still runs.
const INTERRUPT_WAIT: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(100);

fn find(client: &Client, id: &str) -> Result<Option<Terminal>, Fail> {
    let terminals = client.list_terminals(None).map_err(server_error)?;
    Ok(terminals.into_iter().find(|t| t.id == id))
}

fn has_running_agent(t: &Terminal) -> bool {
    t.agent
        .as_ref()
        .is_some_and(|a| a.status != AgentStatus::Gone)
}

/// The tool definition of `tools/list`.
pub(super) fn tool() -> Value {
    json!({
        "name": "stop_terminal",
        "description": "Ends what a terminal runs, e.g. the deliver loop you started with start_shell. `interrupt` (default) sends Ctrl-C (SIGINT) to the foreground process and waits a few seconds; `close` closes the pane and ends its processes. Refuses `close` of a terminal with a running agent unless `force` is true. The answer's `outcome` is `ended`, `running` (still alive, e.g. it ignored Ctrl-C) or `gone` (it had exited already). Afterwards `get_terminal` shows the state. In your own space it just runs; in any other space the user must confirm in the poimen app (waits up to 60 s).",
        "inputSchema": {"type": "object", "properties": {
            "terminal": {"type": "string", "description": "Terminal id."},
            "mode": {"type": "string", "enum": ["interrupt", "close"], "description": "`interrupt` (default) or `close`."},
            "force": {"type": "boolean", "description": "Allow `close` of a terminal with a running agent."}
        }, "required": ["terminal"], "additionalProperties": false},
    })
}

/// `stop_terminal {terminal, mode?, force?}`.
pub(super) fn stop_terminal(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    runs::only_keys(args, &["terminal", "mode", "force"])?;
    let id = text_arg(args, "terminal")
        .ok_or_else(|| Fail::Tool("missing argument `terminal`".into()))?;
    let close = match text_arg(args, "mode").unwrap_or("interrupt") {
        "interrupt" => false,
        "close" => true,
        other => {
            return Err(Fail::Tool(format!(
                "`mode` must be `interrupt` or `close`, not `{other}`"
            )));
        }
    };
    let force = match args.get("force") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(Fail::Tool("`force` must be a boolean".into())),
    };
    let mode = if close { "close" } else { "interrupt" };
    let Some(target) = find(client, id)? else {
        return Ok(json!({"terminal": id, "mode": mode, "outcome": "gone",
            "note": "no terminal with this id: it was closed already or never existed"}));
    };
    if close && has_running_agent(&target) && !force {
        return Err(Fail::Tool(format!(
            "refused: terminal {id} runs an agent; closing it ends the agent. Set `force` to close it anyway"
        )));
    }
    let confirmed = authorize(
        client,
        env,
        &target.space_id,
        AgentWriteAction::StopTerminal,
        Some(id),
        mode,
        false,
    )?;
    let outcome = if close {
        client.close_terminal(id).map_err(server_error)?;
        if target.status == TerminalStatus::Exited {
            "gone"
        } else {
            "ended"
        }
    } else if target.status == TerminalStatus::Exited {
        "gone"
    } else {
        client.input(id, "\u{3}").map_err(server_error)?;
        interrupted(client, id)?
    };
    announce(
        client,
        env,
        &target.space_id,
        AgentWriteAction::StopTerminal,
        Some(id),
        mode,
        confirmed,
    );
    let mut answer = json!({"terminal": id, "mode": mode, "outcome": outcome});
    match (outcome, close) {
        ("running", _) => {
            answer["note"] = json!(
                "Ctrl-C was sent; the process still runs (it may be shutting down or ignore it). \
                 Check get_terminal, or use mode `close`."
            );
        }
        ("gone", false) => {
            answer["note"] = json!("the process had already exited; nothing was sent");
        }
        ("gone", true) => {
            answer["note"] = json!("the process had already exited; the pane is closed");
        }
        _ => {}
    }
    Ok(answer)
}

/// Waits until the process of the terminal ended after Ctrl-C: `ended`, else `running`.
fn interrupted(client: &Client, id: &str) -> Result<&'static str, Fail> {
    let deadline = Instant::now() + INTERRUPT_WAIT;
    loop {
        match find(client, id)? {
            None => return Ok("ended"),
            Some(t) if t.status == TerminalStatus::Exited => return Ok("ended"),
            Some(_) if Instant::now() >= deadline => return Ok("running"),
            Some(_) => std::thread::sleep(POLL),
        }
    }
}
