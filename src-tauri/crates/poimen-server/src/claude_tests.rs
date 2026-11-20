//! Tests of `claude.rs` (kept in a file of their own: the module is long).

use super::*;

fn install(p: &Path, e: &Path, c: Channel) -> Result<(), String> {
    super::install(p, e, c, None)
}

fn status(p: &Path, c: Channel) -> Result<Vec<&'static str>, String> {
    super::status(p, c, None).map(|s| s.installed)
}

fn event(name: &str) -> Value {
    json!({"session_id": "abc", "hook_event_name": name, "cwd": "/w"})
}

fn status_of(payload: &Value) -> Option<AgentStatus> {
    map_hook(payload).map(|r| r.status)
}

// Payloads as Claude Code sends them (fields the mapping does not read are included).
#[test]
fn session_start_is_idle_and_carries_the_session_id() {
    let mut p = event("SessionStart");
    p["source"] = json!("startup");
    let r = map_hook(&p).unwrap();
    assert_eq!(r.status, AgentStatus::Idle);
    assert_eq!(r.session_id.as_deref(), Some("abc"));
}

#[test]
fn prompt_and_tool_events_are_working_and_stop_is_done() {
    let mut tool = event("PostToolUse");
    tool["tool_name"] = json!("Bash");
    assert_eq!(
        status_of(&event("UserPromptSubmit")),
        Some(AgentStatus::Working)
    );
    assert_eq!(status_of(&tool), Some(AgentStatus::Working));
    assert_eq!(status_of(&event("Stop")), Some(AgentStatus::Done));
}

#[test]
fn only_a_permission_notification_blocks() {
    let mut p = event("Notification");
    p["notification_type"] = json!("permission_prompt");
    p["message"] = json!("Claude needs your permission to use Bash");
    assert_eq!(status_of(&p), Some(AgentStatus::Blocked));
    p["notification_type"] = json!("idle_prompt");
    assert_eq!(status_of(&p), None);
    assert_eq!(status_of(&event("Notification")), None);
}

#[test]
fn a_permission_notification_carries_its_text_other_events_do_not() {
    let fixture: Value =
        serde_json::from_str(include_str!("../fixtures/claude-notification.json")).unwrap();
    let r = map_hook(&fixture).unwrap();
    assert_eq!(
        r.message.as_deref(),
        Some("Claude needs your permission to use Bash")
    );
    let mut long = fixture.clone();
    long["message"] = json!(format!("a\u{1b}[1m\n{}", "x".repeat(500)));
    let text = map_hook(&long).unwrap().message.unwrap();
    assert_eq!(text.chars().count(), 300);
    assert!(text.starts_with("a [1m x"));
    long["message"] = json!("  ");
    assert_eq!(map_hook(&long).unwrap().message, None);
    let mut stop = event("Stop");
    stop["message"] = json!("ignored");
    assert_eq!(map_hook(&stop).unwrap().message, None);
    let request = request_for("t1", 5, &fixture).unwrap();
    assert_eq!(request.message, r.message);
}

#[test]
fn session_end_means_the_agent_is_gone() {
    let mut p = event("SessionEnd");
    p["reason"] = json!("logout");
    assert_eq!(status_of(&p), Some(AgentStatus::Gone));
}

#[test]
fn subagent_and_unknown_events_are_ignored() {
    let mut sub = event("PostToolUse");
    sub["agent_id"] = json!("a1b2");
    sub["agent_type"] = json!("Explore");
    assert_eq!(status_of(&sub), None);
    assert_eq!(status_of(&event("SubagentStop")), None);
    assert_eq!(status_of(&event("PreToolUse")), None);
    assert_eq!(status_of(&json!({})), None);
    assert_eq!(status_of(&json!("nope")), None);
}

#[test]
fn the_request_names_claude_and_the_terminal() {
    let r = request_for("t4", 9, &event("Stop")).unwrap();
    assert_eq!(
        serde_json::to_value(r).unwrap(),
        json!({"terminal_id":"t4","seq":9,"status":"done","agent":"claude","session_id":"abc"})
    );
    assert!(request_for("t4", 9, &event("PreToolUse")).is_none());
}

