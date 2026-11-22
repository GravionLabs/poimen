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
fn a_split_needs_exactly_two_children_and_a_known_kind_and_direction() {
    let leaf = json!({"kind":"terminal","terminal_id":"t1"});
    let split = |children: Value, direction: &str| {
        json!({"kind":"split","split_id":"split1","direction":direction,"ratio":0.5,
            "children":children})
    };
    assert!(serde_json::from_value::<Node>(split(json!([leaf, leaf]), "down")).is_ok());
    assert!(serde_json::from_value::<Node>(split(json!([leaf]), "down")).is_err());
    assert!(serde_json::from_value::<Node>(split(json!([leaf, leaf, leaf]), "down")).is_err());
    assert!(serde_json::from_value::<Node>(split(json!([leaf, leaf]), "left")).is_err());
    assert!(serde_json::from_value::<Node>(json!({"kind":"pane","terminal_id":"t1"})).is_err());
    assert!(serde_json::from_value::<Node>(json!({"kind":"split","split_id":"x"})).is_err());
}

#[test]
fn ratios_are_strictly_between_0_and_1() {
    assert!(valid_ratio(0.5));
    assert!(valid_ratio(0.01));
    assert!(!valid_ratio(0.0));
    assert!(!valid_ratio(1.0));
    assert!(!valid_ratio(-0.2));
    assert!(!valid_ratio(f64::NAN));
    assert!(!valid_ratio(f64::INFINITY));
}

#[test]
fn the_space_list_carries_tabs_and_defaults_them_for_protocol_3() {
    let list: SpaceList =
        serde_json::from_value(serde_json::json!({"spaces":[{"id":"s1","name":"a",
        "cwd":"/x","tabs":["tab2"],"active_tab":"tab2"}],"tabs":[{"tab_id":"tab2",
        "space_id":"s1","name":"zsh","root":{"kind":"terminal","terminal_id":"t3"},
        "active_terminal":"t3"}]}))
        .unwrap();
    assert_eq!(list.tabs[0].root, Node::terminal("t3"));
    let old: SpaceList =
        serde_json::from_value(serde_json::json!({"spaces":[{"id":"s1","name":"a","cwd":"/x"}]}))
            .unwrap();
    assert!(old.tabs.is_empty());
}

#[test]
fn tab_request_params() {
    let p: TabCreate = serde_json::from_value(serde_json::json!({"space_id":"s1"})).unwrap();
    assert_eq!((p.name, p.terminal_id), (None, None));
    let p: TabCreate = serde_json::from_value(
        serde_json::json!({"space_id":"s1","name":"logs","terminal_id":"t2"}),
    )
    .unwrap();
    assert_eq!(
        (p.name.as_deref(), p.terminal_id.as_deref()),
        (Some("logs"), Some("t2"))
    );
    assert!(serde_json::from_value::<TabCreate>(json!({})).is_err());

    let p: TabRename =
        serde_json::from_value(serde_json::json!({"tab_id":"tab2","name":"x"})).unwrap();
    assert_eq!(p.name, "x");
    let p: TabClose = serde_json::from_value(serde_json::json!({"tab_id":"tab2"})).unwrap();
    assert_eq!(p.tab_id, "tab2");
    let p: TabMove =
        serde_json::from_value(serde_json::json!({"tab_id":"tab2","index":0})).unwrap();
    assert_eq!(p.index, 0);
    assert!(serde_json::from_value::<TabMove>(json!({"tab_id":"tab2","index":-1})).is_err());

    let p: TabSplit =
        serde_json::from_value(serde_json::json!({"terminal_id":"t1","direction":"down"})).unwrap();
    assert_eq!(p.direction, SplitDirection::Down);
    assert_eq!(
        (p.ratio, p.cwd, p.name, p.command),
        (None, None, None, None)
    );
    let p: TabSplit =
        serde_json::from_value(serde_json::json!({"terminal_id":"t1","direction":"right",
        "ratio":0.3,"cwd":"/w","name":"logs","command":["tail","-f","x"]}))
        .unwrap();
    assert_eq!(p.ratio, Some(0.3));
    assert_eq!(p.command.unwrap(), ["tail", "-f", "x"]);
    assert!(serde_json::from_value::<TabSplit>(json!({"terminal_id":"t1"})).is_err());

    let p: TabResizeSplit = serde_json::from_value(
        serde_json::json!({"tab_id":"tab2","split_id":"split3","ratio":0.7}),
    )
    .unwrap();
    assert_eq!((p.split_id.as_str(), p.ratio), ("split3", 0.7));
    assert!(
        serde_json::from_value::<TabResizeSplit>(json!({"tab_id":"tab2","ratio":0.7})).is_err()
    );

    let p: TabFocus = serde_json::from_value(serde_json::json!({"tab_id":"tab2"})).unwrap();
    assert_eq!(p.terminal_id, None);
    let p: TabFocus =
        serde_json::from_value(serde_json::json!({"tab_id":"tab2","terminal_id":"t4"})).unwrap();
    assert_eq!(p.terminal_id.as_deref(), Some("t4"));
}

