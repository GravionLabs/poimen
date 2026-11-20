//! Wire shape of `terminal.report_agent` and of the `agent` of a terminal.

use poimen_protocol::{AgentStatus, Terminal, TerminalReportAgent};
use serde_json::json;

#[test]
fn report_agent_params_and_the_terminal_agent_wire_shape() {
    let p: TerminalReportAgent =
        serde_json::from_value(json!({"terminal_id":"t1","seq":5,"status":"gone"})).unwrap();
    assert_eq!(p.status, AgentStatus::Gone);
    assert_eq!((p.agent, p.session_id, p.message), (None, None, None));
    let p: TerminalReportAgent = serde_json::from_value(
        json!({"terminal_id":"t1","seq":6,"status":"blocked","message":"Allow Bash?"}),
    )
    .unwrap();
    assert_eq!(p.message.as_deref(), Some("Allow Bash?"));
    assert!(!p.message_fallback);
    // A stand-in text (#984) is on the wire only when set.
    let p: TerminalReportAgent = serde_json::from_value(
        json!({"terminal_id":"t1","seq":7,"status":"blocked","message":"Bash: ls","message_fallback":true}),
    )
    .unwrap();
    assert!(p.message_fallback);
    assert_eq!(serde_json::to_value(&p).unwrap()["message_fallback"], true);
    let plain: TerminalReportAgent =
        serde_json::from_value(json!({"terminal_id":"t1","seq":8,"status":"error"})).unwrap();
    assert_eq!(plain.status, AgentStatus::Error);
    assert!(
        serde_json::to_value(&plain)
            .unwrap()
            .get("message_fallback")
            .is_none()
    );
    let t: Terminal = serde_json::from_value(json!({"id":"t1","space_id":"s1","name":"n",
        "cwd":"/","cols":80,"rows":24,"status":"running",
        "agent":{"status":"working","agent":"claude","session_id":"abc"}}))
    .unwrap();
    assert_eq!(t.agent.unwrap().session_id.as_deref(), Some("abc"));
}
