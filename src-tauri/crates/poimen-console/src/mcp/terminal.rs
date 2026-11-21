//! The terminal views of the read tools.

use serde_json::{Value, json};

pub(super) fn terminal_json(t: &poimen_client::protocol::Terminal) -> Value {
    let agent = t.agent.as_ref().map(|a| {
        json!({
            "kind": a.agent,
            "status": a.status,
            "tone": a.tone,
            "message": a.message,
        })
    });
    json!({
        "id": t.id,
        "space": t.space_id,
        "name": t.name,
        "directory": t.cwd,
        "state": t.status,
        "agent": agent,
        "tokens": t.tokens,
    })
}

/// `get_terminal` (#1153): the compact view plus the source of the agent status, its times, the
/// child records and the status history.
pub(super) fn terminal_detail(t: &poimen_client::protocol::Terminal) -> Value {
    let mut v = terminal_json(t);
    if let Some(a) = &t.agent {
        v["agent"] = json!({
            "kind": a.agent,
            "status": a.status,
            "source": t.agent_source,
            "message": a.message,
            "message_at": a.message_at,
            "status_since": a.status_since,
            "reported_at": a.reported_at,
            "silent_since": a.silent_since,
            "title": a.title,
            "progress": a.progress,
            "block_kind": a.kind,
            "tone": a.tone,
            "session_id": a.session_id,
        });
    }
    v["hibernated"] = json!(t.hibernated);
    v["records"] = json!(t.records);
    v["status_changes"] = json!(t.status_history);
    v
}
