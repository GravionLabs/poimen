//! The scoped write grant (ADR 0018, #1211). A lead agent asks once for a plan over several
//! spaces (`agent.grant_request`), the user confirms in the app (`agent.grant_answer`), and
//! `poimen mcp` asks `agent.grant_check` before each write: a covered write needs no dialog, any
//! other write falls back to the per-call confirmation of ADR 0012. Grants live in memory and end
//! with the server, the duration, the lead's terminal or a revocation.

use std::collections::HashMap;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use poimen_protocol::{
    AgentGrantAnswer, AgentGrantCheck, AgentGrantRequest, AgentGrantRevoke, AgentGrants,
    AgentStatus, AgentWriteAction, ConfirmReason, ErrorCode, Event, GRANT_DEFAULT_SECS,
    GRANT_MAX_SECS, GRANT_WAIT_MS, GrantInfo, GrantResult, GrantSpace, GrantVerdict, Request,
    TerminalStatus,
};
use serde_json::{Value, json};

use crate::{ApiResult, Shared, invalid, lock, not_found, now_millis, parsed};

/// A confirmed grant.
#[derive(Debug, Clone)]
struct Grant {
    id: String,
    lead: String,
    spaces: Vec<String>,
    tools: Vec<AgentWriteAction>,
    max_agents: u32,
    /// Milliseconds since the epoch.
    expires_at: u64,
}

/// A request the app has not answered yet; what the answer may narrow.
struct Ask {
    answer: Sender<Option<Narrowed>>,
    spaces: Vec<String>,
    max_agents: u32,
}

/// The user's yes: all the spaces and the limit asked for, or fewer.
struct Narrowed {
    spaces: Vec<String>,
    max_agents: u32,
}

/// The state of the grants of one server.
#[derive(Default)]
pub(super) struct Grants {
    active: std::sync::Mutex<Vec<Grant>>,
    asks: std::sync::Mutex<HashMap<String, Ask>>,
}

impl Shared {
    /// Whether the terminal exists and still runs.
    fn terminal_alive(&self, id: &str) -> bool {
        lock(&self.inner)
            .terminals
            .iter()
            .any(|t| t.info.id == id && t.info.status != TerminalStatus::Exited)
    }

    /// The agents `lead` started that have not ended (`done`, `error`, `gone`) or exited.
    fn running_agents(&self, lead: &str) -> u32 {
        let inner = lock(&self.inner);
        let n = inner
            .terminals
            .iter()
            .filter(|t| t.info.started_by.as_deref() == Some(lead))
            .filter(|t| t.info.status != TerminalStatus::Exited)
            .filter(|t| {
                t.info.agent.as_ref().is_some_and(|a| {
                    !matches!(
                        a.status,
                        AgentStatus::Done | AgentStatus::Error | AgentStatus::Gone
                    )
                })
            })
            .count();
        u32::try_from(n).unwrap_or(u32::MAX)
    }

    fn terminal_name(&self, id: &str) -> Option<String> {
        lock(&self.inner)
            .terminals
            .iter()
            .find(|t| t.info.id == id)
            .map(|t| t.info.name.clone())
    }

    fn space_of(&self, id: &str) -> Option<GrantSpace> {
        lock(&self.inner)
            .layout
            .spaces
            .iter()
            .find(|s| s.id == id)
            .map(|s| GrantSpace {
                id: s.id.clone(),
                name: s.name.clone(),
                path: Some(s.cwd.clone()).filter(|c| !c.is_empty()),
            })
    }

    fn grant_info(&self, g: &Grant) -> GrantInfo {
        let lead_name = lock(&self.inner)
            .terminals
            .iter()
            .find(|t| t.info.id == g.lead)
            .map_or_else(|| g.lead.clone(), |t| t.info.name.clone());
        GrantInfo {
            id: g.id.clone(),
            lead_terminal_id: g.lead.clone(),
            lead_name,
            spaces: g.spaces.iter().filter_map(|s| self.space_of(s)).collect(),
            tools: g.tools.clone(),
            max_agents: g.max_agents,
            running: self.running_agents(&g.lead),
            expires_at: g.expires_at,
        }
    }

