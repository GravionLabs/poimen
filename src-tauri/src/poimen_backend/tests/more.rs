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

#[test]
fn a_prompt_draft_is_told_to_the_server_on_the_stream_of_its_terminal() {
    let (backend, log) = backend_for("draft", vec![], false);
    let (opened, _rx) = collect(&backend, Mode::Observe, false);
    opened.unwrap();
    backend.terminal_draft("t1", true).unwrap();
    backend.terminal_draft("t1", false).unwrap();
    // Other terminals have no stream here: nothing is sent for them.
    backend.terminal_draft("t2", true).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while lock(&log)
        .iter()
        .filter(|l| l.starts_with("terminal.set_draft"))
        .count()
        < 2
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the draft was never sent"
        );
        std::thread::yield_now();
    }
    let sent = lock(&log)
        .iter()
        .filter(|l| l.starts_with("terminal.set_draft"))
        .map(|l| serde_json::from_str::<Value>(&l["terminal.set_draft ".len()..]).unwrap())
        .map(|p| {
            (
                p["terminal_id"].as_str().unwrap().to_owned(),
                p["draft"].as_bool().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        sent,
        vec![("t1".to_owned(), true), ("t1".to_owned(), false)]
    );
}

#[test]
fn a_stream_opened_while_a_draft_is_held_tells_the_server_again() {
    let (backend, log) = backend_for("draft-again", vec![], false);
    backend.terminal_draft("t1", true).unwrap();
    let (opened, _rx) = collect(&backend, Mode::Observe, false);
    opened.unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !lock(&log)
        .iter()
        .any(|l| l.starts_with("terminal.set_draft"))
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the draft was never sent"
        );
        std::thread::yield_now();
    }
    assert_eq!(params_of(&log, "terminal.set_draft")["draft"], true);
}

#[test]
fn a_held_controller_is_reported_as_a_closed_record_not_an_error() {
    let (backend, _) = backend_for("held", vec![], true);
    let (opened, rx) = collect(&backend, Mode::Control, false);
    assert!(opened.is_ok());
    let (_, record) = rx.recv().unwrap();
    assert!(matches!(
        record,
        StreamRecord::Closed {
            code: Some(CloseCode::ControllerHeld),
            ..
        }
    ));
}
