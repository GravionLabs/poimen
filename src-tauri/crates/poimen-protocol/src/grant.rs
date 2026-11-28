//! Wire types of the scoped write grant (ADR 0018, #1211): a lead agent asks once for a plan over
//! several spaces, the user confirms it in the app, and the writes inside it need no dialog.

use serde::{Deserialize, Serialize};

use crate::{AgentWriteAction, ConfirmReason};

/// How long a grant lasts when the request names no duration: 8 hours.
pub const GRANT_DEFAULT_SECS: u64 = 8 * 3600;
/// The longest duration of a grant: 24 hours. There is no grant without an end.
pub const GRANT_MAX_SECS: u64 = 24 * 3600;
/// How long `agent.grant_request` waits for the user at most.
pub const GRANT_WAIT_MS: u64 = 120_000;

/// Params of `agent.grant_request`: the lead agent (the terminal `caller_terminal_id`) asks for a
/// grant. The server asks the connected app and answers when the user did, or after `wait_ms`
/// (default and cap [`GRANT_WAIT_MS`]); a timeout counts as "no".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentGrantRequest {
    pub caller_terminal_id: String,
    /// What the lead plans to do, shown to the user as is.
    pub plan: String,
    /// Space ids on the machine of this server; the server resolves names and paths.
    pub spaces: Vec<String>,
    /// A subset of the write tools; never `write_terminal`.
    pub tools: Vec<AgentWriteAction>,
    /// The most agents the lead may have running at once across the granted spaces.
    pub max_agents: u32,
    /// Also asks to start agents in new worktrees of the granted spaces' repositories (ADR 0018,
    /// amendment #1448); shown as its own line in the dialog. Absent is `false`.
    #[serde(default)]
    pub worktrees: bool,
    /// Seconds; default [`GRANT_DEFAULT_SECS`], at most [`GRANT_MAX_SECS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_s: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
}

/// A space of a grant, as the user sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantSpace {
    pub id: String,
    pub name: String,
    /// The directory of the space, when it has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// An active grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantInfo {
    pub id: String,
    /// The terminal the grant belongs to; only it may use the grant.
    pub lead_terminal_id: String,
    pub lead_name: String,
    pub spaces: Vec<GrantSpace>,
    pub tools: Vec<AgentWriteAction>,
    pub max_agents: u32,
    /// Agents the lead started that are still running (not `done`, `error` or `gone`).
    pub running: u32,
    /// Whether the grant may cover worktree starts (#1448); `false` when absent.
    #[serde(default)]
    pub worktrees: bool,
    /// Milliseconds since the epoch.
    pub expires_at: u64,
}

/// Result of `agent.grant_request`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantResult {
    pub granted: bool,
    pub reason: ConfirmReason,
    /// The grant as the user confirmed it (it can be narrower than asked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<GrantInfo>,
}

/// Params of `agent.grant_answer`: the app's answer to an [`crate::Event::AgentGrantRequested`].
/// The user can only deny or narrow: `spaces` must be a subset of the asked ones, `max_agents`
/// not above the asked limit and `worktrees` not `true` when it was not asked; anything wider is
/// refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentGrantAnswer {
    pub id: String,
    pub approve: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spaces: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_agents: Option<u32>,
    /// `Some(false)` turns the asked worktree line off; absent keeps what was asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktrees: Option<bool>,
}

/// Params of `agent.grant_revoke`: by grant `id` (the app), or the grant of the caller's own
/// terminal when only `caller_terminal_id` is set (the lead at its end).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentGrantRevoke {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caller_terminal_id: Option<String>,
}

/// Result of `agent.grants`, and `data` of [`crate::Event::AgentGrantsChanged`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AgentGrants {
    pub grants: Vec<GrantInfo>,
}

/// Params of `agent.grant_check`: `poimen mcp` sends each write it is about to make.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentGrantCheck {
    pub caller_terminal_id: String,
    pub action: AgentWriteAction,
    /// The server-resolved space id of the write.
    pub space_id: String,
    /// The grant id the caller's lead run records (#1454); not trusted: it can only turn a
    /// question into a refusal (`lost`), never a write into a covered one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant: Option<String>,
}

/// Why a grant ended (#1454). A grant replaced by a later grant of the same lead counts as
/// `revoked`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantEndReason {
    Revoked,
    Expired,
    LeadGone,
}

impl GrantEndReason {
    /// The name on the wire.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Revoked => "revoked",
            Self::Expired => "expired",
            Self::LeadGone => "lead_gone",
        }
    }
}

