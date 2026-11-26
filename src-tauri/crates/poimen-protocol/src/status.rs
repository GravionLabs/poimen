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

/// The agent of a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentInfo {
    pub status: AgentStatus,
    /// Which agent, e.g. `claude`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// The latest notification text of the agent (see [`clean_message`]); cleared when the agent
    /// turns `working` again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// When `message` was accepted, milliseconds since the epoch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_at: Option<u64>,
    /// Progress of the program, 0 to 100 (OSC 7501 `progress`, #682).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<u8>,
    /// Why the agent is blocked: `permission`, `question` or `auth` (OSC 7501 `kind`, #682).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// `warning` for a `done` that wants a look (OSC 7501 `tone`, #1152); only with `done`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<String>,
    /// The program's own title (OSC 7501 `title`, #682), plain text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// When the last accepted `terminal.report_agent` came in, milliseconds since the epoch,
    /// also when the status did not change (#897).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_at: Option<u64>,
    /// When `status` last changed, milliseconds since the epoch (#897).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_since: Option<u64>,
    /// When the server marked the agent silent, milliseconds since the epoch: `working` without a
    /// report for `agents.silentAfter` minutes (#898). Set only by the server's sweeper, cleared
    /// by the next report; never present for another status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub silent_since: Option<u64>,
}

impl crate::Terminal {
    /// The kind of the hook agent in the terminal (#1314): `None` for a shell, also one whose
    /// only status is a program's own report (OSC 7501).
    pub fn hook_agent_kind(&self) -> Option<&str> {
        let kind = match self.agent_source.as_deref() {
            Some("osc7501") => self.hook_agent.as_deref(),
            _ => self.agent.as_ref().and_then(|a| a.agent.as_deref()),
        };
        kind.filter(|k| !k.is_empty())
    }
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

    #[test]
    fn hook_agent_is_additive_and_tells_a_hook_agent_from_a_program_status() {
        let terminal = |extra: serde_json::Value| -> Terminal {
            let mut v = json!({
                "id":"t1","space_id":"s1","name":"x","cwd":"/","status":"running",
                "cols":80,"rows":24
            });
            v.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            serde_json::from_value(v).unwrap()
        };
        let program = json!({"status":"working","agent":"deliver"});
        let shell = terminal(json!({"agent": program, "agent_source":"osc7501"}));
        assert_eq!(shell.hook_agent, None);
        assert_eq!(shell.hook_agent_kind(), None);
        let behind =
            terminal(json!({"agent": program, "agent_source":"osc7501", "hook_agent":"claude"}));
        assert_eq!(behind.hook_agent_kind(), Some("claude"));
        assert_eq!(
            serde_json::to_value(&behind).unwrap()["hook_agent"],
            "claude"
        );
        let hooks = terminal(json!({
            "agent": {"status":"idle","agent":"copilot"}, "agent_source":"copilot"
        }));
        assert_eq!(hooks.hook_agent_kind(), Some("copilot"));
        // A server before `agent_source`: its agent is the hooks'.
        let old = terminal(json!({"agent": {"status":"idle","agent":"claude"}}));
        assert_eq!(old.hook_agent_kind(), Some("claude"));
        assert_eq!(terminal(json!({})).hook_agent_kind(), None);
    }
}
