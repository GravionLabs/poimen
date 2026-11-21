//! The optional `tone` of an agent (OSC 7501 `tone=warning`, #1152) is additive on the wire.

use poimen_protocol::{AgentInfo, ChildRecord, Event};
use serde_json::json;

#[test]
fn tone_is_additive_on_the_agent_the_event_and_the_records() {
    let old: AgentInfo = serde_json::from_value(json!({"status":"done"})).unwrap();
    assert_eq!(old.tone, None);
    assert!(serde_json::to_value(&old).unwrap().get("tone").is_none());
    let agent: AgentInfo =
        serde_json::from_value(json!({"status":"done","tone":"warning"})).unwrap();
    assert_eq!(agent.tone.as_deref(), Some("warning"));
    let event: Event = serde_json::from_value(json!({"event":"agent.status_changed",
        "data":{"terminal_id":"t1","status":"done","tone":"warning"}}))
    .unwrap();
    assert!(matches!(event, Event::AgentStatusChanged { tone: Some(t), .. } if t == "warning"));
    let record: ChildRecord =
        serde_json::from_value(json!({"id":"run/a","status":"done","tone":"warning"})).unwrap();
    assert_eq!(record.tone.as_deref(), Some("warning"));
}
