//! The single-request commands of `poimen` (`ls`, `status`, `new`, `send`, `prompt`) over
//! `poimen-client`. Output goes to any `Write`; an error carries the exit code.

use std::io::Write;
use std::time::Duration;

use poimen_client::agent::{AgentError, PromptResult};
use poimen_client::protocol::{AgentStatus, Space, Terminal, TerminalCreate, TerminalStatus};
use poimen_client::{Client, ClientError};
use serde_json::{Value, json};

use crate::cli::Request;

/// How long `prompt` waits for the agent to report `working` or `blocked`.
pub const PROMPT_WAIT: Duration = Duration::from_secs(10);

/// A failed command: the message for stderr and the exit code (1; 2 is for usage errors).
#[derive(Debug)]
pub struct Failure(pub String);

impl From<ClientError> for Failure {
    fn from(e: ClientError) -> Self {
        Failure(e.to_string())
    }
}

impl From<AgentError> for Failure {
    fn from(e: AgentError) -> Self {
        Failure(e.to_string())
    }
}

impl From<std::io::Error> for Failure {
    fn from(e: std::io::Error) -> Self {
        Failure(e.to_string())
    }
}

/// Runs `request` against the server; `wait` is the `prompt` wait.
pub fn run(
    client: &Client,
    request: &Request,
    wait: Duration,
    out: &mut dyn Write,
) -> Result<(), Failure> {
    match request {
        Request::Ls { json } => ls(client, *json, out),
        Request::Status { terminal, json } => status(client, terminal, *json, out),
        Request::New {
            space,
            name,
            command,
        } => {
            let space = find_space(client, space)?;
            let t = client.create_terminal(&TerminalCreate {
                space_id: space.id,
                name: name.clone(),
                command: command.clone(),
                ..Default::default()
            })?;
            writeln!(out, "{}", t.id)?;
            Ok(())
        }
        Request::Send {
            terminal,
            text,
            enter,
        } => {
            let data = if *enter {
                format!("{text}\r")
            } else {
                text.clone()
            };
            client.input(terminal, &data)?;
            Ok(())
        }
        Request::Prompt { terminal, text } => match client.prompt_agent(terminal, text, wait)? {
            PromptResult::Started(status) => {
                writeln!(out, "started: agent is {}", status_word(status))?;
                Ok(())
            }
            PromptResult::Stalled => {
                writeln!(out, "sent, but the agent did not report any activity yet")?;
                Ok(())
            }
            PromptResult::Working => Err(Failure("refused: the agent is working".into())),
            PromptResult::Blocked => Err(Failure(
                "refused: the agent is blocked, waiting for an answer".into(),
            )),
            PromptResult::NoAgent => Err(Failure(format!(
                "refused: no agent has reported in terminal {terminal}"
            ))),
        },
    }
}

/// A space by id, else by exact name (an ambiguous name is an error).
fn find_space(client: &Client, key: &str) -> Result<Space, Failure> {
    let spaces = client.list_spaces()?;
    if let Some(s) = spaces.iter().find(|s| s.id == key) {
        return Ok(s.clone());
    }
    let mut named = spaces.into_iter().filter(|s| s.name == key);
    match (named.next(), named.next()) {
        (Some(s), None) => Ok(s),
        (None, _) => Err(Failure(format!("space `{key}` not found"))),
        (Some(_), Some(_)) => Err(Failure(format!(
            "several spaces are named `{key}`; use the id"
        ))),
    }
}

fn status_word(s: AgentStatus) -> &'static str {
    match s {
        AgentStatus::Idle => "idle",
        AgentStatus::Working => "working",
        AgentStatus::Blocked => "blocked",
        AgentStatus::Done => "done",
        AgentStatus::Gone => "gone",
    }
}

fn state_word(t: &Terminal) -> String {
    match (t.status, t.exit_code) {
        (TerminalStatus::Running, _) => "running".into(),
        (TerminalStatus::Exited, Some(c)) => format!("exited({c})"),
        (TerminalStatus::Exited, None) => "exited".into(),
    }
}

fn agent_word(t: &Terminal) -> String {
    match &t.agent {
        None => "-".into(),
        Some(a) => format!(
            "{}:{}",
            a.agent.as_deref().unwrap_or("agent"),
            status_word(a.status)
        ),
    }
}

fn ls(client: &Client, json: bool, out: &mut dyn Write) -> Result<(), Failure> {
    let spaces = client.list_spaces()?;
    let terminals = client.list_terminals(None)?;
    if json {
        let list: Vec<Value> = spaces
            .iter()
            .map(|s| {
                let mut v = serde_json::to_value(s).expect("space serializes");
                let own: Vec<&Terminal> = terminals.iter().filter(|t| t.space_id == s.id).collect();
                v["terminals"] = serde_json::to_value(own).expect("terminal serializes");
                v
            })
            .collect();
        writeln!(out, "{}", json!(list))?;
        return Ok(());
    }
    for s in &spaces {
        writeln!(out, "{}  {}  {}", s.id, s.name, s.cwd)?;
        for t in terminals.iter().filter(|t| t.space_id == s.id) {
            writeln!(
                out,
                "  {}  {}  {}  {}",
                t.id,
                t.name,
                state_word(t),
                agent_word(t)
            )?;
        }
    }
    Ok(())
}

fn status(client: &Client, id: &str, json: bool, out: &mut dyn Write) -> Result<(), Failure> {
    let t = client
        .list_terminals(None)?
        .into_iter()
        .find(|t| t.id == id)
        .ok_or_else(|| Failure(format!("terminal {id} not found")))?;
    if json {
        writeln!(
            out,
            "{}",
            serde_json::to_value(&t).expect("terminal serializes")
        )?;
    } else {
        writeln!(
            out,
            "{}  {}  {}  {}",
            t.id,
            t.name,
            state_word(&t),
            agent_word(&t)
        )?;
        if let Some(m) = t.agent.as_ref().and_then(|a| a.message.as_deref()) {
            writeln!(out, "message: {m}")?;
        }
    }
    Ok(())
}
