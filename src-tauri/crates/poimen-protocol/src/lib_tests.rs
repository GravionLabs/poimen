#[test]
fn memory_bytes_of_a_terminal_is_optional() {
    let base = json!({"id":"t1","space_id":"s1","name":"zsh","cwd":"/","cols":80,"rows":24,"status":"running"});
    let old: Terminal = serde_json::from_value(base.clone()).unwrap();
    assert_eq!(old.memory_bytes, None);
    assert_eq!(serde_json::to_value(&old).unwrap(), base);
    let mut with = base;
    with["memory_bytes"] = json!(1_048_576u64);
    let t: Terminal = serde_json::from_value(with.clone()).unwrap();
    assert_eq!(t.memory_bytes, Some(1_048_576));
    assert_eq!(serde_json::to_value(&t).unwrap(), with);
}

#[test]
fn agent_info_times_are_additive() {
    let old: AgentInfo = serde_json::from_value(json!({"status":"idle"})).unwrap();
    assert_eq!((old.reported_at, old.status_since), (None, None));
    assert_eq!(
        serde_json::to_value(&old).unwrap(),
        json!({"status":"idle"})
    );
    let v =
        json!({"status":"working","reported_at":1700000005000u64,"status_since":1700000000000u64});
    let info: AgentInfo = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(info.reported_at, Some(1_700_000_005_000));
    assert_eq!(info.status_since, Some(1_700_000_000_000));
    assert_eq!(serde_json::to_value(&info).unwrap(), v);
}

