//! `poimen mcp` (ADR 0012, #613): a stdio MCP server with read-only tools, so an agent can ask
//! poimen about its environment. JSON-RPC 2.0, one message per line; it ends when the input
//! closes. It is a client of `poimen-server` through `poimen-client` and reads the server on
//! every call; it keeps nothing but the caller's terminal id. Failures of a tool are tool errors
//! (`isError`), never a crash of the process.
//!
//! The write tools `start_agent` and `prompt_agent` (#615) act freely in the space of the
//! calling terminal. For any other space the server asks the connected app (`agent.confirm`) and
//! the call waits for the user's answer; declined, timed out, no app and no calling terminal all
//! refuse with the reason. The target space is the id the server holds for the target, never a
//! name the agent supplies.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use poimen_client::Client;
use poimen_client::agent::{AgentError, PromptResult};
use poimen_client::protocol::{AgentConfirm, AgentWriteAction, AgentWriteNotice, TerminalCreate};
use poimen_core::agent_events::{self, MAX_EVENTS_BYTES, Reducer};
use poimen_core::{git, harness, settings};
use serde_json::{Value, json};

/// The MCP revisions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// How long a write into another space waits for the user.
pub const CONFIRM_WAIT: Duration = Duration::from_secs(60);

/// How long `start_agent` waits for the agent's first report.
const START_WAIT: Duration = Duration::from_secs(30);

/// What the process knows about its caller and its machine.
pub struct Env<'a> {
    /// `POIMEN_TERMINAL_ID` of the caller; `None` outside a poimen terminal.
    pub terminal_id: Option<&'a str>,
    /// The home directory whose `config.json` lists the `harnesses`; the defaults without.
    pub home: Option<PathBuf>,
    /// How long a confirmation in the app may take.
    pub confirm_wait: Duration,
}

/// Runs the server until `input` closes, with the default harnesses. `terminal_id` is
/// `POIMEN_TERMINAL_ID` of the caller.
pub fn serve(
    client: &Client,
    terminal_id: Option<&str>,
    input: impl BufRead,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    let env = Env {
        terminal_id,
        home: None,
        confirm_wait: CONFIRM_WAIT,
    };
    serve_with(client, &env, input, out)
}

/// [`serve`] with an explicit environment.
pub fn serve_with(
    client: &Client,
    env: &Env,
    input: impl BufRead,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(client, env, &message),
            Err(e) => Some(error(Value::Null, -32700, &format!("parse error: {e}"))),
        };
        if let Some(reply) = reply {
            writeln!(out, "{reply}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// The answer to one message; `None` for a notification.
pub fn handle(client: &Client, env: &Env, message: &Value) -> Option<Value> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // A response or garbage: nothing to answer.
        return message
            .get("id")
            .is_none()
            .then(|| error(Value::Null, -32600, "invalid request"));
    };
    let id = message.get("id").cloned()?;
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    Some(match method {
        "initialize" => {
            let wanted = params.get("protocolVersion").and_then(Value::as_str);
            let version = PROTOCOL_VERSIONS
                .iter()
                .find(|v| Some(**v) == wanted)
                .unwrap_or(&PROTOCOL_VERSIONS[0]);
            result(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {}},
                    "serverInfo": {"name": "poimen", "version": env!("CARGO_PKG_VERSION")},
                }),
            )
        }
        "ping" => result(id, json!({})),
        "tools/list" => result(id, json!({"tools": tools()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            match call(client, env, name, &args) {
                Ok(v) => result(id, tool_ok(&v)),
                Err(Fail::Unknown) => error(id, -32602, &format!("unknown tool `{name}`")),
                Err(Fail::Tool(m)) => result(id, tool_error(&m)),
            }
        }
        other => error(id, -32601, &format!("method `{other}` not found")),
    })
}