    fn grants_snapshot(&self) -> AgentGrants {
        let active = lock(&self.confirms.grants.active).clone();
        AgentGrants {
            grants: active.iter().map(|g| self.grant_info(g)).collect(),
        }
    }

    fn publish_grants(&self) {
        self.publish_to_apps(&Event::AgentGrantsChanged {
            grants: self.grants_snapshot().grants,
        });
    }

    /// Drops the grants that ran out or whose lead terminal is gone; true when it dropped one.
    pub(crate) fn prune_grants(&self) -> bool {
        let now = now_millis();
        let leads: Vec<String> = lock(&self.confirms.grants.active)
            .iter()
            .map(|g| g.lead.clone())
            .collect();
        let alive: Vec<bool> = leads.iter().map(|l| self.terminal_alive(l)).collect();
        let mut active = lock(&self.confirms.grants.active);
        let before = active.len();
        active.retain(|g| {
            g.expires_at > now
                && leads
                    .iter()
                    .zip(&alive)
                    .any(|(l, alive)| *l == g.lead && *alive)
        });
        let dropped = active.len() != before;
        drop(active);
        if dropped {
            self.publish_grants();
        }
        dropped
    }
}

/// The `agent.grant_*` and `agent.grants` methods.
pub(super) fn dispatch(shared: &Shared, method: &str, req: &Request) -> ApiResult {
    match method {
        "agent.grant_request" => parsed(req).and_then(|p| request(shared, p)),
        "agent.grant_answer" => parsed(req).and_then(|p| answer(shared, p)),
        "agent.grant_revoke" => parsed(req).and_then(|p| revoke(shared, p)),
        "agent.grant_check" => parsed(req).map(|p| check(shared, &p)),
        "agent.grants" => {
            shared.prune_grants();
            Ok(json!(shared.grants_snapshot()))
        }
        other => Err((ErrorCode::UnknownMethod, format!("unknown method {other}"))),
    }
}

