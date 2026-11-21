use super::*;
use poimen_client::protocol::AgentInfo;

#[test]
fn error_progress_kind_and_title_reach_the_pane_and_its_event() {
    let event = backend_event(&Event::AgentStatusChanged {
        terminal_id: "t1".into(),
        status: AgentStatus::Error,
        agent: None,
        session_id: None,
        message: None,
        message_at: None,
        progress: Some(40),
        kind: Some("auth".into()),
        tone: None,
        title: Some("build".into()),
        silent_since: None,
    });
    assert_eq!(
        event.data,
        json!({"terminal_id": "t1", "agent_status": "error", "agent_progress": 40,
            "agent_kind": "auth", "agent_title": "build"})
    );
    let mut t = terminal("t1", "s1", "a");
    t.agent = Some(AgentInfo {
        reported_at: None,
        status_since: None,
        silent_since: None,
        status: AgentStatus::Error,
        agent: None,
        session_id: None,
        message: None,
        message_at: None,
        progress: Some(40),
        kind: Some("auth".into()),
        tone: None,
        title: Some("build".into()),
    });
    let snapshot = snap(&[space("s1", "api")], &[t, terminal("t2", "s1", "b")]);
    let pane = &snapshot["terminals"][0];
    assert_eq!(pane["agent_status"], "error");
    assert_eq!(pane["agent_progress"], 40);
    assert_eq!(pane["agent_kind"], "auth");
    assert_eq!(pane["agent_title"], "build");
    assert_eq!(snapshot["spaces"][0]["agent_status"], "error");
    assert!(snapshot["terminals"][1].get("agent_progress").is_none());
}
