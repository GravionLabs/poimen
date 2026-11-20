//! Hibernation of idle agents (ADR 0014, #903): a terminal whose agent has been `done` for
//! `agents.hibernateAfter` minutes gets the harness's exit input typed, and the server keeps what
//! a resume needs in `Terminal.hibernated`. The terminal and its shell are never closed. Resuming
//! and showing it are other work.
//!
//! Resuming (#904): the first controller attach of a hibernated terminal types the harness's
//! resume command line; an observer never does.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::agent::{ShellKind, command_line};
use poimen_core::harness;
use poimen_core::settings::{config_path, hibernate_after_minutes};
use poimen_protocol::{AgentStatus, Event, Hibernated, TerminalStatus};

use crate::{Shared, lock};

/// How often the sweeper looks.
pub(crate) const SWEEP: Duration = Duration::from_secs(5);

/// How long a resumed agent has to report before the resume counts as failed.
pub(crate) const RESUME_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a resume waits for the exited agent's `gone` report before it gives up.
const EXIT_WAIT: Duration = Duration::from_secs(10);
/// Time for the shell to draw its prompt after the agent ended.
const PROMPT_SETTLE: Duration = Duration::from_millis(300);
const POLL: Duration = Duration::from_millis(50);

/// The harnesses of the user config, read now (the defaults without a config).
fn harnesses_of(shared: &Shared) -> Vec<harness::Harness> {
    let root = shared
        .home
        .as_deref()
        .and_then(|home| std::fs::read_to_string(config_path(home)).ok())
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());
    match root.as_ref().and_then(|r| r.get("harnesses")) {
        Some(v) => harness::resolve(v, &mut Vec::new()),
        None => harness::default_harnesses(),
    }
}

/// The setting and the harnesses of the user config, read now; `None` while hibernation is off
/// (no config, no key, `0`, or an invalid value).
fn wanted(shared: &Shared) -> Option<(u64, Vec<harness::Harness>)> {
    let home = shared.home.as_deref()?;
    let text = std::fs::read_to_string(config_path(home)).ok()?;
    let root: serde_json::Value = serde_json::from_str(&text).ok()?;
    let minutes = hibernate_after_minutes(&root).ok().flatten()?;
    let mut warnings = Vec::new();
    let harnesses = match root.get("harnesses") {
        Some(v) => harness::resolve(v, &mut warnings),
        None => harness::default_harnesses(),
    };
    Some((minutes, harnesses))
}

impl Shared {
    /// One pass at `now_ms`: hibernates every terminal that meets all of the rules of the ADR.
    pub(crate) fn hibernate_sweep(&self, now_ms: u64) {
        let Some((minutes, harnesses)) = wanted(self) else {
            return;
        };
        let after_ms = minutes.saturating_mul(60_000);
        let mut typing = Vec::new();
        {
            let mut inner = lock(&self.inner);
            for entry in &mut inner.terminals {
                if entry.info.hibernated.is_some()
                    || entry.osc.is_some()
                    || entry.stream.has_controller()
                {
                    continue;
                }
                let Some(agent) = entry.info.agent.as_ref() else {
                    continue;
                };
                if agent.status != AgentStatus::Done {
                    continue;
                }
                let (Some(kind), Some(session), Some(since)) = (
                    agent.agent.as_deref(),
                    agent.session_id.as_deref(),
                    agent.status_since,
                ) else {
                    continue;
                };
                if now_ms.saturating_sub(since) < after_ms {
                    continue;
                }
                let Some(h) = harnesses.iter().find(|h| h.id == kind) else {
                    continue;
                };
                let Some(exit) = h.exit_input.as_deref().filter(|e| !e.trim().is_empty()) else {
                    continue;
                };
                if h.resume_args.is_empty() || session.is_empty() {
                    continue;
                }
                let Some(writer) = entry.host.as_ref().map(|h| h.writer()) else {
                    continue;
                };
                let hibernated = Hibernated {
                    agent: kind.to_owned(),
                    session_id: session.to_owned(),
                    since: now_ms,
                };
                // Set before typing, so a second pass never types again.
                entry.info.hibernated = Some(hibernated.clone());
                self.publish(Event::TerminalHibernatedChanged {
                    terminal_id: entry.info.id.clone(),
                    hibernated: Some(hibernated),
                });
                typing.push((writer, format!("{exit}\r")));
            }
            if typing.is_empty() {
                return;
            }
            self.persist(&inner);
        }
        for (writer, text) in typing {
            if let Err(e) = writer.write(text.as_bytes()) {
                self.log("hibernate", &format!("the exit input was not typed: {e}"));
            }
        }
    }
}