fn result(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn tool_ok(v: &Value) -> Value {
    json!({
        "content": [{"type": "text", "text": serde_json::to_string_pretty(v).unwrap_or_default()}],
        "structuredContent": v,
    })
}

fn tool_error(message: &str) -> Value {
    json!({"content": [{"type": "text", "text": message}], "isError": true})
}

fn tools() -> Value {
    let none = json!({"type": "object", "properties": {}});
    json!([
        {
            "name": "list_spaces",
            "description": "The poimen spaces: id, name, directory and tab ids.",
            "inputSchema": none,
        },
        {
            "name": "list_terminals",
            "description": "The poimen terminals: id, space, name, state, agent kind and status, tokens. Optionally of one space.",
            "inputSchema": {"type": "object", "properties": {
                "space": {"type": "string", "description": "Space id; all spaces when absent."}
            }},
        },
        {
            "name": "whoami",
            "description": "The terminal and space this agent runs in (from POIMEN_TERMINAL_ID); empty outside a poimen terminal.",
            "inputSchema": none,
        },
        {
            "name": "list_runs",
            "description": "The deliver runs (agent-events) of the repository containing a directory.",
            "inputSchema": {"type": "object", "properties": {
                "repository": {"type": "string", "description": "A directory inside the repository."}
            }, "required": ["repository"]},
        },
        {
            "name": "start_agent",
            "description": "Starts an agent in a new terminal of a space. In your own space it just runs; in any other space the user must confirm in the poimen app (waits up to 60 s, refused when declined, not answered, or poimen is not open).",
            "inputSchema": {"type": "object", "properties": {
                "space": {"type": "string", "description": "Space id; your own space when absent."},
                "harness": {"type": "string", "description": "A harness id of the user's config, e.g. claude or copilot."},
                "model": {"type": "string", "description": "A model of the harness."},
                "prompt": {"type": "string", "description": "The first prompt of the agent."},
                "name": {"type": "string", "description": "The name of the new terminal; the harness id by default."}
            }, "required": ["harness"]},
        },
        {
            "name": "prompt_agent",
            "description": "Types a prompt into the agent of a terminal. Refused while that agent is working or blocked. In another space than your own the user must confirm in the poimen app (waits up to 60 s).",
            "inputSchema": {"type": "object", "properties": {
                "terminal": {"type": "string", "description": "Terminal id."},
                "text": {"type": "string", "description": "The prompt."}
            }, "required": ["terminal", "text"]},
        },
    ])
}

enum Fail {
    Unknown,
    Tool(String),
}

impl<E: std::fmt::Display> From<E> for Fail {
    fn from(e: E) -> Self {
        Fail::Tool(e.to_string())
    }
}

fn call(client: &Client, env: &Env, name: &str, args: &Value) -> Result<Value, Fail> {
    let terminal_id = env.terminal_id;
    match name {
        "list_spaces" => {
            let spaces = client.list_spaces().map_err(server_error)?;
            let list: Vec<Value> = spaces
                .iter()
                .map(|s| json!({"id": s.id, "name": s.name, "directory": s.cwd, "tabs": s.tabs}))
                .collect();
            Ok(json!({"spaces": list}))
        }
        "list_terminals" => {
            let space = args.get("space").and_then(Value::as_str);
            let terminals = client.list_terminals(space).map_err(server_error)?;
            Ok(json!({"terminals": terminals.iter().map(terminal_json).collect::<Vec<_>>()}))
        }
        "whoami" => {
            let Some(id) = terminal_id.filter(|i| !i.is_empty()) else {
                return Ok(json!({"in_poimen": false}));
            };
            let terminals = client.list_terminals(None).map_err(server_error)?;
            let t = terminals
                .iter()
                .find(|t| t.id == id)
                .ok_or_else(|| Fail::Tool(format!("terminal {id} is not known to the server")))?;
            let spaces = client.list_spaces().map_err(server_error)?;
            let space = spaces.iter().find(|s| s.id == t.space_id);
            Ok(json!({
                "in_poimen": true,
                "terminal": terminal_json(t),
                "space": space.map(|s| json!({"id": s.id, "name": s.name, "directory": s.cwd})),
            }))
        }
        "list_runs" => {
            let dir = args
                .get("repository")
                .and_then(Value::as_str)
                .filter(|d| !d.is_empty())
                .ok_or_else(|| Fail::Tool("missing argument `repository`".into()))?;
            let runs = runs_of(dir).map_err(Fail::Tool)?;
            Ok(json!({"repository": dir, "runs": runs}))
        }
        "start_agent" => start_agent(client, env, args),
        "prompt_agent" => prompt_agent(client, env, args),
        _ => Err(Fail::Unknown),
    }
}

fn text_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
}

