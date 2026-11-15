//! `poimen mcp` (ADR 0012, #613): a stdio MCP server with read-only tools, so an agent can ask
//! poimen about its environment. JSON-RPC 2.0, one message per line; it ends when the input
//! closes. It is a client of `poimen-server` through `poimen-client` and reads the server on
//! every call; it keeps nothing but the caller's terminal id. Failures of a tool are tool errors
//! (`isError`), never a crash of the process.

use std::io::{BufRead, Write};
use std::path::Path;

use poimen_client::Client;
use poimen_core::agent_events::{self, MAX_EVENTS_BYTES, Reducer};
use poimen_core::git;
use serde_json::{Value, json};

/// The MCP revisions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// Runs the server until `input` closes. `terminal_id` is `POIMEN_TERMINAL_ID` of the caller.
pub fn serve(
    client: &Client,
    terminal_id: Option<&str>,
    input: impl BufRead,
    out: &mut dyn Write,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(message) => handle(client, terminal_id, &message),
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
pub fn handle(client: &Client, terminal_id: Option<&str>, message: &Value) -> Option<Value> {
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
            match call(client, terminal_id, name, &args) {
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

fn call(
    client: &Client,
    terminal_id: Option<&str>,
    name: &str,
    args: &Value,
) -> Result<Value, Fail> {
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
        _ => Err(Fail::Unknown),
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
