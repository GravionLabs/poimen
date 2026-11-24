//! `request_grant` and `revoke_grant` (#1211, ADR 0018): a lead agent asks the user once for a
//! plan over several spaces; inside the grant its writes need no dialog per call. The server holds
//! the grant and checks every write (`agent.grant_check`); this process only asks and revokes.

use std::time::Duration;

use poimen_client::Client;
use poimen_client::protocol::{
    AgentGrantRequest, AgentGrantRevoke, AgentWriteAction, ConfirmReason, GRANT_MAX_SECS,
    GRANT_WAIT_MS,
};
use serde_json::{Value, json};

use super::{Env, Fail, runs, server_error, text_arg};

/// The tool definition of `request_grant`.
pub(super) fn request_tool() -> Value {
    json!({
        "name": "request_grant",
        "description": "Asks the user ONCE to let you coordinate a plan over several spaces without a dialog per write. The user sees your plan, the spaces, the tools and the limit in the poimen app and can deny it or narrow it (fewer spaces, a lower limit); waits up to 2 minutes, a timeout or no poimen app counts as no. The answer says what was granted. Inside the grant, start_agent, prompt_agent, start_shell, stop_terminal and create_space in the granted spaces need no confirmation, but every write still shows in the user's notification center; a write outside it (another space, write_terminal into a terminal you did not start, a worktree start) still asks per call. At most `max_agents` agents you started may run at once; a start over the limit is refused, wait for one to finish. The grant belongs to this terminal, never covers another machine, ends at the first of `duration_hours`, your terminal ending, revoke_grant or the user's revocation, and is lost when the poimen server restarts (ask again). Running agents are never stopped by it ending. `spaces` are space ids from list_spaces (create_space makes one for a repository from list_repositories first); names are not matched. Ask once for the whole plan, before the first start_shell or start_agent, and write what you got with record_run `grant`.",
        "inputSchema": {"type": "object", "properties": {
            "plan": {"type": "string", "description": "What you are going to do, in plain words: the user decides on this text."},
            "spaces": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Space ids from list_spaces."},
            "tools": {"type": "array", "minItems": 1, "items": {"type": "string", "enum": ["start_agent", "prompt_agent", "start_shell", "stop_terminal", "create_space"]}, "description": "The write tools the grant covers."},
            "max_agents": {"type": "integer", "minimum": 1, "description": "The most agents you started that may run at once, across the granted spaces."},
            "duration_hours": {"type": "number", "exclusiveMinimum": 0, "maximum": 24, "description": "How long the grant lasts; 8 by default."}
        }, "required": ["plan", "spaces", "tools", "max_agents"], "additionalProperties": false},
    })
}

/// The tool definition of `revoke_grant`.
pub(super) fn revoke_tool() -> Value {
    json!({
        "name": "revoke_grant",
        "description": "Ends the grant of this terminal (see request_grant); call it when your plan is done, after the last stop_terminal or finish of record_run. Later writes in other spaces ask per call again. It stops no agent and sends no input.",
        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
    })
}

fn caller<'a>(env: &Env<'a>) -> Result<&'a str, Fail> {
    env.terminal_id.filter(|i| !i.is_empty()).ok_or_else(|| {
        Fail::Tool(
            "refused: this agent does not run in a poimen terminal (POIMEN_TERMINAL_ID is not set)"
                .into(),
        )
    })
}

fn strings(args: &Value, key: &str) -> Result<Vec<String>, Fail> {
    args.get(key)
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
        .and_then(|a| {
            a.iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
        })
        .ok_or_else(|| Fail::Tool(format!("`{key}` is a non-empty list of strings")))
}

/// `request_grant {plan, spaces, tools, max_agents, duration_hours?}`.
pub(super) fn request_grant(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    runs::only_keys(
        args,
        &["plan", "spaces", "tools", "max_agents", "duration_hours"],
    )?;
    let caller = caller(env)?;
    let plan =
        text_arg(args, "plan").ok_or_else(|| Fail::Tool("missing argument `plan`".into()))?;
    let spaces = strings(args, "spaces")?;
    let tools = strings(args, "tools")?
        .into_iter()
        .map(|t| {
            serde_json::from_value::<AgentWriteAction>(json!(t))
                .ok()
                .filter(|a| *a != AgentWriteAction::WriteTerminal)
                .ok_or_else(|| {
                    Fail::Tool(format!(
                        "`tools`: {t} is not a write tool a grant can cover (write_terminal never is)"
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let max_agents = args
        .get("max_agents")
        .and_then(Value::as_u64)
        .filter(|n| *n >= 1)
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| Fail::Tool("`max_agents` is an integer of at least 1".into()))?;
    let duration_s = match args.get("duration_hours") {
        None | Some(Value::Null) => None,
        Some(v) => {
            let hours = v
                .as_f64()
                .filter(|h| *h > 0.0 && *h <= (GRANT_MAX_SECS / 3600) as f64)
                .ok_or_else(|| Fail::Tool("`duration_hours` is above 0 and at most 24".into()))?;
            Some((hours * 3600.0).round() as u64)
        }
    };
    let wait = env
        .confirm_wait
        .saturating_mul(2)
        .min(Duration::from_millis(GRANT_WAIT_MS));
    let answer = client
        .request_agent_grant(
            &AgentGrantRequest {
                caller_terminal_id: caller.to_owned(),
                plan: plan.to_owned(),
                spaces,
                tools,
                max_agents,
                duration_s,
                wait_ms: None,
            },
            wait,
        )
        .map_err(server_error)?;
    let note = match answer.reason {
        ConfirmReason::Approved => None,
        ConfirmReason::Declined => Some("the user declined the grant"),
        ConfirmReason::Timeout => Some("the user did not answer in time; counts as no"),
        ConfirmReason::NoApp => Some("poimen is not open, so nobody can confirm a grant"),
    };
    Ok(json!({"granted": answer.granted, "grant": answer.grant, "note": note}))
}

/// `revoke_grant {}`.
pub(super) fn revoke_grant(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    runs::only_keys(args, &[])?;
    let caller = caller(env)?;
    let revoked = client
        .revoke_agent_grant(&AgentGrantRevoke {
            id: None,
            caller_terminal_id: Some(caller.to_owned()),
        })
        .map_err(server_error)?;
    Ok(json!({"revoked": revoked}))
}