#[test]
fn env_of_the_create_requests_is_optional_and_round_trips() {
    let old: TerminalCreate = serde_json::from_str(r#"{"space_id":"s1"}"#).unwrap();
    assert!(old.env.is_empty());
    assert_eq!(serde_json::to_value(&old).unwrap().get("env"), None);
    let with: TerminalCreate =
        serde_json::from_str(r#"{"space_id":"s1","env":{"A":"1"}}"#).unwrap();
    assert_eq!(with.env["A"], "1");
    let split: TabSplit =
        serde_json::from_str(r#"{"terminal_id":"t1","direction":"right","env":{"A":"1"}}"#)
            .unwrap();
    assert_eq!(split.env["A"], "1");
    let space: SpaceCreate =
        serde_json::from_str(r#"{"name":"x","cwd":"/","env":{"A":"1"}}"#).unwrap();
    assert_eq!(space.env["A"], "1");
    let apply: SpaceApplyLayout =
        serde_json::from_str(r#"{"space_id":"s1","env":{"A":"1"}}"#).unwrap();
    assert_eq!(apply.env["A"], "1");
    let tab: TabCreate = serde_json::from_str(r#"{"space_id":"s1"}"#).unwrap();
    assert!(tab.env.is_empty());
}

use super::*;
use serde_json::json;

#[path = "lib_tests_more.rs"]
mod more;

#[test]
fn subscribe_params_with_and_without_client() {
    let r = parse_request(
        r#"{"id":"1","method":"events.subscribe","params":{"client":{"kind":"tui","pid":7}}}"#,
    )
    .unwrap();
    let p: EventsSubscribe = params(&r).unwrap();
    assert_eq!(
        p.client,
        Some(ClientInfo {
            kind: ClientKind::Tui,
            pid: 7
        })
    );
    let r = parse_request(r#"{"id":"1","method":"events.subscribe"}"#).unwrap();
    let p: EventsSubscribe = params(&r).unwrap();
    assert_eq!(p.client, None);
    let r = parse_request(
        r#"{"id":"1","method":"events.subscribe","params":{"client":{"kind":"bogus","pid":1}}}"#,
    )
    .unwrap();
    // A kind of a newer client is read as unknown, not refused.
    let p: EventsSubscribe = params(&r).unwrap();
    assert_eq!(p.client.unwrap().kind, ClientKind::Unknown);
}

#[test]
fn subscribe_asks_for_run_events_only_when_set() {
    let r = parse_request(
        r#"{"id":"1","method":"events.subscribe","params":{"client":{"kind":"app","pid":7},"run_events":true}}"#,
    )
    .unwrap();
    let p: EventsSubscribe = params(&r).unwrap();
    assert!(p.run_events);
    let r = parse_request(r#"{"id":"1","method":"events.subscribe","params":{}}"#).unwrap();
    assert!(!params::<EventsSubscribe>(&r).unwrap().run_events);
    // Off is not sent, so an older server sees the params it knows.
    assert_eq!(
        serde_json::to_value(EventsSubscribe::default()).unwrap(),
        json!({})
    );
}

#[test]
fn agent_confirmation_wire_shapes() {
    let c: AgentConfirm = serde_json::from_value(json!({
        "caller_terminal_id": "t1", "action": "prompt_agent", "space_id": "s2",
        "terminal_id": "t9", "text": "go"
    }))
    .unwrap();
    assert_eq!((c.action, c.wait_ms), (AgentWriteAction::PromptAgent, None));
    let shell: AgentConfirm = serde_json::from_value(json!({
        "caller_terminal_id": "t1", "action": "start_shell", "space_id": "s2", "text": "node x"
    }))
    .unwrap();
    assert_eq!(shell.action, AgentWriteAction::StartShell);
    assert_eq!(
        serde_json::to_value(ConfirmResult {
            approved: false,
            reason: ConfirmReason::NoApp
        })
        .unwrap(),
        json!({"approved": false, "reason": "no_app"})
    );
    let line = r#"{"event":"agent.confirm_requested","data":{"id":"c1","caller_terminal_id":"t1","caller_name":"lead","action":"start_agent","space_id":"s2","space_name":"api","text":"claude","wait_ms":60000}}"#;
    let ServerLine::Event(e) = parse_server_line(line).unwrap() else {
        panic!("not an event");
    };
    assert!(matches!(
        e,
        Event::AgentConfirmRequested {
            terminal_id: None,
            ..
        }
    ));
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        serde_json::from_str::<Value>(line).unwrap()
    );
    let wrote: Event = serde_json::from_value(json!({"event":"agent.wrote","data":{
        "caller_terminal_id":"t1","caller_name":"lead","action":"prompt_agent",
        "space_id":"s1","space_name":"web","terminal_id":"t2","text":"hi","confirmed":false}}))
    .unwrap();
    assert!(matches!(
        wrote,
        Event::AgentWrote {
            confirmed: false,
            ..
        }
    ));
    let answer: AgentConfirmAnswer =
        serde_json::from_value(json!({"id": "c1", "approve": true})).unwrap();
    assert!(answer.approve);
}

#[test]
fn run_events_wire_shape() {
    let line = r#"{"event":"run.events","data":{"root":"/home/me/api","run":"r1","events":[{"v":1,"seq":1,"type":"run.started"}]}}"#;
    let ServerLine::Event(e) = parse_server_line(line).unwrap() else {
        panic!("not an event");
    };
    assert_eq!(
        e,
        Event::RunEvents {
            root: "/home/me/api".into(),
            run: "r1".into(),
            events: vec![json!({"v":1,"seq":1,"type":"run.started"})],
        }
    );
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        serde_json::from_str::<Value>(line).unwrap()
    );
}

#[test]
fn ping_result_without_clients_and_the_clients_event() {
    let p: PingResult =
        serde_json::from_value(serde_json::json!({"version":"1","protocol":4,"pid":2})).unwrap();
    assert!(p.clients.is_empty());
    let e: Event = serde_json::from_value(
        json!({"event":"server.clients_changed","data":{"clients":["app","unknown"]}}),
    )
    .unwrap();
    assert_eq!(
        e,
        Event::ServerClientsChanged {
            clients: vec![ClientKind::App, ClientKind::Unknown]
        }
    );
}

#[test]
fn parses_a_request_with_and_without_params() {
    let r = parse_request(r#"{"id":"1","method":"space.create","params":{"name":"a","cwd":"/x"}}"#)
        .unwrap();
    assert_eq!(r.method, "space.create");
    let p: SpaceCreate = params(&r).unwrap();
    assert_eq!(p.cwd, "/x");
    assert!(
        p.layout,
        "space.create builds the layout unless told not to"
    );
    assert_eq!(p.trust, None);

    let r = parse_request(r#"{"id":"2","method":"ping"}"#).unwrap();
    assert_eq!(r.params, Value::Null);
}

#[test]
fn rejects_garbage_with_parse_error_and_no_id() {
    let e = parse_request("not json").unwrap_err();
    assert_eq!(e.id, None);
    assert_eq!(e.error.unwrap().code, ErrorCode::ParseError);
}

#[test]
fn rejects_a_request_without_method_but_keeps_its_id() {
    let e = parse_request(r#"{"id":"7"}"#).unwrap_err();
    assert_eq!(e.id.as_deref(), Some("7"));
    assert_eq!(e.error.unwrap().code, ErrorCode::InvalidRequest);
}

#[test]
fn wrong_params_are_invalid_params() {
    let r = parse_request(r#"{"id":"3","method":"terminal.resize","params":{"terminal_id":"t"}}"#)
        .unwrap();
    let e = params::<TerminalResize>(&r).unwrap_err();
    assert_eq!(e.id.as_deref(), Some("3"));
    assert_eq!(e.error.unwrap().code, ErrorCode::InvalidParams);
}

#[test]
fn optional_params_default() {
    let r = parse_request(r#"{"id":"4","method":"terminal.list"}"#).unwrap();
    assert_eq!(params::<TerminalList>(&r).unwrap().space_id, None);
}

#[test]
fn event_wire_shape() {
    let e = Event::TerminalExited {
        terminal_id: "t1".into(),
        exit_code: Some(3),
    };
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        json!({"event":"terminal.exited","data":{"terminal_id":"t1","exit_code":3}})
    );
    let status: Event = serde_json::from_value(
        json!({"event":"agent.status_changed","data":{"terminal_id":"t1","status":"blocked"}}),
    )
    .unwrap();
    assert_eq!(
        status,
        Event::AgentStatusChanged {
            terminal_id: "t1".into(),
            status: AgentStatus::Blocked,
            agent: None,
            session_id: None,
            message: None,
            message_at: None,
            progress: None,
            kind: None,
            tone: None,
            title: None,
            silent_since: None,
        }
    );
}

#[test]
fn silent_since_is_additive() {
    let old: AgentInfo = serde_json::from_value(json!({"status":"working"})).unwrap();
    assert_eq!(old.silent_since, None);
    let info: AgentInfo =
        serde_json::from_value(json!({"status":"working","silent_since":1700000000000u64}))
            .unwrap();
    assert_eq!(info.silent_since, Some(1_700_000_000_000));
    assert_eq!(
        serde_json::to_value(&info).unwrap()["silent_since"],
        1_700_000_000_000u64
    );
    let event: Event = serde_json::from_value(json!({"event":"agent.status_changed",
        "data":{"terminal_id":"t1","status":"working","silent_since":5}}))
    .unwrap();
    assert!(matches!(
        event,
        Event::AgentStatusChanged {
            silent_since: Some(5),
            ..
        }
    ));
}

#[test]
fn error_state_progress_kind_and_title_are_additive() {
    let old: AgentInfo = serde_json::from_value(serde_json::json!({"status":"blocked"})).unwrap();
    assert_eq!((old.progress, &old.kind, &old.title), (None, &None, &None));
    assert_eq!(
        serde_json::to_value(&old).unwrap(),
        json!({"status":"blocked"})
    );
    let info = AgentInfo {
        reported_at: None,
        status_since: None,
        silent_since: None,
        status: AgentStatus::Error,
        agent: None,
        session_id: None,
        message: None,
        message_at: None,
        progress: Some(40),
        kind: Some("permission".into()),
        tone: None,
        title: Some("build".into()),
    };
    let value = serde_json::to_value(&info).unwrap();
    assert_eq!(
        value,
        json!({"status":"error","progress":40,"kind":"permission","title":"build"})
    );
    assert_eq!(serde_json::from_value::<AgentInfo>(value).unwrap(), info);
    let event: Event = serde_json::from_value(serde_json::json!({"event":"agent.status_changed",
        "data":{"terminal_id":"t1","status":"error","progress":7}}))
    .unwrap();
    assert!(matches!(
        event,
        Event::AgentStatusChanged {
            status: AgentStatus::Error,
            progress: Some(7),
            ..
        }
    ));
}

#[test]
fn record_ids_follow_the_segment_rules() {
    let eight = ["a"; 8].join("/");
    for ok in ["a", "run/issue-12", "a.b_c+d-e", &eight] {
        assert!(valid_record_id(ok), "{ok}");
    }
    let too_deep = ["a"; 9].join("/");
    let seg33 = "a".repeat(33);
    let long = vec!["a".repeat(32); 5].join("/");
    for bad in [
        "", "/", "a/", "/a", "a//b", "a b", "a=b", "a,b", &too_deep, &seg33, &long,
    ] {
        assert!(!valid_record_id(bad), "{bad}");
    }
}

#[test]
fn the_records_event_and_field_round_trip() {
    let event: Event = serde_json::from_str(
        r#"{"event":"terminal.records_changed","data":{"terminal_id":"t1","records":[{"id":"run/a","status":"working","progress":3}]}}"#,
    )
    .unwrap();
    let Event::TerminalRecordsChanged { records, .. } = &event else {
        panic!("{event:?}");
    };
    assert_eq!(records[0].progress, Some(3));
    assert_eq!(records[0].app, None);
}

#[test]
fn the_resume_failed_event_round_trips() {
    let text =
        r#"{"event":"terminal.resume_failed","data":{"terminal_id":"t1","reason":"no report"}}"#;
    let event: Event = serde_json::from_str(text).unwrap();
    assert_eq!(
        event,
        Event::TerminalResumeFailed {
            terminal_id: "t1".into(),
            reason: "no report".into()
        }
    );
    assert_eq!(serde_json::to_string(&event).unwrap(), text);
}

#[test]
fn clean_message_removes_bidi_overrides_and_invisible_characters() {
    assert_eq!(
        clean_message("a\u{202E}b\u{200B}c\u{2066}d\u{FEFF}").as_deref(),
        Some("abcd")
    );
    assert_eq!(clean_message("\u{202E}\u{200B}"), None);
}

#[test]
fn clean_message_strips_controls_collapses_and_truncates() {
    assert_eq!(
        clean_message("  Allow\u{1b}[31m Bash?\n\tnow\u{0} ").as_deref(),
        Some("Allow [31m Bash? now")
    );
    assert_eq!(clean_message("\n\t \u{7}"), None);
    assert_eq!(clean_message(""), None);
    let long = "ab ".repeat(200);
    let cut = clean_message(&long).unwrap();
    assert_eq!(cut.chars().count(), 299);
    assert!(cut.ends_with('b'));
    assert_eq!(
        clean_message(&"é".repeat(400)).unwrap().chars().count(),
        300
    );
}

#[test]
fn set_colors_params_have_optional_cursor_and_palette() {
    let p: SetColors =
        serde_json::from_value(serde_json::json!({"foreground":"#ffffff","background":"#000000"}))
            .unwrap();
    assert_eq!((p.cursor, p.palette), (None, None));
}

#[test]
fn token_wire_shapes_and_names() {
    let p: TerminalReportTokens = serde_json::from_value(serde_json::json!({"terminal_id":"t1",
        "tokens":{"deliver_step":"build","deliver_pr":null},"ttl_s":30,"source":"deliver"}))
    .unwrap();
    assert_eq!(p.tokens["deliver_step"].as_deref(), Some("build"));
    assert_eq!(p.tokens["deliver_pr"], None);
    assert_eq!(p.ttl_s, Some(30));
    let p: TerminalReportTokens =
        serde_json::from_value(serde_json::json!({"terminal_id":"t1","tokens":{},"source":"s"}))
            .unwrap();
    assert_eq!(p.ttl_s, None);
    assert!(
        serde_json::from_value::<TerminalReportTokens>(
            json!({"terminal_id":"t1","tokens":{"a":1},"source":"s"})
        )
        .is_err()
    );
    let c: TerminalClearTokens =
        serde_json::from_value(serde_json::json!({"terminal_id":"t1","source":"deliver"})).unwrap();
    assert_eq!(c.source, "deliver");
    let t: Terminal =
        serde_json::from_value(serde_json::json!({"id":"t1","space_id":"s1","name":"n",
        "cwd":"/","cols":80,"rows":24,"status":"running","tokens":{"deliver":"on"}}))
        .unwrap();
    assert_eq!(t.tokens["deliver"], "on");
    let event: Event = serde_json::from_str(
        r#"{"event":"terminal.tokens_changed","data":{"terminal_id":"t1","tokens":{"a":"b"}}}"#,
    )
    .unwrap();
    assert_eq!(
        event,
        Event::TerminalTokensChanged {
            terminal_id: "t1".into(),
            tokens: [("a".to_owned(), "b".to_owned())].into()
        }
    );
    assert!(valid_token_name("deliver_step-2"));
    assert!(valid_token_name(&"a".repeat(32)));
    assert!(!valid_token_name(""));
    assert!(!valid_token_name(&"a".repeat(33)));
    assert!(!valid_token_name("a b"));
    assert!(!valid_token_name("é"));
    assert!(!valid_token_name("a.b"));
}

#[test]
fn attach_wire_shapes() {
    let p: TerminalAttach = serde_json::from_value(
        serde_json::json!({"terminal_id":"t1","mode":"control","take_over":true}),
    )
    .unwrap();
    assert_eq!(p.mode, AttachMode::Control);
    assert!(p.take_over);
    let p: TerminalAttach =
        serde_json::from_value(serde_json::json!({"terminal_id":"t1","mode":"observe"})).unwrap();
    assert!(!p.take_over);
    assert!(
        serde_json::from_value::<TerminalAttach>(json!({"terminal_id":"t1","mode":"x"})).is_err()
    );
    let d: TerminalDetach =
        serde_json::from_value(serde_json::json!({"terminal_id":"t1"})).unwrap();
    assert_eq!(d.terminal_id, "t1");
    assert!(serde_json::from_value::<TerminalDetach>(json!({})).is_err());
    let d: TerminalSetDraft =
        serde_json::from_value(serde_json::json!({"terminal_id":"t1","draft":true})).unwrap();
    assert_eq!((d.terminal_id.as_str(), d.draft), ("t1", true));
    assert!(serde_json::from_value::<TerminalSetDraft>(json!({"terminal_id":"t1"})).is_err());

    let e = Event::TerminalDetached {
        terminal_id: "t1".into(),
        reason: DetachReason::TakenOver,
    };
    assert_eq!(
        serde_json::to_value(&e).unwrap(),
        json!({"event":"terminal.detached","data":{"terminal_id":"t1","reason":"taken_over"}})
    );
    let line = r#"{"event":"terminal.output","data":{"terminal_id":"t1","seq":3,"data":"aGk="}}"#;
    match parse_server_line(line).unwrap() {
        ServerLine::Event(Event::TerminalOutput { seq, data, .. }) => {
            assert_eq!((seq, decode_bytes(&data).unwrap()), (3, b"hi".to_vec()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_protocol_3_space_has_no_tabs_and_a_protocol_4_space_lists_them() {
    let old: Space =
        serde_json::from_value(serde_json::json!({"id":"s1","name":"a","cwd":"/x"})).unwrap();
    assert_eq!((old.tabs.len(), old.active_tab), (0, None));
    let space: Space = serde_json::from_value(
        json!({"id":"s1","name":"a","cwd":"/x","tabs":["tab2","tab3"],"active_tab":"tab3"}),
    )
    .unwrap();
    assert_eq!(space.tabs, ["tab2", "tab3"]);
    assert_eq!(space.active_tab.as_deref(), Some("tab3"));
    assert_eq!(
        serde_json::to_value(Space {
            id: "s1".into(),
            name: "a".into(),
            cwd: "/x".into(),
            ..Default::default()
        })
        .unwrap(),
        json!({"id":"s1","name":"a","cwd":"/x","tabs":[]})
    );
}

fn tree() -> Value {
    json!({"kind":"split","split_id":"split5","direction":"right","ratio":0.25,"children":[
        {"kind":"terminal","terminal_id":"t1"},
        {"kind":"split","split_id":"split7","direction":"down","ratio":0.5,"children":[
            {"kind":"terminal","terminal_id":"t4"},
            {"kind":"terminal","terminal_id":"t6"}
        ]}
    ]})
}

#[test]
fn a_tab_and_its_split_tree_round_trip() {
    let wire = json!({"tab_id":"tab2","space_id":"s1","name":"dev","root":tree(),
        "active_terminal":"t4"});
    let tab: Tab = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(tab.root.terminal_ids(), ["t1", "t4", "t6"]);
    assert_eq!(tab.root.split_ids(), ["split5", "split7"]);
    assert_eq!(tab.root.first_terminal(), "t1");
    assert!(tab.root.contains_terminal("t6"));
    assert!(!tab.root.contains_terminal("t9"));
    match &tab.root {
        Node::Split {
            direction, ratio, ..
        } => assert_eq!((*direction, *ratio), (SplitDirection::Right, 0.25)),
        other => panic!("{other:?}"),
    }
    assert_eq!(serde_json::to_value(&tab).unwrap(), wire);
    let leaf: Node =
        serde_json::from_value(serde_json::json!({"kind":"terminal","terminal_id":"t1"})).unwrap();
    assert_eq!(leaf, Node::terminal("t1"));
}

#[test]
fn an_ended_terminal_from_history_is_additive() {
    let base = json!({"id":"t1","space_id":"s1","name":"zsh","cwd":"/","cols":80,"rows":24,"status":"exited"});
    let old: Terminal = serde_json::from_value(base.clone()).unwrap();
    assert!(!old.from_history);
    assert_eq!(serde_json::to_value(&old).unwrap(), base);
    let mut with = base;
    with["from_history"] = json!(true);
    let t: Terminal = serde_json::from_value(with.clone()).unwrap();
    assert!(t.from_history);
    assert_eq!(serde_json::to_value(&t).unwrap(), with);

    let attach =
        json!({"terminal_id":"t1","seq":0,"cols":80,"rows":24,"mode":"observe","snapshot":""});
    let old: AttachResult = serde_json::from_value(attach.clone()).unwrap();
    assert!(!old.read_only);
    assert_eq!(serde_json::to_value(&old).unwrap(), attach);
    let mut with = attach;
    with["read_only"] = json!(true);
    let r: AttachResult = serde_json::from_value(with.clone()).unwrap();
    assert!(r.read_only);
    assert_eq!(serde_json::to_value(&r).unwrap(), with);
}

#[test]
fn an_ended_terminal_with_a_session_to_resume_is_from_history_and_hibernated() {
    // #1232: no new field; the pair `from_history` + `hibernated` says "ended, can resume".
    let wire = json!({"id":"t1","space_id":"s1","name":"claude","cwd":"/w","cols":80,"rows":24,
        "status":"exited","hibernated":{"agent":"claude","session_id":"abc","since":5},
        "from_history":true});
    let t: Terminal = serde_json::from_value(wire.clone()).unwrap();
    assert!(t.from_history);
    let h = t.hibernated.as_ref().unwrap();
    assert_eq!(
        (h.agent.as_str(), h.session_id.as_str(), h.since),
        ("claude", "abc", 5)
    );
    assert_eq!(serde_json::to_value(&t).unwrap(), wire);
}
