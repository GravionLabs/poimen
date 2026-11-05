//! Starting an agent in a terminal and sending it a prompt (#156). The server runs no agent
//! logic: it types into the PTY and learns the agent's state from its hook reports
//! (`terminal.report_agent`), so both calls here are typed input plus a wait on the terminal's
//! `agent` field.

use std::fmt;
use std::time::{Duration, Instant};

use poimen_protocol::{AgentInfo, AgentStatus, Terminal, TerminalStatus};

use crate::{Client, ClientError};

/// How long `start_agent` waits for the `SessionStart` report of the agent.
pub const START_TIMEOUT: Duration = Duration::from_secs(30);
/// How long `prompt_agent` waits for the agent to react to a submitted prompt.
pub const PROMPT_WAIT: Duration = Duration::from_secs(10);

const POLL: Duration = Duration::from_millis(50);

#[derive(Debug)]
pub enum AgentError {
    Client(ClientError),
    /// The terminal does not exist.
    NotFound(String),
    /// The terminal already has an agent, so nothing was typed.
    AlreadyRunning,
    /// The terminal's process ended before the agent reported.
    Exited,
    /// No `SessionStart` report within the time.
    Timeout(Duration),
}

impl fmt::Display for AgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AgentError::Client(e) => write!(f, "{e}"),
            AgentError::NotFound(id) => write!(f, "terminal {id} not found"),
            AgentError::AlreadyRunning => write!(f, "the terminal already has an agent"),
            AgentError::Exited => write!(f, "the terminal ended before the agent started"),
            AgentError::Timeout(limit) => write!(
                f,
                "the agent did not report within {} s; are the poimen hooks installed \
                 (`poimen-server integration install claude|copilot`)?",
                limit.as_secs()
            ),
        }
    }
}

impl std::error::Error for AgentError {}

impl From<ClientError> for AgentError {
    fn from(e: ClientError) -> Self {
        AgentError::Client(e)
    }
}

/// What happened to a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptResult {
    /// Submitted; the agent then reported this status (`Working` or `Blocked`).
    Started(AgentStatus),
    /// Submitted, but the agent reported no activity in time.
    Stalled,
    /// Nothing was sent: the agent is blocked at a dialog.
    Blocked,
    /// Nothing was sent: the agent is working.
    Working,
    /// Nothing was sent: no agent reported in this terminal.
    NoAgent,
}

/// The command line that starts `program` with `args` in a shell: every word single-quoted when
/// it holds anything but plain characters, so a prompt with spaces stays one argument. Empty
/// arguments are dropped.
pub fn command_line(program: &str, args: &[String]) -> String {
    std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(str::trim)
        .filter(|w| !w.is_empty())
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ")
}

fn quote(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if word.chars().all(plain) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

impl Client {
    fn find_terminal(&self, terminal_id: &str) -> Result<Terminal, AgentError> {
        self.list_terminals(None)?
            .into_iter()
            .find(|t| t.id == terminal_id)
            .ok_or_else(|| AgentError::NotFound(terminal_id.to_owned()))
    }

    /// Runs `program args` in the terminal (which sits at a shell prompt) and returns once the
    /// agent's `SessionStart` report arrives, or fails after `timeout`. A terminal that already
    /// has an agent is refused before anything is typed.
    pub fn start_agent(
        &self,
        terminal_id: &str,
        program: &str,
        args: &[String],
        timeout: Duration,
    ) -> Result<AgentInfo, AgentError> {
        if self.find_terminal(terminal_id)?.agent.is_some() {
            return Err(AgentError::AlreadyRunning);
        }
        self.input(terminal_id, &format!("{}\r", command_line(program, args)))?;
        let deadline = Instant::now() + timeout;
        loop {
            let terminal = self.find_terminal(terminal_id)?;
            if let Some(agent) = terminal.agent {
                return Ok(agent);
            }
            if terminal.status == TerminalStatus::Exited {
                return Err(AgentError::Exited);
            }
            if Instant::now() >= deadline {
                return Err(AgentError::Timeout(timeout));
            }
            std::thread::sleep(POLL);
        }
    }

    /// Types `text` and Enter into the terminal of the agent, unless the agent is working or
    /// blocked, then waits up to `wait` for it to report `Working` or `Blocked`.
    pub fn prompt_agent(
        &self,
        terminal_id: &str,
        text: &str,
        wait: Duration,
    ) -> Result<PromptResult, AgentError> {
        match self.find_terminal(terminal_id)?.agent.map(|a| a.status) {
            None | Some(AgentStatus::Gone) => return Ok(PromptResult::NoAgent),
            Some(AgentStatus::Working) => return Ok(PromptResult::Working),
            Some(AgentStatus::Blocked) => return Ok(PromptResult::Blocked),
            Some(AgentStatus::Idle | AgentStatus::Done) => {}
        }
        self.input(terminal_id, &format!("{text}\r"))?;
        let deadline = Instant::now() + wait;
        loop {
            match self.find_terminal(terminal_id)?.agent.map(|a| a.status) {
                Some(status @ (AgentStatus::Working | AgentStatus::Blocked)) => {
                    return Ok(PromptResult::Started(status));
                }
                _ if Instant::now() >= deadline => return Ok(PromptResult::Stalled),
                _ => std::thread::sleep(POLL),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn plain_words_stay_bare_and_others_are_single_quoted() {
        assert_eq!(
            command_line("claude", &words(&["--model", "opus-4.5"])),
            "claude --model opus-4.5"
        );
        assert_eq!(
            command_line("claude", &words(&["fix it's \"bug\" $(x)\nnow"])),
            "claude 'fix it'\\''s \"bug\" $(x)\nnow'"
        );
    }

    #[test]
    fn empty_arguments_are_dropped() {
        assert_eq!(command_line(" codex ", &words(&["", "  "])), "codex");
    }
}
