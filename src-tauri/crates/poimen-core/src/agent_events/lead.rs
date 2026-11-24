//! The state of a lead run (ADR 0018, #1212) as the Runs view shows it (#1213): what the
//! `orchestrate@1` records of a run file say about its members, dependencies and grant. Read-only
//! and derived: the file stays the only store. The core reader folds the records here and ignores
//! everything else of the profile.

use serde::Serialize;
use serde_json::Value;

use poimen_protocol::lead::{LEAD_PROFILE, LeadRecord};

/// At most this many members, dependencies and decisions are kept (a lead writes few; the cap
/// bounds a runaway file).
const MAX_ITEMS: usize = 200;

/// A started loop of the lead.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeadMember {
    pub task: String,
    pub repository: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deliver_run: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub machine: Option<String>,
}

/// `task` waits for `blocked_by`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeadDependency {
    pub task: String,
    pub blocked_by: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// The grant the lead recorded (ids and limits only).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeadGrant {
    pub id: String,
    pub spaces: Vec<String>,
    pub tools: Vec<String>,
    pub max_agents: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires: Option<String>,
}

/// What the `orchestrate@1` records of one run file say, newest wins where it replaces.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LeadState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<Value>,
    pub members: Vec<LeadMember>,
    pub dependencies: Vec<LeadDependency>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant: Option<LeadGrant>,
    /// The terminal of the newest `orchestrate.resume`: the lead's terminal after a takeover.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumed_in: Option<String>,
}

impl LeadState {
    /// Folds one event; `true` when it was a lead record.
    pub fn push(&mut self, event: &Value) -> bool {
        if event.get("profile").and_then(Value::as_str) != Some(LEAD_PROFILE) {
            return false;
        }
        let Ok(record) = serde_json::from_value::<LeadRecord>(event.clone()) else {
            return false;
        };
        match record {
            LeadRecord::Plan { plan } => self.plan = Some(plan),
            LeadRecord::Member {
                task,
                repository,
                space,
                terminal,
                deliver_run,
                machine,
            } => {
                let member = LeadMember {
                    task,
                    repository,
                    space,
                    terminal,
                    deliver_run,
                    machine,
                };
                match self.members.iter().position(|m| m.task == member.task) {
                    Some(at) => self.members[at] = member,
                    None if self.members.len() < MAX_ITEMS => self.members.push(member),
                    None => {}
                }
            }
            LeadRecord::Dependency {
                task,
                blocked_by,
                note,
            } => {
                let known = self
                    .dependencies
                    .iter()
                    .any(|d| d.task == task && d.blocked_by == blocked_by);
                if !known && self.dependencies.len() < MAX_ITEMS {
                    self.dependencies.push(LeadDependency {
                        task,
                        blocked_by,
                        note,
                    });
                }
            }
            LeadRecord::Grant {
                grant,
                spaces,
                tools,
                max_agents,
                expires,
            } => {
                self.grant = Some(LeadGrant {
                    id: grant,
                    spaces,
                    tools,
                    max_agents,
                    expires,
                });
            }
            LeadRecord::Resume { terminal } => self.resumed_in = Some(terminal),
            LeadRecord::Decision { .. } => {}
        }
        true
    }
}