fn env(terminal_id: Option<&str>, in_poimen: bool) -> HookEnv {
    HookEnv {
        terminal_id: terminal_id.map(str::to_owned),
        in_poimen,
        foreign: false,
        socket: PathBuf::from("/nonexistent/poimen/s.sock"),
        log: None,
    }
}

#[test]
fn the_hook_does_nothing_outside_a_poimen_terminal() {
    // No terminal id: returns at once without reading stdin or touching the socket.
    assert_eq!(
        run_hook(std::io::empty(), &env(None, false)),
        Outcome::Outside
    );
    assert_eq!(
        run_hook(std::io::empty(), &env(Some(""), false)),
        Outcome::Outside
    );
}

#[test]
fn the_hook_survives_garbage_and_a_missing_server() {
    let failed = |o: Outcome| matches!(o, Outcome::Failed(_));
    let payload = br#"{"hook_event_name":"Stop","session_id":"x"}"#.to_vec();
    let in_terminal = env(Some("t1"), true);
    assert!(failed(run_hook(
        std::io::Cursor::new(payload),
        &in_terminal
    )));
    assert!(failed(run_hook(
        std::io::Cursor::new(b"not json".to_vec()),
        &in_terminal,
    )));
    let tool = br#"{"hook_event_name":"PreToolUse","session_id":"x"}"#.to_vec();
    assert_eq!(
        run_hook(std::io::Cursor::new(tool), &in_terminal),
        Outcome::Ignored
    );
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("poimen-claude-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("settings.json")
}

fn herdr_settings() -> Value {
    json!({
        "model": "opus",
        "permissions": {"allow": ["Bash(ls)"]},
        "hooks": {
            "SessionStart": [{"hooks": [{"type": "command", "command": "~/.claude/hooks/herdr-agent-state.sh session"}]}],
            "PreToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": "check.sh"}]}]
        }
    })
}