/// Allows a write into `space_id`: freely in the caller's own space, else only with the user's yes
/// in the app. Returns whether the user confirmed.
fn authorize(
    client: &Client,
    env: &Env,
    space_id: &str,
    action: AgentWriteAction,
    terminal_id: Option<&str>,
    text: &str,
) -> Result<bool, Fail> {
    let Some(caller) = env.terminal_id.filter(|i| !i.is_empty()) else {
        return Err(Fail::Tool(
            "refused: this agent does not run in a poimen terminal (POIMEN_TERMINAL_ID is not set), \
             so there is no space of its own to write into"
                .into(),
        ));
    };
    let terminals = client.list_terminals(None).map_err(server_error)?;
    let own = terminals
        .iter()
        .find(|t| t.id == caller)
        .ok_or_else(|| Fail::Tool(format!("terminal {caller} is not known to the server")))?;
    if own.space_id == space_id {
        return Ok(false);
    }
    let asked = AgentConfirm {
        caller_terminal_id: caller.to_owned(),
        action,
        space_id: space_id.to_owned(),
        terminal_id: terminal_id.map(str::to_owned),
        text: text.to_owned(),
        wait_ms: None,
    };
    let answer = client
        .confirm_agent_write(&asked, env.confirm_wait)
        .map_err(server_error)?;
    use poimen_client::protocol::ConfirmReason as Why;
    match answer.reason {
        Why::Approved if answer.approved => Ok(true),
        Why::Declined => Err(Fail::Tool(
            "refused: the user declined the write into another space".into(),
        )),
        Why::Timeout => Err(Fail::Tool(
            "refused: the user did not answer in time (the write into another space was not made)"
                .into(),
        )),
        Why::NoApp => Err(Fail::Tool(
            "refused: poimen is not open, so nobody can confirm a write into another space".into(),
        )),
        Why::Approved => Err(Fail::Tool("refused: the server did not approve".into())),
    }
}

/// Tells the apps about a write that happened; a failure does not undo the write.
fn announce(
    client: &Client,
    env: &Env,
    space_id: &str,
    action: AgentWriteAction,
    terminal_id: Option<&str>,
    text: &str,
    confirmed: bool,
) {
    let Some(caller) = env.terminal_id.filter(|i| !i.is_empty()) else {
        return;
    };
    let _ = client.notice_agent_write(&AgentWriteNotice {
        caller_terminal_id: caller.to_owned(),
        action,
        space_id: space_id.to_owned(),
        terminal_id: terminal_id.map(str::to_owned),
        text: text.to_owned(),
        confirmed,
    });
}

fn harnesses_of(env: &Env) -> Vec<harness::Harness> {
    let config = env
        .home
        .as_deref()
        .and_then(|h| std::fs::read_to_string(settings::config_path(h)).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok());
    match config.as_ref().and_then(|c| c.get("harnesses")) {
        Some(v) => harness::resolve(v, &mut Vec::new()),
        None => harness::default_harnesses(),
    }
}

fn agent_error(e: AgentError) -> Fail {
    Fail::Tool(e.to_string())
}

/// `start_agent {space?, harness, model?, prompt?, name?}`: a new terminal in the space (the
/// caller's by default) running the harness.
fn start_agent(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    let id =
        text_arg(args, "harness").ok_or_else(|| Fail::Tool("missing argument `harness`".into()))?;
    let harnesses = harnesses_of(env);
    let found = harnesses.iter().find(|h| h.id == id).ok_or_else(|| {
        let known: Vec<_> = harnesses.iter().map(|h| h.id.as_str()).collect();
        Fail::Tool(format!(
            "unknown harness `{id}`; known: {}",
            known.join(", ")
        ))
    })?;
    let argv = harness::build_argv(found, text_arg(args, "model"), text_arg(args, "prompt"));
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| Fail::Tool(format!("harness `{id}` has no command")))?;
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
        AgentWriteAction::StartAgent,
        None,
        &command_line,
    )?;
    let terminal = client
        .create_terminal(&TerminalCreate {
            space_id: space_id.clone(),
            name: Some(text_arg(args, "name").unwrap_or(id).to_owned()),
            ..Default::default()
        })
        .map_err(server_error)?;
    match client.start_agent(&terminal.id, program, rest, START_WAIT) {
        Ok(agent) => {
            announce(
                client,
                env,
                &space_id,
                AgentWriteAction::StartAgent,
                Some(&terminal.id),
                &command_line,
                confirmed,
            );
            Ok(json!({
                "terminal": terminal.id,
                "space": space_id,
                "agent": agent.agent,
                "status": agent.status,
            }))
        }
        Err(e) => {
            // The agent never started: the empty terminal is not left behind.
            let _ = client.close_terminal(&terminal.id);
            Err(agent_error(e))
        }
    }
}