/// What a resume needs, taken under the lock.
struct Resume {
    writer: crate::pty::PtyWriter,
    line: String,
}

impl Shared {
    /// A controller attached to `terminal_id`: when its agent is hibernated and no resume runs,
    /// types the harness's resume command once the shell is at its prompt, and waits for the
    /// agent's report. Without one in time the hibernation ends, the shell stays and a
    /// `terminal.resume_failed` event names the failure.
    pub(crate) fn resume_hibernated(self: &Arc<Self>, terminal_id: &str) {
        let harnesses = harnesses_of(self);
        let started = {
            let mut inner = lock(&self.inner);
            let Some(entry) = inner
                .terminals
                .iter_mut()
                .find(|t| t.info.id == terminal_id)
            else {
                return;
            };
            let Some(hibernated) = entry.info.hibernated.clone() else {
                return;
            };
            if entry.resuming {
                return;
            }
            let plan = (|| {
                let h = harnesses.iter().find(|h| h.id == hibernated.agent)?;
                let argv = harness::build_resume_argv(h, &hibernated.session_id)?;
                let kind = entry
                    .info
                    .program
                    .as_deref()
                    .map_or(ShellKind::Posix, ShellKind::of);
                let line = command_line(&argv[0], &argv[1..], kind)?;
                let writer = entry.host.as_ref()?.writer();
                Some(Resume { writer, line })
            })();
            match plan {
                Some(plan) => {
                    entry.resuming = true;
                    Ok(plan)
                }
                None => Err(format!(
                    "no way to resume the {} session in this terminal",
                    hibernated.agent
                )),
            }
        };
        match started {
            Ok(plan) => {
                let shared = Arc::clone(self);
                let id = terminal_id.to_owned();
                thread::spawn(move || shared.run_resume(&id, plan));
            }
            Err(reason) => self.end_hibernation(terminal_id, Some(&reason)),
        }
    }

    fn run_resume(&self, id: &str, plan: Resume) {
        // The shell is at its prompt once the agent said it is gone.
        let exit_deadline = Instant::now() + EXIT_WAIT;
        loop {
            match self.resume_state(id) {
                ResumeState::Over => return,
                ResumeState::AgentGone => break,
                ResumeState::AgentRuns => {}
            }
            if Instant::now() >= exit_deadline {
                self.end_hibernation(id, Some("the agent has not exited, so it was not resumed"));
                return;
            }
            thread::sleep(POLL);
        }
        thread::sleep(PROMPT_SETTLE);
        if let Err(e) = plan.writer.write(format!("{}\r", plan.line).as_bytes()) {
            self.end_hibernation(id, Some(&format!("the resume command was not typed: {e}")));
            return;
        }
        let timeout = Duration::from_millis(self.resume_timeout_ms.load(Ordering::SeqCst));
        let deadline = Instant::now() + timeout;
        loop {
            if self.resume_state(id) == ResumeState::Over {
                return;
            }
            if Instant::now() >= deadline {
                let reason = format!(
                    "the agent did not report within {} s; the shell is left at its prompt",
                    timeout.as_secs().max(1)
                );
                self.end_hibernation(id, Some(&reason));
                return;
            }
            thread::sleep(POLL);
        }
    }

    /// `Over` when the hibernation ended (the agent reported again, the terminal is gone).
    fn resume_state(&self, id: &str) -> ResumeState {
        let inner = lock(&self.inner);
        match inner.terminals.iter().find(|t| t.info.id == id) {
            Some(e) if e.info.hibernated.is_some() && e.info.status == TerminalStatus::Running => {
                match e.info.agent.as_ref() {
                    None => ResumeState::AgentGone,
                    Some(a) if a.status == AgentStatus::Gone => ResumeState::AgentGone,
                    Some(_) => ResumeState::AgentRuns,
                }
            }
            _ => ResumeState::Over,
        }
    }

    /// Ends the hibernation of `id` and the resume under way, with the failure to tell.
    fn end_hibernation(&self, id: &str, failure: Option<&str>) {
        let mut inner = lock(&self.inner);
        let Some(entry) = inner.terminals.iter_mut().find(|t| t.info.id == id) else {
            return;
        };
        entry.resuming = false;
        if entry.info.hibernated.take().is_none() {
            return;
        }
        self.publish(Event::TerminalHibernatedChanged {
            terminal_id: id.to_owned(),
            hibernated: None,
        });
        if let Some(reason) = failure {
            self.publish(Event::TerminalResumeFailed {
                terminal_id: id.to_owned(),
                reason: reason.to_owned(),
            });
        }
        self.persist(&inner);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResumeState {
    AgentRuns,
    AgentGone,
    Over,
}
