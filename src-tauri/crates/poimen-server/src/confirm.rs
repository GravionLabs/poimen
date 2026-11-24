//! Confirmations of an agent's writes into another space (ADR 0012, #615): `agent.confirm` asks
//! the apps connected to the server and waits for `agent.confirm_answer`, and `agent.write_notice`
//! tells them what an agent wrote. The server only relays; `poimen mcp` decides when to ask.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use poimen_protocol::{
    AgentConfirm, AgentConfirmAnswer, AgentWriteNotice, CONFIRM_WAIT_MS, ClientKind, ConfirmReason,
    ConfirmResult, ErrorCode, Event,
};
use serde_json::json;

use crate::{ApiResult, Shared, lock, not_found, parsed};

mod grants;

/// The open `agent.confirm` requests (#615): the answer of the app goes to the waiting one; and
/// the scoped write grants (ADR 0018, #1211).
#[derive(Default)]
pub(crate) struct Confirms {
    asks: Mutex<HashMap<String, Sender<bool>>>,
    grants: grants::Grants,
}

/// Whether `method` is one of this module's.
pub(crate) fn handles(method: &str) -> bool {
    matches!(
        method,
        "agent.confirm" | "agent.confirm_answer" | "agent.write_notice" | "agent.grants"
    ) || method.starts_with("agent.grant_")
}

/// `agent.confirm*`, `agent.write_notice` and the grant methods.
pub(crate) fn dispatch(shared: &Shared, request: &poimen_protocol::Request) -> ApiResult {
    match request.method.as_str() {
        "agent.confirm" => agent_confirm(shared, parsed(request)?),
        "agent.confirm_answer" => agent_confirm_answer(shared, parsed(request)?),
        "agent.write_notice" => agent_write_notice(shared, parsed(request)?),
        other => grants::dispatch(shared, other, request),
    }
}

impl Shared {
    /// Sends the event to the subscriptions that said they are an app and show confirmations; false when there is none.
    pub(crate) fn publish_to_apps(&self, event: &Event) -> bool {
        let mut sent = false;
        lock(&self.subscribers).retain(|s| {
            if s.kind != ClientKind::App || !s.confirms {
                return true;
            }
            let alive = s.tx.send(event.clone()).is_ok();
            sent |= alive;
            alive
        });
        sent
    }

    /// `(terminal name, space name)` of a terminal and a space, or `not_found`.
    fn names_for_write(
        &self,
        caller: &str,
        space_id: &str,
    ) -> Result<(String, String), (ErrorCode, String)> {
        let inner = lock(&self.inner);
        let terminal = inner
            .terminals
            .iter()
            .find(|t| t.info.id == caller)
            .ok_or_else(|| not_found("terminal", caller))?;
        let space = inner
            .layout
            .spaces
            .iter()
            .find(|s| s.id == space_id)
            .ok_or_else(|| not_found("space", space_id))?;
        Ok((terminal.info.name.clone(), space.name.clone()))
    }
}

/// `agent.confirm`: blocks this connection until the user answered, `wait_ms` passed or no app is
/// connected. Never approves by itself.
pub(crate) fn agent_confirm(shared: &Shared, p: AgentConfirm) -> ApiResult {
    let (caller_name, space_name) = shared.names_for_write(&p.caller_terminal_id, &p.space_id)?;
    let wait_ms = p.wait_ms.unwrap_or(CONFIRM_WAIT_MS).min(CONFIRM_WAIT_MS);
    let id = shared.secret_id("confirm");
    let (tx, rx) = mpsc::channel();
    lock(&shared.confirms.asks).insert(id.clone(), tx);
    let sent = shared.publish_to_apps(&Event::AgentConfirmRequested {
        id: id.clone(),
        caller_terminal_id: p.caller_terminal_id,
        caller_name,
        action: p.action,
        space_id: p.space_id,
        space_name,
        terminal_id: p.terminal_id,
        text: p.text,
        wait_ms,
    });
    let result = if !sent {
        ConfirmResult {
            approved: false,
            reason: ConfirmReason::NoApp,
        }
    } else {
        match rx.recv_timeout(Duration::from_millis(wait_ms)) {
            Ok(true) => ConfirmResult {
                approved: true,
                reason: ConfirmReason::Approved,
            },
            Ok(false) => ConfirmResult {
                approved: false,
                reason: ConfirmReason::Declined,
            },
            Err(_) => ConfirmResult {
                approved: false,
                reason: ConfirmReason::Timeout,
            },
        }
    };
    lock(&shared.confirms.asks).remove(&id);
    if sent {
        shared.publish_to_apps(&Event::AgentConfirmResolved { id });
    }
    Ok(json!(result))
}

/// `agent.confirm_answer`: the user's answer; an id that is settled already is `not_found`.
pub(crate) fn agent_confirm_answer(shared: &Shared, p: AgentConfirmAnswer) -> ApiResult {
    let tx = lock(&shared.confirms.asks).remove(&p.id);
    match tx {
        Some(tx) => {
            let _ = tx.send(p.approve);
            Ok(json!({ "accepted": true }))
        }
        None => Err(not_found("confirmation", &p.id)),
    }
}

/// `agent.write_notice`: tells the apps what an agent wrote.
pub(crate) fn agent_write_notice(shared: &Shared, p: AgentWriteNotice) -> ApiResult {
    let (caller_name, space_name) = shared.names_for_write(&p.caller_terminal_id, &p.space_id)?;
    let delivered = shared.publish_to_apps(&Event::AgentWrote {
        caller_terminal_id: p.caller_terminal_id,
        caller_name,
        action: p.action,
        space_id: p.space_id,
        space_name,
        terminal_id: p.terminal_id,
        text: p.text,
        confirmed: p.confirmed,
    });
    Ok(json!({ "delivered": delivered }))
}
