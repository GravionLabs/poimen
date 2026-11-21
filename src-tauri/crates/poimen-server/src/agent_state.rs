//! Agent status events and the reset of a failed turn (#1110).

use poimen_protocol::{AgentInfo, AgentStatus, Event};

use crate::{Inner, Shared, TerminalEntry, now_millis};

impl TerminalEntry {
    /// Keeps `info.agent_source` and the status history (#1153) in line with `info.agent`: a
    /// change of status or source adds a record (at most `MAX_STATUS_HISTORY`, oldest dropped).
    pub(crate) fn track_status(&mut self, now_ms: u64) {
        let source = self.info.agent.as_ref().map(|a| {
            if self.osc.is_some() {
                "osc7501".to_string()
            } else {
                a.agent.clone().unwrap_or_default()
            }
        });
        self.info.agent_source.clone_from(&source);
        let (status, message) = self
            .info
            .agent
            .as_ref()
            .map_or((AgentStatus::Gone, None), |a| (a.status, a.message.clone()));
        let last = self.info.status_history.last();
        let same = last.is_some_and(|l| l.status == status && l.source == source);
        let nothing = last.is_none() && self.info.agent.is_none();
        if same || nothing {
            return;
        }
        self.info
            .status_history
            .push(poimen_protocol::StatusChange {
                at: now_ms,
                status,
                source,
                message,
            });
        let over = self
            .info
            .status_history
            .len()
            .saturating_sub(poimen_protocol::MAX_STATUS_HISTORY);
        self.info.status_history.drain(..over);
    }
}

/// The event for the agent of a terminal; no agent is `gone`.
pub(crate) fn agent_event(terminal_id: &str, agent: Option<&AgentInfo>) -> Event {
    Event::AgentStatusChanged {
        terminal_id: terminal_id.to_string(),
        status: agent.map_or(AgentStatus::Gone, |a| a.status),
        agent: agent.and_then(|a| a.agent.clone()),
        session_id: agent.and_then(|a| a.session_id.clone()),
        message: agent.and_then(|a| a.message.clone()),
        message_at: agent.and_then(|a| a.message_at),
        progress: agent.and_then(|a| a.progress),
        kind: agent.and_then(|a| a.kind.clone()),
        tone: agent.and_then(|a| a.tone.clone()),
        title: agent.and_then(|a| a.title.clone()),
        silent_since: agent.and_then(|a| a.silent_since),
    }
}

/// A failed turn (`error`) ends when the user moves on: input with Enter (a prompt, a command
/// such as `clear`) sets the status to `idle`. A new turn is reported by the hooks as
/// `working` anyway. A program that reports its own status (OSC 7501) owns it and is left alone.
pub(crate) fn reset_error_on_input(
    shared: &Shared,
    inner: &mut Inner,
    terminal_id: &str,
    data: &str,
) {
    if !data.contains(['\r', '\n']) {
        return;
    }
    let Some(entry) = inner
        .terminals
        .iter_mut()
        .find(|t| t.info.id == terminal_id)
    else {
        return;
    };
    if entry.osc.is_some() {
        return;
    }
    let Some(agent) = entry
        .info
        .agent
        .as_mut()
        .filter(|a| a.status == AgentStatus::Error)
    else {
        return;
    };
    let now = now_millis();
    agent.status = AgentStatus::Idle;
    agent.message = None;
    agent.message_at = None;
    agent.status_since = Some(now);
    agent.reported_at = Some(now);
    entry.track_status(now);
    shared.publish(agent_event(terminal_id, entry.info.agent.as_ref()));
    shared.persist(inner);
}
