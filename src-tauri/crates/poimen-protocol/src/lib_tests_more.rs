use super::*;
use serde_json::json;

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
