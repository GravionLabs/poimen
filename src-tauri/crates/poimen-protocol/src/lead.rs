//! The records of a lead run (ADR 0018, #1212): events of the profile `orchestrate@1` that a lead
//! agent appends to its agent-events run file, next to the core events the Runs view shows. The
//! core reader ignores the types; a new lead session reads them to resume. One JSON object per
//! line, the envelope (`v`, `seq`, `ts`, `run`, `profile`) is added by the writer. No secret is
//! ever recorded.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The `profile` of these events.
pub const LEAD_PROFILE: &str = "orchestrate@1";
/// The prefix of a lead run id: `lead-<uuid>` (before #1417 `lead-<secs>`).
pub const LEAD_RUN_PREFIX: &str = "lead-";

/// A record of a lead run, without its envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum LeadRecord {
    /// The approved plan: repositories with path, milestone, order and blockers across them. Its
    /// shape is the lead's; the newest record replaces the older ones.
    #[serde(rename = "orchestrate.plan")]
    Plan { plan: Value },
    /// A started loop: the repository, its space and terminal, the run id of its deliver loop (`deliver_run`: `run` is the envelope's) and
    /// the machine (`None`: the machine of the lead).
    #[serde(rename = "orchestrate.member")]
    Member {
        task: String,
        repository: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        space: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        terminal: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        deliver_run: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        machine: Option<String>,
    },
    /// A dependency across repositories: `task` waits for `blocked_by`.
    #[serde(rename = "orchestrate.dependency")]
    Dependency {
        task: String,
        blocked_by: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        note: Option<String>,
    },
    /// The grant the user confirmed (ADR 0018): ids and limits only.
    #[serde(rename = "orchestrate.grant")]
    Grant {
        grant: String,
        spaces: Vec<String>,
        tools: Vec<String>,
        max_agents: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires: Option<String>,
    },
    /// What the lead answered to a blocked session, one line.
    #[serde(rename = "orchestrate.decision")]
    Decision {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        task: Option<String>,
    },
    /// A new lead session in `terminal` took the run over after the user's confirmation.
    #[serde(rename = "orchestrate.resume")]
    Resume { terminal: String },
}

impl LeadRecord {
    /// The record of one line of a run file; `None` for a core event, another profile or a line
    /// that does not parse.
    pub fn from_line(line: &str) -> Option<Self> {
        let v: Value = serde_json::from_str(line).ok()?;
        if v.get("profile").and_then(Value::as_str) != Some(LEAD_PROFILE) {
            return None;
        }
        serde_json::from_value(v).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_member_line_parses_with_its_envelope_and_unknown_fields() {
        let line = json!({"v": 1, "seq": 4, "run": "lead-1", "profile": LEAD_PROFILE,
            "type": "orchestrate.member", "task": "poimen#1", "repository": "/r",
            "terminal": "t1", "deliver_run": "deliver-1", "future": true})
        .to_string();
        assert_eq!(
            LeadRecord::from_line(&line),
            Some(LeadRecord::Member {
                task: "poimen#1".into(),
                repository: "/r".into(),
                space: None,
                terminal: Some("t1".into()),
                deliver_run: Some("deliver-1".into()),
                machine: None,
            })
        );
    }

    #[test]
    fn every_record_round_trips() {
        let records = [
            LeadRecord::Plan {
                plan: json!({"repositories": []}),
            },
            LeadRecord::Dependency {
                task: "a#1".into(),
                blocked_by: "b#2".into(),
                note: None,
            },
            LeadRecord::Grant {
                grant: "g1".into(),
                spaces: vec!["s1".into()],
                tools: vec!["start_agent".into()],
                max_agents: 3,
                expires: Some("2026-11-24T20:00:00Z".into()),
            },
            LeadRecord::Decision {
                text: "use B".into(),
                task: Some("a#1".into()),
            },
            LeadRecord::Resume {
                terminal: "t2".into(),
            },
        ];
        for r in records {
            let mut v = serde_json::to_value(&r).unwrap();
            v["profile"] = json!(LEAD_PROFILE);
            assert_eq!(LeadRecord::from_line(&v.to_string()), Some(r));
        }
    }

    #[test]
    fn core_events_other_profiles_and_broken_lines_are_not_records() {
        assert_eq!(
            LeadRecord::from_line(r#"{"type":"progress","done":1}"#),
            None
        );
        let other = r#"{"profile":"deliver@1","type":"orchestrate.resume","terminal":"t"}"#;
        assert_eq!(LeadRecord::from_line(other), None);
        let bad = format!(r#"{{"profile":"{LEAD_PROFILE}","type":"orchestrate.member"}}"#);
        assert_eq!(LeadRecord::from_line(&bad), None);
        assert_eq!(LeadRecord::from_line("{half"), None);
    }
}