fn read(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn cleanup(path: &Path) {
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn install_adds_our_hooks_and_keeps_everything_else() {
    let path = temp("install");
    std::fs::write(&path, herdr_settings().to_string()).unwrap();
    let exe = fake_exe(&path);
    install(&path, &exe, Channel::Stable).unwrap();
    let s = read(&path);
    assert_eq!(s["model"], "opus");
    assert_eq!(s["permissions"], herdr_settings()["permissions"]);
    assert_eq!(
        s["hooks"]["PreToolUse"],
        herdr_settings()["hooks"]["PreToolUse"]
    );
    let start = s["hooks"]["SessionStart"].as_array().unwrap();
    assert_eq!(start.len(), 2);
    assert_eq!(start[0], herdr_settings()["hooks"]["SessionStart"][0]);
    // The entry of this platform (`hook_entry` is checked literally for both).
    assert_eq!(
        start[1]["hooks"][0],
        hook_entry(&exe, cfg!(windows), Channel::Stable)
    );
    assert_eq!(status(&path, Channel::Stable).unwrap(), EVENTS.to_vec());
    cleanup(&path);
}

/// A file standing in for the server binary next to `settings` (status checks it exists).
fn fake_exe(settings: &Path) -> PathBuf {
    let exe = settings.with_file_name("poimen-server");
    std::fs::write(&exe, "").unwrap();
    exe
}

#[test]
fn entries_whose_binary_is_gone_are_not_installed() {
    // An AppImage mount of an earlier run: the marker is there, the binary is not.
    let path = temp("gone");
    let exe = path.with_file_name("mount").join("poimen-server");
    install(&path, &exe, Channel::Stable).unwrap();
    assert!(status(&path, Channel::Stable).unwrap().is_empty());
    // Installing again with a binary that exists replaces the stale entries.
    let exe = fake_exe(&path);
    install(&path, &exe, Channel::Stable).unwrap();
    assert_eq!(status(&path, Channel::Stable).unwrap(), EVENTS.to_vec());
    std::fs::remove_file(&exe).unwrap();
    assert!(status(&path, Channel::Stable).unwrap().is_empty());
    cleanup(&path);
}

#[test]
fn the_hook_binary_is_the_given_one_else_the_running_one() {
    let path = temp("hook-exe");
    let exe = fake_exe(&path);
    let running = || Ok(PathBuf::from("/tmp/.mount_poimenX/usr/bin/poimen-server"));
    assert_eq!(hook_exe(Some(exe.clone().into()), running), Ok(exe.clone()));
    assert_eq!(
        hook_exe(None, running),
        Ok(PathBuf::from("/tmp/.mount_poimenX/usr/bin/poimen-server"))
    );
    assert_eq!(
        hook_exe(Some("".into()), running),
        Ok(PathBuf::from("/tmp/.mount_poimenX/usr/bin/poimen-server"))
    );
    assert!(hook_exe(Some("poimen-server".into()), running).is_err());
    assert!(hook_exe(Some(path.with_file_name("missing").into()), running).is_err());
    let failing = || Err(std::io::Error::other("no exe"));
    assert_eq!(hook_exe(None, failing), Err("no exe".into()));
    cleanup(&path);
}

#[test]
fn the_binary_of_a_hook_command_is_read_back() {
    let unix = Path::new("/b/it's/poimen-server");
    assert_eq!(
        hook_target(&hook_command(unix, Channel::Stable)),
        Some(unix.to_path_buf())
    );
    let win = Path::new(r"C:\Users\Jo O'Neil\poimen-server.exe");
    assert_eq!(
        hook_target(&powershell_command(win, "claude", Channel::Stable)),
        Some(win.to_path_buf())
    );
    assert_eq!(
        hook_target("& 'C:\\a\u{2019}\u{2019}b.exe' hook claude"),
        Some(PathBuf::from("C:\\a\u{2019}b.exe"))
    );
    assert_eq!(
        hook_target("poimen-server hook claude # poimen-agent-hook"),
        None
    );
    assert_eq!(hook_target("'/unterminated hook claude"), None);
}

#[test]
fn install_twice_changes_nothing_and_a_moved_binary_replaces_the_entry() {
    let path = temp("idempotent");
    install(&path, Path::new("/a/poimen-server"), Channel::Stable).unwrap();
    let first = std::fs::read_to_string(&path).unwrap();
    install(&path, Path::new("/a/poimen-server"), Channel::Stable).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
    install(&path, Path::new("/b/it's/poimen-server"), Channel::Stable).unwrap();
    let s = read(&path);
    assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 1);
    assert_eq!(
        s["hooks"]["Stop"][0]["hooks"][0],
        hook_entry(
            Path::new("/b/it's/poimen-server"),
            cfg!(windows),
            Channel::Stable
        )
    );
    if !cfg!(windows) {
        assert_eq!(
            s["hooks"]["Stop"][0]["hooks"][0]["command"],
            r"'/b/it'\''s/poimen-server' hook claude # poimen-agent-hook"
        );
    }
    cleanup(&path);
}

#[test]
fn stable_and_rc_hooks_coexist_and_each_channel_manages_only_its_own() {
    let path = temp("channels");
    std::fs::write(&path, herdr_settings().to_string()).unwrap();
    let stable = fake_exe(&path);
    let rc = path.with_file_name("poimen-server-rc");
    std::fs::write(&rc, "").unwrap();
    install(&path, &stable, Channel::Stable).unwrap();
    install(&path, &rc, Channel::Rc).unwrap();
    // Installing either again replaces only its own entry.
    install(&path, &stable, Channel::Stable).unwrap();
    let s = read(&path);
    let stop = s["hooks"]["Stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2);
    assert_eq!(
        stop[0]["hooks"][0],
        hook_entry(&rc, cfg!(windows), Channel::Rc)
    );
    assert_eq!(
        stop[1]["hooks"][0],
        hook_entry(&stable, cfg!(windows), Channel::Stable)
    );
    if !cfg!(windows) {
        assert_eq!(
            stop[0]["hooks"][0]["command"],
            format!("'{}' hook claude # poimen-rc-agent-hook", rc.display())
        );
    }
    assert_eq!(status(&path, Channel::Stable).unwrap(), EVENTS.to_vec());
    assert_eq!(status(&path, Channel::Rc).unwrap(), EVENTS.to_vec());
    // Removing the rc hooks leaves the stable ones, and the other way round.
    assert!(uninstall(&path, Channel::Rc).unwrap());
    assert!(status(&path, Channel::Rc).unwrap().is_empty());
    assert_eq!(status(&path, Channel::Stable).unwrap(), EVENTS.to_vec());
    install(&path, &rc, Channel::Rc).unwrap();
    assert!(uninstall(&path, Channel::Stable).unwrap());
    assert!(status(&path, Channel::Stable).unwrap().is_empty());
    assert_eq!(status(&path, Channel::Rc).unwrap(), EVENTS.to_vec());
    assert!(uninstall(&path, Channel::Rc).unwrap());
    assert_eq!(read(&path), herdr_settings());
    cleanup(&path);
}

#[test]
fn neither_channel_marker_contains_the_other() {
    let (stable, rc) = (marker(Channel::Stable), marker(Channel::Rc));
    assert_eq!(stable, "# poimen-agent-hook");
    assert_eq!(rc, "# poimen-rc-agent-hook");
    assert!(!stable.contains(rc) && !rc.contains(stable));
}

#[test]
fn uninstall_removes_only_ours_and_restores_the_original() {
    let path = temp("uninstall");
    std::fs::write(&path, herdr_settings().to_string()).unwrap();
    install(&path, Path::new("/x/poimen-server"), Channel::Stable).unwrap();
    assert!(uninstall(&path, Channel::Stable).unwrap());
    assert_eq!(read(&path), herdr_settings());
    assert!(!uninstall(&path, Channel::Stable).unwrap());
    assert!(status(&path, Channel::Stable).unwrap().is_empty());
    cleanup(&path);
}

#[test]
fn uninstall_leaves_a_mixed_group_with_its_other_hook() {
    let path = temp("mixed");
    let mixed = json!({"hooks": {"Stop": [{"hooks": [
        {"type": "command", "command": "mine.sh"},
        {"type": "command", "command": "'/x/poimen-server' hook claude # poimen-agent-hook"}
    ]}]}});
    std::fs::write(&path, mixed.to_string()).unwrap();
    assert!(uninstall(&path, Channel::Stable).unwrap());
    assert_eq!(
        read(&path),
        json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "mine.sh"}]}]}})
    );
    cleanup(&path);
}

