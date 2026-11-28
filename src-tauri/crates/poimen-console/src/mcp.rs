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

mod grant;
mod lead;
mod report;
mod run_files;
mod run_writer;
mod runs;
mod screen;
mod shell;
mod space;
mod stop;
mod terminal;
mod wait_any;
mod worktree;
mod write;

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use terminal::{terminal_detail, terminal_json};
use worktree::{WorktreeSpec, make_worktree_space};

use poimen_client::Client;
use poimen_client::agent::{self, AgentError, PromptResult, WriteAsk, WriteError, authorize_write};
use poimen_client::protocol::{
    AgentStatus, AgentWriteAction, AgentWriteNotice, TerminalCreate, TranscriptState,
};
use poimen_core::{agent_events, harness};
use serde_json::{Value, json};

/// The MCP revisions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

/// How long a write into another space waits for the user.
pub const CONFIRM_WAIT: Duration = agent::CONFIRM_WAIT;

/// How long `start_agent` waits for the agent's first report.
const START_WAIT: Duration = Duration::from_secs(30);

/// How long `wait_agent` waits when the caller gives no `timeout_s`, and the longest it waits.
const WAIT_DEFAULT_S: f64 = 300.0;
const WAIT_MAX_S: f64 = 3600.0;

/// What the process knows about its caller and its machine.
pub struct Env<'a> {
    /// `POIMEN_TERMINAL_ID` of the caller; `None` outside a poimen terminal.
    pub terminal_id: Option<&'a str>,
    /// The home directory whose `config.json` lists the `harnesses`; the defaults without.
    pub home: Option<PathBuf>,
    /// How long a confirmation in the app may take.
    pub confirm_wait: Duration,
    /// The shared event directory of the run tools (`AGENT_EVENTS_DIR`, else the per-user
    /// default: `poimen_core::agent_events::shared_dir`); `None` when there is none.
    pub events_dir: Option<PathBuf>,
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
        events_dir: agent_events::shared_dir_here(),
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
            "description": "The poimen terminals: id, space, name, state, agent kind and status, tokens. Optionally of one space. Compact: `get_terminal` has the source of a status, its times and its recent changes.",
            "inputSchema": {"type": "object", "properties": {
                "space": {"type": "string", "description": "Space id; all spaces when absent."}
            }},
        },
        {
            "name": "get_terminal",
            "description": "One terminal in full: what list_terminals has, plus where the agent status came from (`source`: `osc7501` or the hook kind), its times, title, progress, tone, hibernation, the OSC 7501 child records and the last status changes (at most 20, newest last; only since the server started). A read tool: any space, no confirmation.",
            "inputSchema": {"type": "object", "properties": {
                "terminal": {"type": "string", "description": "Terminal id."}
            }, "required": ["terminal"], "additionalProperties": false},
        },
        {
            "name": "read_terminal",
            "description": "The last lines of any terminal as plain text (its screen and scrollback: a shell, a build, a scripted run), with whether it still runs. Observes only: no input, no control. Cut to 64 KiB with a note. Terminal output can hold secrets; a read tool: any space, no confirmation (ADR 0012).",
            "inputSchema": {"type": "object", "properties": {
                "terminal": {"type": "string", "description": "Terminal id."},
                "lines": {"type": "integer", "minimum": 1, "maximum": 500, "description": "How many lines from the end (default 50, at most 500)."}
            }, "required": ["terminal"], "additionalProperties": false},
        },
        {
            "name": "whoami",
            "description": "The terminal and space this agent runs in (from POIMEN_TERMINAL_ID); empty outside a poimen terminal.",
            "inputSchema": none,
        },
        {
            "name": "list_runs",
            "description": "The runs (agent-events) of all repositories in the per-user event directory (AGENT_EVENTS_DIR when set), each with its `origin` (repo, branch, worktree, host), newest first, at most `limit` (default 20). `repository` (optional) keeps the runs of the repository containing that directory, `repo` (optional) those of `owner/repo` without a local checkout.",
            "inputSchema": {"type": "object", "properties": {
                "repository": {"type": "string", "description": "Optional: only the runs of the repository containing this directory, also those in its legacy `.agents/events`."},
                "repo": {"type": "string", "description": "Optional: only the runs whose origin is this repository (`owner/repo`)."},
                "status": {"type": "string", "enum": ["running", "finished", "needs-you"], "description": "Only runs with this status; `needs-you` is the runs with an attention that needs the user."},
                "limit": {"type": "integer", "minimum": 1, "description": "At most this many runs (default 20, newest first)."},
                "compact": {"type": "boolean", "description": "Leave out the `finished` and `links` lists of each run."}
            }, "additionalProperties": false},
        },
        {
            "name": "get_events",
            "description": "The raw agent-events of one run after a `seq`, in order. The run is found by its id among the runs of all repositories; `repository` is optional. Read-only.",
            "inputSchema": {"type": "object", "properties": {
                "repository": {"type": "string", "description": "Optional: a directory inside the run's repository, to also read its legacy `.agents/events`."},
                "run": {"type": "string", "description": "The run id (`run` of list_runs)."},
                "after": {"type": "integer", "minimum": 0, "description": "Only events with a greater seq (default: from the start)."},
                "type": {"type": "string", "description": "Only events of this type, e.g. `attention.raised`."},
                "limit": {"type": "integer", "minimum": 1, "description": "At most this many events (default 100, at most 1000)."}
            }, "required": ["run"], "additionalProperties": false},
        },
        {
            "name": "start_agent",
            "description": "Starts an agent in a new terminal of a space. In your own space it just runs; in any other space the user must confirm in the poimen app (waits up to 60 s, refused when declined, not answered, or poimen is not open).",
            "inputSchema": {"type": "object", "properties": {
                "space": {"type": "string", "description": "Space id; your own space when absent."},
                "harness": {"type": "string", "description": "A harness id of the user's config, e.g. claude or copilot."},
                "model": {"type": "string", "description": "A model of the harness."},
                "prompt": {"type": "string", "description": "The first prompt of the agent."},
                "name": {"type": "string", "description": "The name of the new terminal; the harness id by default."},
                "worktree": {"type": "object", "description": "Start in a new space on a task worktree of the repository of `space` (or your own space) instead: {issue: number, title?, mode?} (the task mode, `agent` by default) or {branch: name}. An existing worktree of that branch is reused, never changed. A new space always needs the user's confirmation in the poimen app. The answer names the space, the worktree path and the branch.",
                    "properties": {
                        "issue": {"type": "integer"},
                        "title": {"type": "string"},
                        "mode": {"type": "string"},
                        "branch": {"type": "string"}
                    }}
            }, "required": ["harness"]},
        },
        {
            "name": "start_shell",
            "description": "Runs a long-running command in a new shell pane of the user, e.g. a scripted run (`node run.mts`) or a build watcher. Not for short commands: use your own shell for those. `command` is an argv list, never a shell string. Returns the terminal id; the pane stays after the command ends. Watch it with read_terminal (output), get_terminal (status, exit code), get_events and list_runs (agent runs it writes); wait for it with wait_any; end it with stop_terminal. For another repository first find its directory with list_repositories and make a space with create_space, and ask once for request_grant when you start several runs. In your own space it just runs; in any other space the user must confirm in the poimen app (waits up to 60 s).",
            "inputSchema": {"type": "object", "properties": {
                "command": {"type": "array", "items": {"type": "string"}, "minItems": 1, "description": "Program and arguments, e.g. [\"node\", \"run.mts\"]."},
                "cwd": {"type": "string", "description": "Working directory; the directory of the space by default."},
                "name": {"type": "string", "description": "The name of the new terminal; the program by default."},
                "space": {"type": "string", "description": "Space id; your own space when absent."},
                "env": {"type": "object", "additionalProperties": {"type": "string"}, "description": "Extra environment variables."}
            }, "required": ["command"], "additionalProperties": false},
        },
        stop::tool(),
        write::tool(),
        space::create_tool(),
        space::list_tool(),
        grant::request_tool(),
        grant::revoke_tool(),
        lead::record_tool(),
        lead::read_tool(),
        {
            "name": "prompt_agent",
            "description": "Types a prompt into the agent of a terminal. Refused while that agent is working or blocked. In another space than your own the user must confirm in the poimen app (waits up to 60 s).",
            "inputSchema": {"type": "object", "properties": {
                "terminal": {"type": "string", "description": "Terminal id."},
                "text": {"type": "string", "description": "The prompt."}
            }, "required": ["terminal", "text"]},
        },
        {
            "name": "wait_agent",
            "description": "Waits until the agent of a terminal is done, blocked, in error or gone, or the timeout passed. Returns the status, when blocked the notification text, and the agent's `report_done` report when there is one. A read tool: any space, no confirmation. Uses the server's events, no polling.",
            "inputSchema": {"type": "object", "properties": {
                "terminal": {"type": "string", "description": "Terminal id."},
                "timeout_s": {"type": "number", "description": "Seconds to wait (default 300, at most 3600). On timeout the current status is returned with timed_out true."}
            }, "required": ["terminal"]},
        },
        wait_any::tool(),
        {
            "name": "report_done",
            "description": "Call once at the very end of your work when another agent started you: reports the result to the agent that started you (poimen types a line into it when it is idle) and closes your run in the Runs view. Report once, with a summary of at most three sentences.",
            "inputSchema": {"type": "object", "properties": {
                "outcome": {"type": "string", "enum": ["succeeded", "failed", "cancelled"]},
                "summary": {"type": "string", "description": "What came out, at most three sentences."},
                "files": {"type": "array", "items": {"type": "string"}, "description": "The files you changed."}
            }, "required": ["outcome", "summary"]},
        },
        {
            "name": "read_agent",
            "description": "The last assistant message of the agent's transcript in a terminal (Claude Code and Copilot CLI), with the session id. An error when the terminal has no agent transcript. A read tool: any space, no confirmation.",
            "inputSchema": {"type": "object", "properties": {
                "terminal": {"type": "string", "description": "Terminal id."}
            }, "required": ["terminal"]},
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
        "get_terminal" => {
            runs::only_keys(args, &["terminal"])?;
            let id = args
                .get("terminal")
                .and_then(Value::as_str)
                .filter(|i| !i.is_empty())
                .ok_or_else(|| Fail::Tool("terminal is required".into()))?;
            let terminals = client.list_terminals(None).map_err(server_error)?;
            let t = terminals
                .iter()
                .find(|t| t.id == id)
                .ok_or_else(|| Fail::Tool(format!("terminal {id} is not known to the server")))?;
            Ok(terminal_detail(t))
        }
        "read_terminal" => screen::read_terminal(client, args),
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
        "list_runs" => runs::list_runs(env.events_dir.as_deref(), args),
        "get_events" => runs::get_events(env.events_dir.as_deref(), args),
        "start_agent" => start_agent(client, env, args),
        "start_shell" => shell::start_shell(client, env, args),
        "create_space" => space::create_space(client, env, args),
        "list_repositories" => space::list_repositories(client, env, args),
        "request_grant" => grant::request_grant(client, env, args),
        "revoke_grant" => grant::revoke_grant(client, env, args),
        "record_run" => lead::record_run(client, env, args),
        "read_run" => lead::read_run(client, env, args),
        "stop_terminal" => stop::stop_terminal(client, env, args),
        "write_terminal" => write::write_terminal(client, env, args),
        "prompt_agent" => prompt_agent(client, env, args),
        "wait_agent" => wait_agent(client, args).map(|v| report::with_report(client, v)),
        "wait_any" => wait_any::wait_any(client, env, args),
        "report_done" => report::report_done(client, env, args),
        "read_agent" => read_agent(client, args),
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
    always_ask: bool,
) -> Result<bool, Fail> {
    authorize_write(
        client,
        env.terminal_id,
        &WriteAsk {
            space_id,
            action,
            terminal_id,
            text,
            always_ask,
        },
        env.confirm_wait,
    )
    .map_err(|e| match e {
        WriteError::Refused(m) => Fail::Tool(m),
        WriteError::Client(e) => server_error(e),
    })
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
    agent::harnesses_of(env.home.as_deref())
}