#[test]
fn tab_event_wire_shapes() {
    let tab = json!({"tab_id":"tab2","space_id":"s1","name":"dev","root":tree(),
        "active_terminal":"t1"});
    for name in ["tab.created", "tab.changed"] {
        let event: Event =
            serde_json::from_value(serde_json::json!({"event":name,"data":{"tab":tab}})).unwrap();
        let parsed = match &event {
            Event::TabCreated { tab } | Event::TabChanged { tab } => tab,
            other => panic!("{other:?}"),
        };
        assert_eq!(parsed.tab_id, "tab2");
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"event":name,"data":{"tab":tab}})
        );
    }
    let closed = Event::TabClosed {
        tab_id: "tab2".into(),
        space_id: "s1".into(),
    };
    assert_eq!(
        serde_json::to_value(&closed).unwrap(),
        json!({"event":"tab.closed","data":{"tab_id":"tab2","space_id":"s1"}})
    );
    let changed: Event =
        serde_json::from_value(serde_json::json!({"event":"space.changed","data":{
        "space":{"id":"s1","name":"a","cwd":"/x","tabs":["tab3","tab2"],"active_tab":"tab2"}}}))
        .unwrap();
    match changed {
        Event::SpaceChanged { space } => assert_eq!(space.tabs, ["tab3", "tab2"]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn bytes_round_trip_including_non_utf8() {
    let raw = [0u8, 0xff, 0x1b, b'[', 0xc3];
    assert_eq!(decode_bytes(&encode_bytes(&raw)).unwrap(), raw);
    assert!(decode_bytes("***").is_err());
}

#[test]
fn server_lines_are_told_apart() {
    let line = r#"{"event":"space.closed","data":{"space_id":"s1"}}"#;
    assert!(matches!(
        parse_server_line(line).unwrap(),
        ServerLine::Event(Event::SpaceClosed { .. })
    ));
    let line = r#"{"id":"1","result":{"protocol":1}}"#;
    assert!(matches!(
        parse_server_line(line).unwrap(),
        ServerLine::Response(_)
    ));
    let line = r#"{"id":"1","error":{"code":"not_found","message":"x"}}"#;
    match parse_server_line(line).unwrap() {
        ServerLine::Response(r) => assert_eq!(r.error.unwrap().code, ErrorCode::NotFound),
        other => panic!("{other:?}"),
    }
}

fn unix_env(socket: Option<&str>, runtime: Option<&str>, user: Option<&str>) -> SocketEnv {
    SocketEnv {
        socket: socket.map(Into::into),
        base: runtime.map(Into::into),
        user: user.map(Into::into),
        temp_dir: PathBuf::from("/tmp"),
        windows: false,
    }
}

#[test]
fn socket_path_precedence() {
    let stable = Channel::Stable;
    assert_eq!(
        unix_env(Some("/a.sock"), Some("/run/user/1"), None).path(stable),
        PathBuf::from("/a.sock")
    );
    assert_eq!(
        unix_env(Some(""), Some("/run/user/1"), None).path(stable),
        PathBuf::from("/run/user/1/poimen/server.sock")
    );
    assert_eq!(
        unix_env(None, None, Some("jk")).path(stable),
        PathBuf::from("/tmp/poimen-jk/server.sock")
    );
}

#[test]
fn machine_check_result_parses_and_ok_ignores_warnings() {
    let r: MachineCheckResult = serde_json::from_value(serde_json::json!({
        "version": "0.18.0", "protocol": 6,
        "checks": [
            {"id": "git", "status": "ok"},
            {"id": "copilot", "status": "warning", "detail": "x", "fix": "copilot"}
        ]
    }))
    .unwrap();
    assert_eq!(r.checks[1].status, CheckStatus::Warning);
    assert_eq!(r.checks[1].fix.as_deref(), Some("copilot"));
    assert_eq!(r.checks[0].detail, None);
    assert!(r.ok());
    let bad: MachineCheckResult = serde_json::from_value(serde_json::json!({
        "version": "1", "protocol": 6, "checks": [{"id": "gh", "status": "missing"}]
    }))
    .unwrap();
    assert!(!bad.ok());
    assert!(
        serde_json::from_value::<MachineCheckItem>(json!({"id": "x", "status": "bogus"})).is_err()
    );
}

#[test]
fn worktree_cleanup_parses() {
    let p: WorktreeCleanup = serde_json::from_value(serde_json::json!({
        "owner": "o", "repo": "app",
        "merged": [{"branch": "feat/7-x", "number": 12}]
    }))
    .unwrap();
    assert_eq!(p.merged[0].number, 12);
    assert_eq!(p.focused_space, None);
    assert!(!p.skip_teardown);
    assert!(
        serde_json::from_value::<WorktreeCleanup>(json!({"owner": "o", "repo": "app"})).is_err()
    );
    assert!(
        serde_json::from_value::<WorktreeCleanup>(json!({
            "owner": "o", "repo": "app", "merged": [{"branch": "b"}]
        }))
        .is_err()
    );
    let r: WorktreeCleanupResult = serde_json::from_value(serde_json::json!({
        "path": "/s/o/app",
        "removed": [{"path": "/s/o/app-7", "branch": "feat/7-x", "pr": 12, "branch_deleted": true}],
        "left": [{"path": "/s/o/app-8", "branch": "feat/8-y", "pr": 13,
                  "reason": "dirty", "command": "git worktree remove x"}],
        "teardown_untrusted": false
    }))
    .unwrap();
    assert_eq!(r.removed[0].note, None);
    assert!(r.removed[0].branch_deleted);
    assert_eq!(r.left[0].reason, "dirty");
    let none: WorktreeCleanupResult =
        serde_json::from_value(serde_json::json!({"path": null})).unwrap();
    assert_eq!(none, WorktreeCleanupResult::default());
}

#[test]
fn agent_transcript_request_and_result_wire_shapes() {
    let p: AgentTranscript =
        serde_json::from_value(serde_json::json!({"terminal_id": "t1", "follower": "f1"})).unwrap();
    assert!(!p.restart);
    assert_eq!(
        serde_json::to_value(AgentTranscript { restart: true, ..p }).unwrap(),
        json!({"terminal_id": "t1", "follower": "f1", "restart": true})
    );
    assert!(serde_json::from_value::<AgentTranscript>(json!({"terminal_id": "t1"})).is_err());
    let r: AgentTranscriptResult = serde_json::from_value(serde_json::json!({
        "state": "ok", "session": "s1", "fresh": true,
        "updates": [{"kind": "main", "calls": [], "tools": 0, "usage": []}]
    }))
    .unwrap();
    assert_eq!(
        (r.state, r.fresh, r.updates.len()),
        (TranscriptState::Ok, true, 1)
    );
    let r: AgentTranscriptResult =
        serde_json::from_value(serde_json::json!({"state": "unreadable", "reason": "denied"}))
            .unwrap();
    assert_eq!(r.state, TranscriptState::Unreadable);
    assert_eq!(r.reason.as_deref(), Some("denied"));
    assert!(r.updates.is_empty() && !r.fresh);
    assert_eq!(
        serde_json::to_value(TranscriptState::NoAgent).unwrap(),
        json!("no_agent")
    );
}

#[test]
fn repo_and_worktree_requests_parse() {
    let p: RepoEnsure =
        serde_json::from_value(serde_json::json!({"owner": "GravionLabs", "repo": "poimen"}))
            .unwrap();
    assert_eq!(
        (p.owner.as_str(), p.repo.as_str()),
        ("GravionLabs", "poimen")
    );
    assert!(serde_json::from_value::<RepoEnsure>(json!({"owner": "o"})).is_err());
    let r: RepoEnsureResult =
        serde_json::from_value(serde_json::json!({"path": "/s/o/r", "cloned": true})).unwrap();
    assert!(r.cloned);

    let w: WorktreeCreate = serde_json::from_value(serde_json::json!({
        "repo_dir": "/s/o/r", "issue": {"nr": 7, "title": "feat: X"}, "mode": "agent"
    }))
    .unwrap();
    assert_eq!(w.issue.nr, 7);
    assert_eq!(w.issue.url, "");
    assert!(
        serde_json::from_value::<WorktreeCreate>(json!({
            "repo_dir": "/r", "issue": {"title": "x"}, "mode": "agent"
        }))
        .is_err()
    );
    let r: WorktreeCreateResult =
        serde_json::from_value(json!({"path": "/s/o/r-7", "branch": "feat/7-x", "created": false}))
            .unwrap();
    assert_eq!(r.branch, "feat/7-x");
    assert!(!r.created);
}

#[test]
fn the_rc_channel_has_its_own_socket() {
    assert_eq!(
        unix_env(None, Some("/run/user/1"), None).path(Channel::Rc),
        PathBuf::from("/run/user/1/poimen-rc/server.sock")
    );
    assert_eq!(
        unix_env(None, None, Some("jk")).path(Channel::Rc),
        PathBuf::from("/tmp/poimen-rc-jk/server.sock")
    );
    assert_eq!(
        unix_env(None, None, None).path(Channel::Rc),
        PathBuf::from("/tmp/poimen-rc-user/server.sock")
    );
}

#[test]
fn the_default_socket_of_the_other_channel_is_foreign_and_ignored() {
    // A stable app started from a terminal of the rc server, and the other way round.
    let rc_terminal = unix_env(
        Some("/run/user/1/poimen-rc/server.sock"),
        Some("/run/user/1"),
        None,
    );
    assert!(rc_terminal.is_foreign(Channel::Stable));
    assert!(!rc_terminal.is_foreign(Channel::Rc));
    assert_eq!(
        rc_terminal.path(Channel::Stable),
        PathBuf::from("/run/user/1/poimen/server.sock")
    );
    assert_eq!(
        rc_terminal.path(Channel::Rc),
        PathBuf::from("/run/user/1/poimen-rc/server.sock")
    );
    let stable_terminal = unix_env(Some("/tmp/poimen-jk/server.sock"), None, Some("jk"));
    assert!(stable_terminal.is_foreign(Channel::Rc));
    assert!(!stable_terminal.is_foreign(Channel::Stable));
    // Any other path is an explicit choice, honored by both.
    let custom = unix_env(Some("/a.sock"), Some("/run/user/1"), None);
    assert!(!custom.is_foreign(Channel::Stable));
    assert!(!custom.is_foreign(Channel::Rc));
    assert_eq!(custom.path(Channel::Rc), PathBuf::from("/a.sock"));
    assert!(!unix_env(None, Some("/run/user/1"), None).is_foreign(Channel::Rc));
}

fn windows_env(socket: Option<&str>, local: Option<&str>, user: Option<&str>) -> SocketEnv {
    SocketEnv {
        socket: socket.map(Into::into),
        base: local.map(Into::into),
        user: user.map(Into::into),
        temp_dir: PathBuf::from(r"C:\Temp"),
        windows: true,
    }
}

#[test]
fn windows_socket_path_precedence() {
    let tmp = PathBuf::from(r"C:\Temp");
    let local = PathBuf::from(r"C:\Users\u\AppData\Local");
    let local_str = r"C:\Users\u\AppData\Local";
    let stable = Channel::Stable;
    assert_eq!(
        windows_env(Some(r"D:\x.sock"), Some(local_str), None).path(stable),
        PathBuf::from(r"D:\x.sock")
    );
    assert_eq!(
        windows_env(Some(""), Some(local_str), None).path(stable),
        local.join("poimen-server").join("server.sock")
    );
    assert_eq!(
        windows_env(None, Some(""), Some("jk")).path(stable),
        tmp.join("poimen-jk").join("server.sock")
    );
    assert_eq!(
        windows_env(None, None, None).path(stable),
        tmp.join("poimen-user").join("server.sock")
    );
    assert_eq!(
        windows_env(None, Some(local_str), None).path(Channel::Rc),
        local.join("poimen-server-rc").join("server.sock")
    );
    assert_eq!(
        windows_env(None, None, Some("jk")).path(Channel::Rc),
        tmp.join("poimen-rc-jk").join("server.sock")
    );
    let stable_terminal = windows_env(
        Some(
            &local
                .join("poimen-server")
                .join("server.sock")
                .to_string_lossy(),
        ),
        Some(local_str),
        None,
    );
    assert!(stable_terminal.is_foreign(Channel::Rc));
    assert_eq!(
        stable_terminal.path(Channel::Rc),
        local.join("poimen-server-rc").join("server.sock")
    );
}

#[test]
fn the_windows_dir_needs_local_app_data() {
    assert_eq!(
        windows_dir_from(Some(r"C:\L".into()), Channel::Stable),
        Some(PathBuf::from(r"C:\L").join("poimen-server"))
    );
    assert_eq!(
        windows_dir_from(Some(r"C:\L".into()), Channel::Rc),
        Some(PathBuf::from(r"C:\L").join("poimen-server-rc"))
    );
    assert_eq!(windows_dir_from(Some("".into()), Channel::Stable), None);
    assert_eq!(windows_dir_from(None, Channel::Rc), None);
}

#[test]
fn channel_values_parse_and_name_things() {
    assert_eq!(Channel::parse(None), Some(Channel::Stable));
    assert_eq!(Channel::parse(Some("")), Some(Channel::Stable));
    assert_eq!(Channel::parse(Some("stable")), Some(Channel::Stable));
    assert_eq!(Channel::parse(Some("rc")), Some(Channel::Rc));
    assert_eq!(Channel::parse(Some("RC")), None);
    assert_eq!(Channel::parse(Some("beta")), None);
    assert_eq!(Channel::parse(Some("rcx")), None);
    assert_eq!(Channel::Stable.named("poimen"), "poimen");
    assert_eq!(Channel::Rc.named("poimen"), "poimen-rc");
    assert_eq!(Channel::Stable.server_program(false), "poimen-server");
    assert_eq!(Channel::Rc.server_program(false), "poimen-server-rc");
    assert_eq!(Channel::Rc.server_program(true), "poimen-server-rc.exe");
    assert_eq!(Channel::Stable.host_program(false), "poimen-pty-host");
    assert_eq!(Channel::Stable.host_program(true), "poimen-pty-host.exe");
    assert_eq!(Channel::Rc.host_program(true), "poimen-pty-host-rc.exe");
    assert_eq!(Channel::Rc.name(), "rc");
    assert_eq!(serde_json::to_value(Channel::Rc).unwrap(), "rc");
    assert_eq!(serde_json::to_value(Channel::Stable).unwrap(), "stable");
}

#[test]
fn space_create_params_carry_the_layout_flag_trust_answer_and_command() {
    let p: SpaceCreate = serde_json::from_value(serde_json::json!({
        "name": "a", "cwd": "/x", "layout": false,
        "trust": {"kind": "trust", "hash": "ab12"}, "command": "claude"
    }))
    .unwrap();
    assert!(!p.layout);
    assert_eq!(
        p.trust,
        Some(TrustAnswer::Trust {
            hash: "ab12".into()
        })
    );
    assert_eq!(p.command.as_deref(), Some("claude"));
    let without: TrustAnswer =
        serde_json::from_value(serde_json::json!({"kind": "without"})).unwrap();
    assert_eq!(without, TrustAnswer::Without);
    assert!(serde_json::from_value::<TrustAnswer>(json!({"kind": "maybe"})).is_err());
    assert_eq!(
        serde_json::to_value(SpaceCreate::empty("a", "/x")).unwrap(),
        json!({"name": "a", "cwd": "/x", "layout": false})
    );
    assert!(SpaceCreate::with_layout("a", "/x").layout);
}

#[test]
fn space_create_result_is_the_trust_question_or_the_space() {
    let asked: SpaceCreateResult = serde_json::from_value(serde_json::json!({"trust_required": {
        "root": "/r", "files": ["/r/.poimen/config.json"], "hash": "ab12",
        "commands": [{"what": "setup", "command": "make"}],
        "layout": [{"name": "agent", "command": "claude"},
                   {"name": "sh", "split": {"from": "agent", "direction": "down", "ratio": 0.5}}]
    }}))
    .unwrap();
    let SpaceCreateResult::TrustRequired { trust_required } = asked else {
        panic!("{asked:?}")
    };
    assert_eq!(trust_required.hash, "ab12");
    assert_eq!(trust_required.commands[0].command, "make");
    let layout = trust_required.layout.unwrap();
    assert_eq!(layout[0].split, None);
    assert_eq!(
        layout[1].split,
        Some(LayoutSplit {
            from: "agent".into(),
            direction: SplitDirection::Down,
            ratio: 0.5
        })
    );

    let created: SpaceCreateResult = serde_json::from_value(serde_json::json!({
        "space": {"id": "s1", "name": "a", "cwd": "/x", "tabs": ["tab2"], "active_tab": "tab2"},
        "tabs": [{"tab_id": "tab2", "space_id": "s1", "name": "agent",
                  "root": {"kind": "terminal", "terminal_id": "t3"}, "active_terminal": "t3"}],
        "warnings": ["w"], "setup_started": true
    }))
    .unwrap();
    let SpaceCreateResult::Created(created) = created else {
        panic!("{created:?}")
    };
    assert_eq!(created.space.id, "s1");
    assert_eq!(created.tabs[0].root, Node::terminal("t3"));
    assert_eq!(created.warnings, ["w"]);
    assert!(created.setup_started);

    // A protocol 4 server answers only `{space}`.
    let old: SpaceCreateResult = serde_json::from_value(
        serde_json::json!({"space": {"id": "s1", "name": "a", "cwd": "/x"}}),
    )
    .unwrap();
    assert!(matches!(old, SpaceCreateResult::Created(c) if c.tabs.is_empty() && !c.setup_started));
}

#[test]
fn space_apply_layout_params_carry_space_cwd_preset_and_trust() {
    let r = parse_request(
        r#"{"id":"1","method":"space.apply_layout","params":{"space_id":"s1","cwd":"/x",
            "preset":"duo","trust":{"kind":"without"}}}"#,
    )
    .unwrap();
    assert_eq!(r.method, "space.apply_layout");
    let p: SpaceApplyLayout = params(&r).unwrap();
    assert_eq!(p.space_id, "s1");
    assert_eq!(p.cwd.as_deref(), Some("/x"));
    assert_eq!(p.preset.as_deref(), Some("duo"));
    assert_eq!(p.trust, Some(TrustAnswer::Without));

    // Only `space_id` is required; the defaults stay off the wire.
    let bare: SpaceApplyLayout =
        serde_json::from_value(serde_json::json!({"space_id": "s2"})).unwrap();
    assert_eq!(bare, SpaceApplyLayout::resolved("s2"));
    assert_eq!(
        serde_json::to_value(SpaceApplyLayout::preset("s2", "duo")).unwrap(),
        json!({"space_id": "s2", "preset": "duo"})
    );
    assert!(serde_json::from_value::<SpaceApplyLayout>(json!({"preset": "duo"})).is_err());
}