/// `prompt_agent {terminal, text}`: types the prompt into the agent of the terminal, refused while
/// that agent is working or blocked (the rules of `agent.prompt`).
fn prompt_agent(client: &Client, env: &Env, args: &Value) -> Result<Value, Fail> {
    let terminal = text_arg(args, "terminal")
        .ok_or_else(|| Fail::Tool("missing argument `terminal`".into()))?;
    let text =
        text_arg(args, "text").ok_or_else(|| Fail::Tool("missing argument `text`".into()))?;
    let terminals = client.list_terminals(None).map_err(server_error)?;
    let target = terminals
        .iter()
        .find(|t| t.id == terminal)
        .ok_or_else(|| Fail::Tool(format!("terminal {terminal} not found")))?;
    let space_id = target.space_id.clone();
    let confirmed = authorize(
        client,
        env,
        &space_id,
        AgentWriteAction::PromptAgent,
        Some(terminal),
        text,
    )?;
    match client
        .prompt_agent(terminal, text, crate::commands::PROMPT_WAIT)
        .map_err(agent_error)?
    {
        PromptResult::Started(status) => {
            announce(
                client,
                env,
                &space_id,
                AgentWriteAction::PromptAgent,
                Some(terminal),
                text,
                confirmed,
            );
            Ok(json!({"terminal": terminal, "sent": true, "status": status}))
        }
        PromptResult::Stalled => {
            announce(
                client,
                env,
                &space_id,
                AgentWriteAction::PromptAgent,
                Some(terminal),
                text,
                confirmed,
            );
            Ok(json!({"terminal": terminal, "sent": true, "status": null}))
        }
        PromptResult::Working => Err(Fail::Tool(
            "refused: the agent is working; prompt it when it is idle".into(),
        )),
        PromptResult::Blocked => Err(Fail::Tool(
            "refused: the agent is blocked at a dialog; the user must answer it".into(),
        )),
        PromptResult::NoAgent => Err(Fail::Tool(
            "refused: no agent reported in that terminal".into(),
        )),
    }
}

fn server_error(e: poimen_client::ClientError) -> Fail {
    Fail::Tool(format!("poimen-server: {e}"))
}

fn terminal_json(t: &poimen_client::protocol::Terminal) -> Value {
    let agent = t.agent.as_ref().map(|a| {
        json!({
            "kind": a.agent,
            "status": a.status,
            "message": a.message,
        })
    });
    json!({
        "id": t.id,
        "space": t.space_id,
        "name": t.name,
        "directory": t.cwd,
        "state": t.status,
        "agent": agent,
        "tokens": t.tokens,
    })
}

/// The runs of the repository containing `dir`, from `<common-dir>/.agents/events/*.jsonl`
/// folded by the core reducer (the same one the app uses), oldest file name first. No events
/// directory is an empty list; a directory outside a repository is an error.
fn runs_of(dir: &str) -> Result<Vec<Value>, String> {
    if !Path::new(dir).is_dir() {
        return Err(format!("`{dir}` is not a directory"));
    }
    let (_, common) =
        git::roots(dir).ok_or_else(|| format!("`{dir}` is not inside a git repository"))?;
    let events = Path::new(&common).join(agent_events::EVENTS_PATH);
    let Ok(entries) = std::fs::read_dir(&events) else {
        return Ok(Vec::new());
    };
    let mut files: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| agent_events::is_run_file(p))
        .collect();
    files.sort();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64());
    let mut runs = Vec::new();
    for path in files {
        let regular =
            std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_EVENTS_BYTES);
        let Some(text) = regular
            .then(|| std::fs::read_to_string(&path).ok())
            .flatten()
        else {
            continue;
        };
        // Only complete lines: a half-written last line waits.
        let complete = text.rfind('\n').map_or("", |i| &text[..=i]);
        let mut reducer = Reducer::new();
        for line in complete.lines().filter(|l| !l.trim().is_empty()) {
            reducer.push_line(line);
        }
        if let Some(run) = reducer.snapshot(now, &agent_events::pid_alive) {
            let mut v = serde_json::to_value(&run).map_err(|e| e.to_string())?;
            if let Some(o) = v.as_object_mut() {
                o.remove("events");
            }
            runs.push(v);
        }
    }
    Ok(runs)
}