/// What the server says about a write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum GrantVerdict {
    /// A grant of the caller covers the write; no dialog.
    Covered { grant: String },
    /// No grant covers it: the per-call confirmation of ADR 0012 applies.
    NeedsConfirmation,
    /// A grant covers it but a limit stops it; do not ask.
    Refused { reason: String },
    /// No grant covers it and the `grant` of the check ended for a known reason (#1454): the
    /// per-call confirmation applies, and its answer names the reason.
    Ended {
        grant: String,
        reason: GrantEndReason,
    },
    /// No grant covers it and the server knows nothing of the `grant` of the check (its state
    /// was not restored, #1454): refuse with `grant_lost`, do not ask.
    Lost { grant: String },
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::{Event, ServerLine, parse_server_line};

    #[test]
    fn a_request_has_defaults_and_refuses_unknown_tools() {
        let r: AgentGrantRequest = serde_json::from_value(json!({
            "caller_terminal_id": "t1", "plan": "three repos", "spaces": ["s1", "s2"],
            "tools": ["start_agent", "prompt_agent"], "max_agents": 3
        }))
        .unwrap();
        assert_eq!((r.duration_s, r.wait_ms, r.worktrees), (None, None, false));
        assert_eq!(
            r.tools,
            [AgentWriteAction::StartAgent, AgentWriteAction::PromptAgent]
        );
        assert!(
            serde_json::from_value::<AgentGrantRequest>(json!({
                "caller_terminal_id": "t1", "plan": "x", "spaces": [], "tools": ["rm_rf"],
                "max_agents": 1
            }))
            .is_err()
        );
    }

    #[test]
    fn answer_revoke_and_check_shapes() {
        let a: AgentGrantAnswer =
            serde_json::from_value(json!({"id": "g1", "approve": true})).unwrap();
        assert_eq!((a.spaces, a.max_agents, a.worktrees), (None, None, None));
        let off: AgentGrantAnswer =
            serde_json::from_value(json!({"id": "g1", "approve": true, "worktrees": false}))
                .unwrap();
        assert_eq!(off.worktrees, Some(false));
        assert_eq!(
            serde_json::to_value(&off).unwrap(),
            json!({"id": "g1", "approve": true, "worktrees": false})
        );
        let narrowed: AgentGrantAnswer = serde_json::from_value(
            json!({"id": "g1", "approve": true, "spaces": ["s1"], "max_agents": 2}),
        )
        .unwrap();
        assert_eq!(narrowed.spaces, Some(vec!["s1".to_owned()]));
        let by_id: AgentGrantRevoke = serde_json::from_value(json!({"id": "g1"})).unwrap();
        assert_eq!(by_id.caller_terminal_id, None);
        let own: AgentGrantRevoke =
            serde_json::from_value(json!({"caller_terminal_id": "t1"})).unwrap();
        assert_eq!(own.id, None);
        let c: AgentGrantCheck = serde_json::from_value(
            json!({"caller_terminal_id": "t1", "action": "start_agent", "space_id": "s2"}),
        )
        .unwrap();
        assert_eq!((c.action, c.grant), (AgentWriteAction::StartAgent, None));
        let held: AgentGrantCheck = serde_json::from_value(json!({"caller_terminal_id": "t1",
            "action": "prompt_agent", "space_id": "s2", "grant": "g1"}))
        .unwrap();
        assert_eq!(held.grant.as_deref(), Some("g1"));
        assert_eq!(serde_json::to_value(&held).unwrap()["grant"], "g1");
        assert!(
            serde_json::to_value(AgentGrantCheck {
                grant: None,
                ..held
            })
            .unwrap()
            .get("grant")
            .is_none(),
            "an older server never sees the field"
        );
    }

    #[test]
    fn verdicts_are_tagged() {
        for (value, verdict) in [
            (
                json!({"verdict": "covered", "grant": "g1"}),
                GrantVerdict::Covered { grant: "g1".into() },
            ),
            (
                json!({"verdict": "needs_confirmation"}),
                GrantVerdict::NeedsConfirmation,
            ),
            (
                json!({"verdict": "refused", "reason": "limit"}),
                GrantVerdict::Refused {
                    reason: "limit".into(),
                },
            ),
            (
                json!({"verdict": "ended", "grant": "g1", "reason": "revoked"}),
                GrantVerdict::Ended {
                    grant: "g1".into(),
                    reason: GrantEndReason::Revoked,
                },
            ),
            (
                json!({"verdict": "ended", "grant": "g1", "reason": "expired"}),
                GrantVerdict::Ended {
                    grant: "g1".into(),
                    reason: GrantEndReason::Expired,
                },
            ),
            (
                json!({"verdict": "ended", "grant": "g1", "reason": "lead_gone"}),
                GrantVerdict::Ended {
                    grant: "g1".into(),
                    reason: GrantEndReason::LeadGone,
                },
            ),
            (
                json!({"verdict": "lost", "grant": "g1"}),
                GrantVerdict::Lost { grant: "g1".into() },
            ),
        ] {
            assert_eq!(
                serde_json::from_value::<GrantVerdict>(value.clone()).unwrap(),
                verdict
            );
            assert_eq!(serde_json::to_value(&verdict).unwrap(), value);
        }
        for reason in [
            GrantEndReason::Revoked,
            GrantEndReason::Expired,
            GrantEndReason::LeadGone,
        ] {
            assert_eq!(serde_json::to_value(reason).unwrap(), reason.as_str());
        }
        assert!(
            serde_json::from_value::<GrantVerdict>(
                json!({"verdict": "ended", "grant": "g1", "reason": "forgotten"})
            )
            .is_err()
        );
    }

    #[test]
    fn grant_events_and_results_round_trip() {
        let info = json!({"id": "g1", "lead_terminal_id": "t1", "lead_name": "lead",
            "spaces": [{"id": "s1", "name": "api", "path": "/w/api"}, {"id": "s2", "name": "web"}],
            "tools": ["start_agent"], "max_agents": 2, "running": 1, "worktrees": true,
            "expires_at": 1_700_000_000_000_u64});
        let grant: GrantInfo = serde_json::from_value(info.clone()).unwrap();
        assert_eq!(grant.spaces[1].path, None);
        assert_eq!(serde_json::to_value(&grant).unwrap(), info);
        let result: GrantResult =
            serde_json::from_value(json!({"granted": false, "reason": "timeout"})).unwrap();
        assert_eq!(
            (result.granted, result.reason, result.grant),
            (false, ConfirmReason::Timeout, None)
        );
        for line in [
            r#"{"event":"agent.grant_requested","data":{"id":"g1","lead_terminal_id":"t1","lead_name":"lead","plan":"p","spaces":[{"id":"s1","name":"api"}],"tools":["start_agent"],"max_agents":2,"worktrees":true,"duration_s":28800,"wait_ms":120000}}"#,
            r#"{"event":"agent.grant_resolved","data":{"id":"g1"}}"#,
            r#"{"event":"agent.grants_changed","data":{"grants":[]}}"#,
        ] {
            let ServerLine::Event(e) = parse_server_line(line).unwrap() else {
                panic!("not an event");
            };
            assert!(matches!(
                e,
                Event::AgentGrantRequested { .. }
                    | Event::AgentGrantResolved { .. }
                    | Event::AgentGrantsChanged { .. }
            ));
            assert_eq!(
                serde_json::to_value(&e).unwrap(),
                serde_json::from_str::<Value>(line).unwrap()
            );
        }
    }

    #[test]
    fn worktrees_default_to_false_and_round_trip() {
        let asked: AgentGrantRequest = serde_json::from_value(json!({
            "caller_terminal_id": "t1", "plan": "p", "spaces": ["s1"], "tools": ["start_agent"],
            "max_agents": 1, "worktrees": true
        }))
        .unwrap();
        assert!(asked.worktrees);
        assert_eq!(serde_json::to_value(&asked).unwrap()["worktrees"], true);
        assert!(
            serde_json::from_value::<AgentGrantRequest>(json!({
                "caller_terminal_id": "t1", "plan": "p", "spaces": ["s1"], "tools": ["start_agent"],
                "max_agents": 1, "worktrees": "yes"
            }))
            .is_err(),
            "only a boolean"
        );
        // An event and a grant of an older server have no `worktrees`: they cover none.
        let ServerLine::Event(Event::AgentGrantRequested { worktrees, .. }) = parse_server_line(
            r#"{"event":"agent.grant_requested","data":{"id":"g1","lead_terminal_id":"t1","lead_name":"lead","plan":"p","spaces":[],"tools":["start_agent"],"max_agents":1,"duration_s":60,"wait_ms":1000}}"#,
        )
        .unwrap() else {
            panic!("not a grant request");
        };
        assert!(!worktrees);
        let old: GrantInfo = serde_json::from_value(json!({"id": "g1", "lead_terminal_id": "t1",
            "lead_name": "lead", "spaces": [], "tools": [], "max_agents": 1, "running": 0,
            "expires_at": 1}))
        .unwrap();
        assert!(!old.worktrees);
    }
}
