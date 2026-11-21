//! Agent status events and the reset of a failed turn (#1110).

use poimen_protocol::{AgentInfo, AgentStatus, Event};

use crate::{Inner, Shared, now_millis};

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
    shared.publish(agent_event(terminal_id, entry.info.agent.as_ref()));
    shared.persist(inner);
}
