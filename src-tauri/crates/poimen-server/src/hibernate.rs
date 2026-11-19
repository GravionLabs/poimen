//! Hibernation of idle agents (ADR 0014, #903): a terminal whose agent has been `done` for
//! `agents.hibernateAfter` minutes gets the harness's exit input typed, and the server keeps what
//! a resume needs in `Terminal.hibernated`. The terminal and its shell are never closed. Resuming
//! and showing it are other work.

use std::time::Duration;

use poimen_core::harness;
use poimen_core::settings::{config_path, hibernate_after_minutes};
use poimen_protocol::{AgentStatus, Event, Hibernated};

use crate::{Shared, lock};

/// How often the sweeper looks.
pub(crate) const SWEEP: Duration = Duration::from_secs(5);

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
                eprintln!("poimen-server: hibernate: the exit input was not typed: {e}");
            }
        }
    }
}