/// `agent.grant_request`: blocks this connection until the user answered, `wait_ms` passed or no
/// app is connected. Never grants by itself; a second request of the lead replaces its grant once
/// confirmed.
fn request(shared: &Shared, p: AgentGrantRequest) -> ApiResult {
    if !shared.terminal_alive(&p.caller_terminal_id) {
        return Err(not_found("terminal", &p.caller_terminal_id));
    }
    let lead_name = shared
        .terminal_name(&p.caller_terminal_id)
        .unwrap_or_else(|| p.caller_terminal_id.clone());
    if p.plan.trim().is_empty() {
        return Err(invalid("`plan` is empty"));
    }
    if p.tools.is_empty() || p.tools.contains(&AgentWriteAction::WriteTerminal) {
        return Err(invalid(
            "`tools` must name write tools and never write_terminal",
        ));
    }
    if p.max_agents == 0 {
        return Err(invalid("`max_agents` must be at least 1"));
    }
    let mut ids: Vec<String> = Vec::new();
    for id in &p.spaces {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    if ids.is_empty() {
        return Err(invalid("`spaces` is empty"));
    }
    let spaces = ids
        .iter()
        .map(|id| shared.space_of(id).ok_or_else(|| not_found("space", id)))
        .collect::<Result<Vec<_>, _>>()?;
    let mut tools = p.tools.clone();
    tools.dedup();
    let duration_s = p
        .duration_s
        .unwrap_or(GRANT_DEFAULT_SECS)
        .clamp(1, GRANT_MAX_SECS);
    let wait_ms = p.wait_ms.unwrap_or(GRANT_WAIT_MS).min(GRANT_WAIT_MS);

    let id = shared.secret_id("grant");
    let (tx, rx) = mpsc::channel();
    lock(&shared.confirms.grants.asks).insert(
        id.clone(),
        Ask {
            answer: tx,
            spaces: ids,
            max_agents: p.max_agents,
        },
    );
    let sent = shared.publish_to_apps(&Event::AgentGrantRequested {
        id: id.clone(),
        lead_terminal_id: p.caller_terminal_id.clone(),
        lead_name,
        plan: p.plan,
        spaces,
        tools: tools.clone(),
        max_agents: p.max_agents,
        duration_s,
        wait_ms,
    });
    let (outcome, reason) = if !sent {
        (None, ConfirmReason::NoApp)
    } else {
        match rx.recv_timeout(Duration::from_millis(wait_ms)) {
            Ok(Some(n)) => (Some(n), ConfirmReason::Approved),
            Ok(None) => (None, ConfirmReason::Declined),
            Err(_) => (None, ConfirmReason::Timeout),
        }
    };
    lock(&shared.confirms.grants.asks).remove(&id);
    if sent {
        shared.publish_to_apps(&Event::AgentGrantResolved { id: id.clone() });
    }
    let Some(n) = outcome else {
        return Ok(json!(GrantResult {
            granted: false,
            reason,
            grant: None
        }));
    };
    let grant = Grant {
        id,
        lead: p.caller_terminal_id,
        spaces: n.spaces,
        tools,
        max_agents: n.max_agents,
        expires_at: now_millis() + duration_s * 1000,
    };
    let info = shared.grant_info(&grant);
    {
        let mut active = lock(&shared.confirms.grants.active);
        active.retain(|g| g.lead != grant.lead);
        active.push(grant);
    }
    shared.publish_grants();
    Ok(json!(GrantResult {
        granted: true,
        reason,
        grant: Some(info)
    }))
}

/// `agent.grant_answer`: the user's answer. A yes can only narrow what was asked; an answer that
/// widens it is `invalid_params` and leaves the request open. A settled id is `not_found`.
fn answer(shared: &Shared, p: AgentGrantAnswer) -> ApiResult {
    let mut asks = lock(&shared.confirms.grants.asks);
    let Some(ask) = asks.get(&p.id) else {
        return Err(not_found("grant request", &p.id));
    };
    let decision = if p.approve {
        let spaces = match p.spaces {
            None => ask.spaces.clone(),
            Some(s) => {
                if s.is_empty() {
                    return Err(invalid("`spaces` is empty: decline instead"));
                }
                if let Some(extra) = s.iter().find(|id| !ask.spaces.contains(id)) {
                    return Err(invalid(format!("space {extra} was not asked for")));
                }
                s
            }
        };
        let max_agents = p.max_agents.unwrap_or(ask.max_agents);
        if max_agents == 0 || max_agents > ask.max_agents {
            return Err(invalid("`max_agents` can only be lowered, and not to 0"));
        }
        Some(Narrowed { spaces, max_agents })
    } else {
        None
    };
    if let Some(ask) = asks.remove(&p.id) {
        let _ = ask.answer.send(decision);
    }
    Ok(json!({ "accepted": true }))
}

/// `agent.grant_revoke`: by id, or the grant of the caller's terminal. Ends no terminal and sends
/// no input: running agents are left alone (ADR 0018).
fn revoke(shared: &Shared, p: AgentGrantRevoke) -> ApiResult {
    let removed = {
        let mut active = lock(&shared.confirms.grants.active);
        let before = active.len();
        match (&p.id, &p.caller_terminal_id) {
            (Some(id), _) => active.retain(|g| g.id != *id),
            (None, Some(lead)) => active.retain(|g| g.lead != *lead),
            (None, None) => return Err(invalid("`id` or `caller_terminal_id` is required")),
        }
        before - active.len()
    };
    if removed > 0 {
        shared.publish_grants();
    }
    Ok(json!({ "revoked": removed > 0 }))
}

/// `agent.grant_check`: `covered` when a live grant of the caller covers the write, `refused`
/// when one does but the limit of parallel agents is reached, else `needs_confirmation`.
fn check(shared: &Shared, p: &AgentGrantCheck) -> Value {
    shared.prune_grants();
    let found = lock(&shared.confirms.grants.active)
        .iter()
        .find(|g| {
            g.lead == p.caller_terminal_id
                && g.tools.contains(&p.action)
                && g.spaces.contains(&p.space_id)
        })
        .cloned();
    let verdict = match found {
        None => GrantVerdict::NeedsConfirmation,
        Some(g) => {
            let running = shared.running_agents(&g.lead);
            if p.action == AgentWriteAction::StartAgent && running >= g.max_agents {
                GrantVerdict::Refused {
                    reason: format!(
                        "refused: the grant allows {} agents at once and {running} are running; \
                         wait for one to finish",
                        g.max_agents
                    ),
                }
            } else {
                GrantVerdict::Covered { grant: g.id }
            }
        }
    };
    json!(verdict)
}