#[test]
fn a_missing_file_is_created_on_install_and_not_on_uninstall() {
    let path = temp("missing");
    assert!(!uninstall(&path, Channel::Stable).unwrap());
    assert!(!path.exists());
    assert!(status(&path, Channel::Stable).unwrap().is_empty());
    install(&path, Path::new("/x/poimen-server"), Channel::Stable).unwrap();
    assert!(path.exists());
    cleanup(&path);
}

#[test]
fn broken_settings_are_refused_and_left_untouched() {
    let path = temp("broken");
    for bad in [
        "{not json",
        "[1,2]",
        r#"{"hooks": []}"#,
        r#"{"hooks": {"Stop": {}}}"#,
    ] {
        std::fs::write(&path, bad).unwrap();
        assert!(
            install(&path, Path::new("/x/p"), Channel::Stable).is_err(),
            "{bad}"
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
    }
    cleanup(&path);
}

#[test]
fn the_windows_entry_runs_in_powershell_with_a_longer_timeout() {
    let exe = Path::new(r"C:\Users\Jo O'Neil\AppData\Local\poimen\poimen-server.exe");
    assert_eq!(
        hook_entry(exe, true, Channel::Stable),
        json!({
            "type": "command",
            "shell": "powershell",
            "command": r"& 'C:\Users\Jo O''Neil\AppData\Local\poimen\poimen-server.exe' hook claude # poimen-agent-hook",
            "timeout": 5
        })
    );
    assert!(is_ours(
        &hook_entry(exe, true, Channel::Stable),
        Channel::Stable
    ));
}

#[test]
fn the_unix_entry_is_unchanged() {
    assert_eq!(
        hook_entry(Path::new("/opt/poimen-server"), false, Channel::Stable),
        json!({
            "type": "command",
            "command": "'/opt/poimen-server' hook claude # poimen-agent-hook",
            "timeout": 2
        })
    );
}

#[test]
fn powershell_quoting_doubles_every_single_quote() {
    assert_eq!(powershell_quote("plain"), "'plain'");
    assert_eq!(powershell_quote(""), "''");
    assert_eq!(powershell_quote("a'b"), "'a''b'");
    assert_eq!(powershell_quote("a\u{2019}b"), "'a\u{2019}\u{2019}b'");
    assert_eq!(powershell_quote("$x `n"), "'$x `n'");
}

#[cfg(unix)]
#[test]
fn the_file_mode_is_kept() {
    use std::os::unix::fs::PermissionsExt;
    let path = temp("mode");
    std::fs::write(&path, "{}").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    install(&path, Path::new("/x/p"), Channel::Stable).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    cleanup(&path);
}

// --- permission requests, failed turns, CLI versions (#984) -------------------------------------

#[test]
fn a_permission_request_blocks_with_the_tool_and_its_input() {
    let mut p = event("PermissionRequest");
    p["tool_name"] = json!("Bash");
    p["tool_input"] = json!({"command": "rm -rf build", "description": "clean"});
    let r = map_hook(&p).unwrap();
    assert_eq!(r.status, AgentStatus::Blocked);
    assert_eq!(r.message.as_deref(), Some("Bash: rm -rf build"));
    assert!(r.fallback_message);
    p["tool_input"] = json!({"file_path": "/w/a.rs"});
    assert_eq!(
        map_hook(&p).unwrap().message.as_deref(),
        Some("Bash: /w/a.rs")
    );
    p["tool_input"] = json!({"odd": 1});
    assert_eq!(
        map_hook(&p).unwrap().message.as_deref(),
        Some(r#"Bash: {"odd":1}"#)
    );
    p["tool_input"] = json!({});
    assert_eq!(
        map_hook(&p).unwrap().message.as_deref(),
        Some("Bash needs your permission")
    );
    p["tool_name"] = json!("");
    assert_eq!(map_hook(&p).unwrap().message, None);
    // The text of a notification is not a stand-in.
    let mut n = event("Notification");
    n["notification_type"] = json!("permission_prompt");
    assert!(!map_hook(&n).unwrap().fallback_message);
}

#[test]
fn failed_turns_and_tool_events_map_to_error_and_working() {
    assert_eq!(status_of(&event("StopFailure")), Some(AgentStatus::Error));
    assert_eq!(
        status_of(&event("PostToolUseFailure")),
        Some(AgentStatus::Working)
    );
    assert_eq!(
        status_of(&event("PostToolBatch")),
        Some(AgentStatus::Working)
    );
    for name in [
        "PermissionRequest",
        "StopFailure",
        "PostToolUseFailure",
        "PostToolBatch",
    ] {
        let mut sub = event(name);
        sub["agent_id"] = json!("a1");
        assert_eq!(status_of(&sub), None, "{name} of a subagent");
    }
    let request = request_for("t1", 3, &event("StopFailure")).unwrap();
    assert_eq!(request.status, AgentStatus::Error);
    assert!(!request.message_fallback);
}

#[test]
fn the_cli_version_is_read_from_its_output() {
    assert_eq!(parse_version("2.1.329 (Claude Code)\n"), Some((2, 1, 329)));
    assert_eq!(parse_version("2.0.45"), Some((2, 0, 45)));
    assert_eq!(parse_version("claude 2.1.0"), None);
    assert_eq!(parse_version("2.1 (Claude Code)"), None);
    assert_eq!(parse_version(""), None);
}

#[test]
fn the_events_follow_the_cli_version() {
    let later = |v| events_for(v).len() - EVENTS.len();
    assert_eq!(events_for(None), EVENTS.to_vec());
    assert_eq!(later(Some((2, 0, 44))), 0);
    assert_eq!(later(Some((2, 0, 45))), 1);
    assert_eq!(later(Some((2, 0, 56))), 2);
    assert_eq!(later(Some((2, 1, 77))), 2);
    assert_eq!(later(Some((2, 1, 78))), 3);
    assert_eq!(later(Some((2, 1, 117))), 3);
    assert_eq!(later(Some((2, 1, 118))), 4);
    assert_eq!(later(Some((3, 0, 0))), 4);
    assert!(events_for(Some((2, 1, 118))).contains(&"PostToolBatch"));
}

fn installed_events(path: &Path) -> Vec<String> {
    let mut names: Vec<String> = read(path)["hooks"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    names.sort();
    names
}

#[test]
fn install_writes_only_the_events_the_cli_knows() {
    for (cli, count) in [
        (None, 6),
        (Some((2, 0, 44)), 6),
        (Some((2, 0, 45)), 7),
        (Some((2, 1, 77)), 8),
        (Some((2, 1, 200)), 10),
    ] {
        let path = temp("versions");
        let exe = fake_exe(&path);
        super::install(&path, &exe, Channel::Stable, cli).unwrap();
        assert_eq!(installed_events(&path).len(), count, "{cli:?}");
        let s = super::status(&path, Channel::Stable, cli).unwrap();
        assert_eq!((s.installed.len(), s.missing.len()), (count, 0), "{cli:?}");
        cleanup(&path);
    }
}

#[test]
fn status_reports_the_events_missing_for_the_installed_cli() {
    let path = temp("missing");
    let exe = fake_exe(&path);
    super::install(&path, &exe, Channel::Stable, None).unwrap();
    let new = Some((2, 1, 329));
    let s = super::status(&path, Channel::Stable, new).unwrap();
    assert_eq!(s.installed.len(), 6);
    assert_eq!(
        s.missing,
        [
            "PermissionRequest",
            "PostToolUseFailure",
            "StopFailure",
            "PostToolBatch"
        ]
    );
    // An unknown version needs only the six.
    assert!(
        super::status(&path, Channel::Stable, None)
            .unwrap()
            .missing
            .is_empty()
    );
    // Repair: installing again for the new version completes it.
    super::install(&path, &exe, Channel::Stable, new).unwrap();
    assert!(
        super::status(&path, Channel::Stable, new)
            .unwrap()
            .missing
            .is_empty()
    );
    // Nothing installed: no event is installed, all are missing.
    cleanup(&path);
    let none = temp("missing-none");
    let s = super::status(&none, Channel::Stable, new).unwrap();
    assert!(s.installed.is_empty() && s.missing.len() == 10);
    cleanup(&none);
}

#[test]
fn a_downgraded_cli_loses_the_events_it_does_not_know() {
    let path = temp("downgrade");
    let exe = Path::new("/x/poimen-server");
    let mut settings = herdr_settings();
    settings["hooks"]["PermissionRequest"] =
        json!([{"hooks": [{"type": "command", "command": "mine.sh"}]}]);
    std::fs::write(&path, settings.to_string()).unwrap();
    super::install(&path, exe, Channel::Stable, Some((2, 1, 329))).unwrap();
    assert_eq!(
        read(&path)["hooks"]["PermissionRequest"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    super::install(&path, exe, Channel::Stable, Some((2, 0, 10))).unwrap();
    let s = read(&path);
    // The user's own entry stays, ours is gone, and the other events are gone entirely.
    assert_eq!(
        s["hooks"]["PermissionRequest"],
        settings["hooks"]["PermissionRequest"]
    );
    assert!(s["hooks"].get("StopFailure").is_none());
    assert_eq!(installed_events(&path).len(), 8);
    cleanup(&path);
}

#[test]
fn uninstall_removes_the_new_entries_and_leaves_other_channels_and_the_user_alone() {
    let path = temp("uninstall-new");
    std::fs::write(&path, herdr_settings().to_string()).unwrap();
    let new = Some((2, 1, 329));
    let stable = fake_exe(&path);
    let rc = path.with_file_name("poimen-server-rc");
    std::fs::write(&rc, "").unwrap();
    super::install(&path, &stable, Channel::Stable, new).unwrap();
    super::install(&path, &rc, Channel::Rc, new).unwrap();
    assert!(uninstall(&path, Channel::Stable).unwrap());
    assert!(
        super::status(&path, Channel::Stable, new)
            .unwrap()
            .installed
            .is_empty()
    );
    assert_eq!(
        super::status(&path, Channel::Rc, new)
            .unwrap()
            .installed
            .len(),
        10
    );
    assert!(uninstall(&path, Channel::Rc).unwrap());
    assert_eq!(read(&path), herdr_settings());
    cleanup(&path);
}