#[test]
fn space_apply_layout_result_is_the_trust_question_or_the_new_tabs() {
    let asked: SpaceApplyLayoutResult =
        serde_json::from_value(serde_json::json!({"trust_required": {
            "root": "/x", "files": ["/x/.poimen/config.json"], "hash": "h",
            "commands": [{"what": "preset \"duo\": pane \"a\"", "command": "vim"}],
            "layout": [{"name": "a", "command": "vim"}]
        }}))
        .unwrap();
    let SpaceApplyLayoutResult::TrustRequired { trust_required } = asked else {
        panic!("{asked:?}")
    };
    assert_eq!(trust_required.hash, "h");
    assert_eq!(trust_required.commands[0].command, "vim");

    let applied: SpaceApplyLayoutResult = serde_json::from_value(serde_json::json!({
        "space": {"id": "s1", "name": "a", "cwd": "/x", "tabs": ["tab2", "tab4"]},
        "tabs": [{"tab_id": "tab4", "space_id": "s1", "name": "a",
                  "root": {"kind": "terminal", "terminal_id": "t5"}, "active_terminal": "t5"}],
        "warnings": [], "setup_started": false
    }))
    .unwrap();
    let SpaceApplyLayoutResult::Applied(applied) = applied else {
        panic!("{applied:?}")
    };
    assert_eq!(applied.space.tabs, ["tab2", "tab4"]);
    assert_eq!(applied.tabs.len(), 1);
    assert_eq!(applied.tabs[0].root, Node::terminal("t5"));
}