fn agent_error(e: AgentError) -> Fail {
    Fail::Tool(e.to_string())
}

/// `start_agent {space?, harness, model?, prompt?, name?, worktree?}`: a new terminal in the space (the
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
    let prompt = text_arg(args, "prompt").map(report::with_preamble);
    let argv = harness::build_argv(found, text_arg(args, "model"), prompt.as_deref());
    let (program, rest) = argv
        .split_first()
        .ok_or_else(|| Fail::Tool(format!("harness `{id}` has no command")))?;
    let worktree = WorktreeSpec::parse(args.get("worktree"))?;
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
    let confirm_text = match &worktree {
        Some(spec) => format!("{command_line} (in a new worktree: {})", spec.describe()),
        None => command_line.clone(),
    };
    let confirmed = authorize(
        client,
        env,
        &space_id,
        AgentWriteAction::StartAgent,
        None,
        &confirm_text,
        worktree.is_some(),
    )?;
    let (space_id, new_space) = match &worktree {
        Some(spec) => {
            let source = spaces
                .iter()
                .find(|s| s.id == space_id)
                .ok_or_else(|| Fail::Tool(format!("space {space_id} not found")))?;
            let made = make_worktree_space(client, env, &source.cwd, spec)?;
            (made.space.id.clone(), Some(made))
        }
        None => (space_id, None),
    };
    // A space made for this start does not outlive a failed start (the worktree stays).
    let drop_new_space = || {
        if new_space.is_some() {
            let _ = client.close_space(&space_id);
        }
    };
    let terminal = client
        .create_terminal(&TerminalCreate {
            space_id: space_id.clone(),
            name: Some(text_arg(args, "name").unwrap_or(id).to_owned()),
            started_by: env.terminal_id.map(str::to_owned),
            ..Default::default()
        })
        .map_err(|e| {
            drop_new_space();
            server_error(e)
        })?;
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
            let mut answer = json!({
                "terminal": terminal.id,
                "space": space_id,
                "agent": agent.agent,
                "status": agent.status,
            });
            if let Some(made) = &new_space {
                answer["worktree"] = json!({
                    "path": made.path,
                    "branch": made.branch,
                    "created": made.created,
                });
            }
            Ok(answer)
        }
        Err(e) => {
            // The agent never started: the empty terminal is not left behind.
            let _ = client.close_terminal(&terminal.id);
            drop_new_space();
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
        false,
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

/// The agent status a wait ends on: the agent finished a turn, needs the user, failed or ended.
fn is_final(status: AgentStatus) -> bool {
    matches!(
        status,
        AgentStatus::Done | AgentStatus::Blocked | AgentStatus::Error | AgentStatus::Gone
    )
}

fn wait_result(terminal: &str, status: &str, message: Option<&str>, timed_out: bool) -> Value {
    let mut v = json!({"terminal": terminal, "status": status, "timed_out": timed_out});
    if let (Some(m), Some(o)) = (message, v.as_object_mut()) {
        o.insert("message".into(), json!(m));
    }
    v
}

/// `wait_agent {terminal, timeout_s?}`: returns when the agent is done, blocked, in error or gone
/// (also: the terminal is closed or its process ended), or at the timeout. It subscribes to the
/// server's events before it reads the status, so no change is missed, and never polls.
fn wait_agent(client: &Client, args: &Value) -> Result<Value, Fail> {
    let terminal = text_arg(args, "terminal")
        .ok_or_else(|| Fail::Tool("missing argument `terminal`".into()))?;
    let seconds = args
        .get("timeout_s")
        .and_then(Value::as_f64)
        .filter(|s| s.is_finite() && *s >= 0.0)
        .unwrap_or(WAIT_DEFAULT_S)
        .min(WAIT_MAX_S);
    let deadline = Instant::now() + Duration::from_secs_f64(seconds);
    let watched = agent::watch_agent(client, terminal, Some(deadline), is_final, |_| {})
        .map_err(server_error)?
        .ok_or_else(|| Fail::Tool(format!("terminal {terminal} not found")))?;
    let status = serde_json::to_value(watched.status).unwrap_or_default();
    Ok(wait_result(
        terminal,
        status.as_str().unwrap_or("idle"),
        watched.message.as_deref(),
        watched.timed_out,
    ))
}

/// `read_agent {terminal}`: the last assistant message of the agent's transcript, read by the
/// server of the terminal (`agent.transcript`), so it works for a remote machine too.
fn read_agent(client: &Client, args: &Value) -> Result<Value, Fail> {
    let terminal = text_arg(args, "terminal")
        .ok_or_else(|| Fail::Tool("missing argument `terminal`".into()))?;
    let known = client.list_terminals(None).map_err(server_error)?;
    if !known.iter().any(|t| t.id == terminal) {
        return Err(Fail::Tool(format!("terminal {terminal} not found")));
    }
    // One reader per terminal and process; `restart` makes every answer start at the beginning.
    let follower = format!("mcp-{}", std::process::id());
    let answer = client
        .agent_transcript(terminal, &follower, true)
        .map_err(server_error)?;
    match answer.state {
        TranscriptState::Ok => {
            let last = answer.updates.iter().rev().find_map(|u| {
                (u.get("kind").and_then(Value::as_str) == Some("main"))
                    .then(|| u.get("last_message").and_then(Value::as_str))
                    .flatten()
            });
            match last {
                Some(message) => Ok(json!({
                    "terminal": terminal,
                    "session": answer.session,
                    "message": message,
                })),
                None => Err(Fail::Tool(
                    "the agent's transcript has no assistant message yet".into(),
                )),
            }
        }
        TranscriptState::NoAgent => Err(Fail::Tool(format!(
            "terminal {terminal} has no agent with a readable transcript (Claude Code or Copilot CLI with poimen hooks reporting a session)"
        ))),
        TranscriptState::Missing => Err(Fail::Tool(
            "the agent's transcript file is not there yet; try again later".into(),
        )),
        TranscriptState::Unreadable => Err(Fail::Tool(format!(
            "the agent's transcript cannot be read: {}",
            answer.reason.unwrap_or_default()
        ))),
    }
}

fn server_error(e: poimen_client::ClientError) -> Fail {
    Fail::Tool(format!("poimen-server: {e}"))
}
