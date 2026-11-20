//! Wire shapes of events and fields, parsed and written back.

use poimen_protocol::{Event, Terminal};

#[test]
fn the_awake_event_round_trips() {
    let event: Event =
        serde_json::from_str(r#"{"event":"server.awake_changed","data":{"awake":true}}"#).unwrap();
    assert_eq!(event, Event::ServerAwakeChanged { awake: true });
    assert_eq!(
        serde_json::to_string(&event).unwrap(),
        r#"{"event":"server.awake_changed","data":{"awake":true}}"#
    );
}

#[test]
fn the_hibernated_event_and_field_round_trip() {
    let event: Event = serde_json::from_str(
        r#"{"event":"terminal.hibernated_changed","data":{"terminal_id":"t1","hibernated":{"agent":"claude","session_id":"s1","since":5}}}"#,
    )
    .unwrap();
    let Event::TerminalHibernatedChanged { hibernated, .. } = &event else {
        panic!("{event:?}");
    };
    assert_eq!(hibernated.as_ref().unwrap().session_id, "s1");
    let cleared: Event = serde_json::from_str(
        r#"{"event":"terminal.hibernated_changed","data":{"terminal_id":"t1"}}"#,
    )
    .unwrap();
    assert!(matches!(
        cleared,
        Event::TerminalHibernatedChanged {
            hibernated: None,
            ..
        }
    ));
    let old: Terminal = serde_json::from_str(
        r#"{"id":"t1","space_id":"s","name":"n","cwd":"/","cols":1,"rows":1,"status":"running"}"#,
    )
    .unwrap();
    assert_eq!(old.hibernated, None);
}

#[test]
fn started_by_is_optional_and_round_trips() {
    use poimen_protocol::TerminalCreate;
    let old: TerminalCreate = serde_json::from_str(r#"{"space_id":"s1"}"#).unwrap();
    assert_eq!(old.started_by, None);
    assert_eq!(serde_json::to_value(&old).unwrap().get("started_by"), None);
    let with: TerminalCreate =
        serde_json::from_str(r#"{"space_id":"s1","started_by":"t1"}"#).unwrap();
    assert_eq!(with.started_by.as_deref(), Some("t1"));
    let t: Terminal = serde_json::from_value(serde_json::json!({
        "id":"t2","space_id":"s1","name":"n","cwd":"/","cols":80,"rows":24,
        "status":"running","started_by":"t1"
    }))
    .unwrap();
    assert_eq!(t.started_by.as_deref(), Some("t1"));
    assert_eq!(serde_json::to_value(&t).unwrap()["started_by"], "t1");
}
