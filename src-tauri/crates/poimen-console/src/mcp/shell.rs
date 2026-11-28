//! `start_shell` (#1181): a new terminal of a space that runs a command, for work that outlives a
//! tool call (a scripted run). The permission model is the one of `start_agent`.

use std::collections::BTreeMap;

use poimen_client::Client;
use poimen_client::protocol::{AgentWriteAction, TerminalCreate};
use serde_json::{Value, json};

use super::{Env, Fail, announce, authorize, runs, server_error, text_arg};

/// `command` as an argv list: a non-empty array of strings, the first one not blank.
fn command_arg(args: &Value) -> Result<Vec<String>, Fail> {
    let bad = || {
        Fail::Tool(
            "`command` must be a non-empty list of strings (argv, not a shell string)".into(),
        )
    };
    let list = args
        .get("command")
        .and_then(Value::as_array)
        .ok_or_else(bad)?;
    let argv: Vec<String> = list
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or_else(bad))
        .collect::<Result<_, _>>()?;
    if argv.first().is_none_or(|p| p.trim().is_empty()) {
        return Err(bad());
    }
    Ok(argv)
}

fn env_arg(args: &Value) -> Result<BTreeMap<String, String>, Fail> {
    let Some(value) = args.get("env").filter(|v| !v.is_null()) else {
        return Ok(BTreeMap::new());
    };
    let bad = || Fail::Tool("`env` must be an object of strings".into());
    value
        .as_object()
        .ok_or_else(bad)?
        .iter()
        .map(|(k, v)| {
            let v = v.as_str().ok_or_else(bad)?;
            if k.is_empty() || k.contains('=') {
                return Err(Fail::Tool(format!("bad environment variable name `{k}`")));
            }
            Ok((k.clone(), v.to_owned()))
        })
        .collect()
}

/// `start_shell {command, cwd?, name?, space?, env?}`.
pub(super) fn start_shell(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    runs::only_keys(args, &["command", "cwd", "name", "space", "env"])?;
    let argv = command_arg(args)?;
    let extra = env_arg(args)?;
    let spaces = client.list_spaces().map_err(server_error)?;
    let space_id = match text_arg(args, "space") {
        Some(s) => s.to_owned(),
        None => {
            let caller = env.terminal_id.filter(|i| !i.is_empty()).ok_or_else(|| {
                Fail::Tool(
                    "refused: no `space` given and no calling terminal to take it from".into(),
                )
            })?;
            let terminals = client.list_terminals(None).map_err(server_error)?;
            terminals
                .iter()
                .find(|t| t.id == caller)
                .map(|t| t.space_id.clone())
                .ok_or_else(|| {
                    Fail::Tool(format!("terminal {caller} is not known to the server"))
                })?
        }
    };
    if !spaces.iter().any(|s| s.id == space_id) {
        return Err(Fail::Tool(format!("space {space_id} not found")));
    }
    let command_line = argv.join(" ");
    let confirmed = authorize(
        client,
        env,
        &space_id,
        AgentWriteAction::StartShell,
        None,
        &command_line,
        false,
    )?;
    let program = argv[0].rsplit('/').next().unwrap_or(&argv[0]).to_owned();
    let terminal = client
        .create_terminal(&TerminalCreate {
            space_id: space_id.clone(),
            cwd: text_arg(args, "cwd").map(str::to_owned),
            name: Some(text_arg(args, "name").map_or(program, str::to_owned)),
            command: Some(argv),
            env: extra,
            started_by: env.terminal_id.map(str::to_owned),
            ..Default::default()
        })
        .map_err(server_error)?;
    announce(
        client,
        env,
        &space_id,
        AgentWriteAction::StartShell,
        Some(&terminal.id),
        &command_line,
        confirmed,
    );
    Ok(json!({"terminal": terminal.id, "space": space_id, "name": terminal.name}))
}
