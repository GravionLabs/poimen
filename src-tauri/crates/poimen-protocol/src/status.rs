//! Hibernation and the status history of a terminal.

use serde::{Deserialize, Serialize};

use crate::AgentStatus;

/// A hibernated agent: what a resume needs (ADR 0014). Set by the server's sweeper before it
/// types the harness's exit input, kept when the hook reports the agent `gone`, cleared by a
/// resume or by any other agent report (the user came back).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hibernated {
    /// Which agent (the harness id, e.g. `claude`).
    pub agent: String,
    pub session_id: String,
    /// When the server hibernated it, milliseconds since the epoch.
    pub since: u64,
}

/// Most status changes a terminal keeps.
pub const MAX_STATUS_HISTORY: usize = 20;

/// One change of the agent status of a terminal (#1153).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusChange {
    /// Milliseconds since the epoch.
    pub at: u64,
    pub status: AgentStatus,
    /// `osc7501` or the hook kind; absent for `gone`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::Terminal;

    #[test]
    fn status_history_is_additive() {
        let t: Terminal = serde_json::from_value(json!({
            "id":"t1","space_id":"s1","name":"x","cwd":"/","status":"running",
            "cols":80,"rows":24,
            "agent_source":"osc7501",
            "status_history":[{"at":5,"status":"done","source":"osc7501","message":"m"},{"at":6,"status":"gone"}]
        })).unwrap();
        assert_eq!(t.agent_source.as_deref(), Some("osc7501"));
        assert_eq!(t.status_history.len(), 2);
        assert_eq!(t.status_history[1].source, None);
        let v = serde_json::to_value(&t).unwrap();
        assert_eq!(v["status_history"][0]["message"], "m");
    }
}
