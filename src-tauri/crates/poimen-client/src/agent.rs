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
    /// The terminal runs cmd.exe, whose quoting cannot carry arbitrary arguments.
    UnsupportedShell,
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
            AgentError::UnsupportedShell => write!(
                f,
                "cannot start an agent in cmd.exe: it cannot quote arguments safely; \
                 install PowerShell (pwsh) so terminals use it"
            ),
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

/// The kind of shell a terminal runs, which decides how words are quoted for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    /// sh, bash, zsh, fish and the like: single quotes, an apostrophe as `'\''`.
    Posix,
    /// pwsh and powershell: single quotes, an apostrophe doubled.
    PowerShell,
    /// cmd.exe: no usable quoting, so no command line is built.
    Cmd,
}

impl ShellKind {
    /// The kind of the program at `path` (either path separator, `.exe` ignored, any case). Any
    /// program that is not PowerShell or cmd is taken as POSIX.
    pub fn of(path: &str) -> Self {
        let name = path
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(path)
            .to_ascii_lowercase();
        match name.strip_suffix(".exe").unwrap_or(&name) {
            "pwsh" | "powershell" => ShellKind::PowerShell,
            "cmd" => ShellKind::Cmd,
            _ => ShellKind::Posix,
        }
    }
}

/// The command line that starts `program` with `args` in a shell of `kind`: every word
/// single-quoted when it holds anything but plain characters, so a prompt with spaces stays one
/// argument. Empty arguments are dropped. `None` for cmd.exe, which has no such quoting.
pub fn command_line(program: &str, args: &[String], kind: ShellKind) -> Option<String> {
    if kind == ShellKind::Cmd {
        return None;
    }
    Some(
        std::iter::once(program)
            .chain(args.iter().map(String::as_str))
            .map(str::trim)
            .filter(|w| !w.is_empty())
            .map(|w| quote(w, kind))
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn quote(word: &str, kind: ShellKind) -> String {
    // PowerShell reads `,` as the array operator, a leading `@` as splatting and a bare `%` as
    // ForEach-Object, so its bare set is narrower than POSIX's.
    let plain = |c: char| {
        c.is_ascii_alphanumeric()
            || match kind {
                ShellKind::PowerShell => r"_+=:./\-".contains(c),
                _ => "_@%+=:,./-".contains(c),
            }
    };
    if word.chars().all(plain) {
        word.to_owned()
    } else if kind == ShellKind::PowerShell {
        format!("'{}'", word.replace('\'', "''"))
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
        let terminal = self.find_terminal(terminal_id)?;
        if terminal.agent.is_some() {
            return Err(AgentError::AlreadyRunning);
        }
        let kind = terminal
            .program
            .as_deref()
            .map_or(ShellKind::Posix, ShellKind::of);
        let line = command_line(program, args, kind).ok_or(AgentError::UnsupportedShell)?;
        self.input(terminal_id, &format!("{line}\r"))?;
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

    /// Runs `program args` for an agent without hooks, which never reports: returns right after
    /// the command line is typed, without waiting. The terminal then shows no agent status. A
    /// terminal that already has an agent is refused before anything is typed.
    pub fn start_unreported(
        &self,
        terminal_id: &str,
        program: &str,
        args: &[String],
    ) -> Result<(), AgentError> {
        let terminal = self.find_terminal(terminal_id)?;
        if terminal.agent.is_some() {
            return Err(AgentError::AlreadyRunning);
        }
        let kind = terminal
            .program
            .as_deref()
            .map_or(ShellKind::Posix, ShellKind::of);
        let line = command_line(program, args, kind).ok_or(AgentError::UnsupportedShell)?;
        self.input(terminal_id, &format!("{line}\r"))?;
        Ok(())
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

    fn line(kind: ShellKind, args: &[&str]) -> Option<String> {
        command_line("claude", &words(args), kind)
    }

    #[test]
    fn plain_words_stay_bare_in_every_quoting_shell() {
        for kind in [ShellKind::Posix, ShellKind::PowerShell] {
            assert_eq!(
                line(kind, &["--model", "opus-4.5"]).unwrap(),
                "claude --model opus-4.5"
            );
        }
    }

    #[test]
    fn posix_quotes_an_apostrophe_with_the_escape_sequence() {
        assert_eq!(
            line(ShellKind::Posix, &["fix it's \"bug\" $(x)\nnow"]).unwrap(),
            "claude 'fix it'\\''s \"bug\" $(x)\nnow'"
        );
        assert_eq!(
            line(ShellKind::Posix, &["don't stop"]).unwrap(),
            "claude 'don'\\''t stop'"
        );
    }

    #[test]
    fn powershell_doubles_an_apostrophe() {
        assert_eq!(
            line(ShellKind::PowerShell, &["don't stop"]).unwrap(),
            "claude 'don''t stop'"
        );
        assert_eq!(
            line(ShellKind::PowerShell, &["two words", "$x"]).unwrap(),
            "claude 'two words' '$x'"
        );
    }

    #[test]
    fn powershell_quotes_comma_at_and_percent() {
        assert_eq!(
            line(ShellKind::PowerShell, &["a,b", "@x", "%", r"C:\tmp\f.txt"]).unwrap(),
            r"claude 'a,b' '@x' '%' C:\tmp\f.txt"
        );
        assert_eq!(
            line(ShellKind::Posix, &["a,b", "@x", "%"]).unwrap(),
            "claude a,b @x %"
        );
    }

    #[test]
    fn empty_arguments_are_dropped() {
        for kind in [ShellKind::Posix, ShellKind::PowerShell] {
            assert_eq!(
                command_line(" codex ", &words(&["", "  "]), kind).unwrap(),
                "codex"
            );
        }
    }

    #[test]
    fn cmd_gets_no_command_line() {
        assert_eq!(line(ShellKind::Cmd, &["a b"]), None);
        assert_eq!(line(ShellKind::Cmd, &[]), None);
    }

    #[test]
    fn the_shell_kind_follows_the_program_name() {
        assert_eq!(ShellKind::of("/bin/zsh"), ShellKind::Posix);
        assert_eq!(
            ShellKind::of(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            ShellKind::PowerShell
        );
        assert_eq!(ShellKind::of("POWERSHELL.EXE"), ShellKind::PowerShell);
        assert_eq!(
            ShellKind::of(r"C:\Windows\System32\cmd.exe"),
            ShellKind::Cmd
        );
    }
}
