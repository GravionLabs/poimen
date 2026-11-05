//! Claude Code integration: the hook that reports the agent state to the server, and the
//! installer that registers it in `~/.claude/settings.json` next to whatever is there.
//!
//! The hook is `poimen-server hook claude`: Claude Code runs it with the event as JSON on stdin.
//! It does nothing outside a poimen terminal and never blocks or fails Claude.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use poimen_client::Client;
use poimen_protocol::{AgentStatus, ENV_TERMINAL_ID, TerminalReportAgent};
use serde_json::{Map, Value, json};

/// Marks the command of our entries in `settings.json`; install and uninstall find them by it.
const MARKER: &str = "# poimen-agent-hook";
/// The hook events poimen registers.
pub const EVENTS: [&str; 6] = [
    "SessionStart",
    "UserPromptSubmit",
    "Notification",
    "PostToolUse",
    "Stop",
    "SessionEnd",
];
/// Seconds Claude Code waits for the hook before giving up on it.
const HOOK_TIMEOUT_SECS: u64 = 2;

/// What one hook event says about the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub status: AgentStatus,
    pub session_id: Option<String>,
}

/// Maps a hook payload to a report: `SessionStart`/`Stop` idle, `UserPromptSubmit`/`PostToolUse`
/// working, a permission `Notification` blocked, `SessionEnd` gone. Events of subagents (the
/// payload has an `agent_id`) and everything else give `None`.
pub fn map_hook(payload: &Value) -> Option<Report> {
    if payload.get("agent_id").is_some_and(|v| !v.is_null()) {
        return None;
    }
    let status = match payload.get("hook_event_name")?.as_str()? {
        "SessionStart" | "Stop" => AgentStatus::Idle,
        "UserPromptSubmit" | "PostToolUse" => AgentStatus::Working,
        "Notification" => match payload.get("notification_type").and_then(Value::as_str) {
            Some("permission_prompt") => AgentStatus::Blocked,
            _ => return None,
        },
        "SessionEnd" => AgentStatus::Gone,
        _ => return None,
    };
    let session_id = payload
        .get("session_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    Some(Report { status, session_id })
}

/// The report request for a payload in the terminal `terminal_id`.
pub fn request_for(terminal_id: &str, seq: u64, payload: &Value) -> Option<TerminalReportAgent> {
    let report = map_hook(payload)?;
    Some(TerminalReportAgent {
        terminal_id: terminal_id.to_owned(),
        seq,
        status: report.status,
        agent: Some("claude".into()),
        session_id: report.session_id,
    })
}

/// Runs the hook: reads the payload from `stdin` and reports it. Returns quietly in every
/// failure case, and gives up after a short time so Claude Code is never held up. `terminal_id`
/// is `$POIMEN_TERMINAL_ID`; unset (not a poimen terminal) means nothing happens.
pub fn run_hook(stdin: impl Read + Send + 'static, terminal_id: Option<String>, socket: PathBuf) {
    run_hook_with(stdin, terminal_id, socket, request_for);
}

/// [`run_hook`] for any agent: `build` turns the payload into the report request.
pub fn run_hook_with(
    mut stdin: impl Read + Send + 'static,
    terminal_id: Option<String>,
    socket: PathBuf,
    build: impl FnOnce(&str, u64, &Value) -> Option<TerminalReportAgent> + Send + 'static,
) {
    let Some(terminal_id) = terminal_id.filter(|t| !t.is_empty()) else {
        return;
    };
    let (done, wait) = mpsc::channel();
    thread::spawn(move || {
        let mut raw = Vec::new();
        let _ = (&mut stdin).take(1 << 20).read_to_end(&mut raw);
        if let Some(request) = serde_json::from_slice::<Value>(&raw)
            .ok()
            .and_then(|payload| build(&terminal_id, now_nanos(), &payload))
        {
            let _ = Client::new(socket).request(
                "terminal.report_agent",
                serde_json::to_value(request).unwrap_or(Value::Null),
            );
        }
        let _ = done.send(());
    });
    let _ = wait.recv_timeout(Duration::from_millis(1500));
}

/// `$POIMEN_TERMINAL_ID`, when set.
pub fn terminal_id_from_env() -> Option<String> {
    std::env::var(ENV_TERMINAL_ID).ok()
}

pub(crate) fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

// --- settings.json ------------------------------------------------------------------------------

/// The shell command of the hook for the binary at `exe`.
pub fn hook_command(exe: &Path) -> String {
    let quoted = exe.display().to_string().replace('\'', r"'\''");
    format!("'{quoted}' hook claude {MARKER}")
}

/// `<home>/.claude/settings.json`.
pub fn default_settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

fn is_ours(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains(MARKER))
}

fn read_settings(path: &Path) -> Result<Map<String, Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => Ok(map),
            _ => Err(format!(
                "{} is not a JSON object; left untouched",
                path.display()
            )),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// Writes through a temp file in the same directory and a rename, keeping the file mode.
fn write_atomic(path: &Path, settings: &Map<String, Value>) -> Result<(), String> {
    let err = |e: std::io::Error| format!("cannot write {}: {e}", path.display());
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(err)?;
    let tmp = dir.join(format!(".settings.json.poimen-{}", std::process::id()));
    let mut text = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(&tmp, text).map_err(err)?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        err(e)
    })
}

/// Removes our hooks from one event's groups; returns whether anything was removed.
fn strip(groups: &mut Vec<Value>) -> bool {
    let mut removed = false;
    for group in groups.iter_mut() {
        if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            let before = hooks.len();
            hooks.retain(|h| !is_ours(h));
            removed |= hooks.len() != before;
        }
    }
    groups.retain(|g| {
        g.get("hooks")
            .and_then(Value::as_array)
            .is_none_or(|h| !h.is_empty())
    });
    removed
}

/// Registers the hook for every event in `settings_path`, replacing earlier entries of ours and
/// keeping everything else. Idempotent.
pub fn install(settings_path: &Path, exe: &Path) -> Result<(), String> {
    let mut settings = read_settings(settings_path)?;
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("\"hooks\" in settings.json is not an object; left untouched")?;
    let command = hook_command(exe);
    for event in EVENTS {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("hooks.{event} is not an array; left untouched"))?;
        strip(groups);
        groups.push(json!({
            "hooks": [{"type": "command", "command": command, "timeout": HOOK_TIMEOUT_SECS}]
        }));
    }
    write_atomic(settings_path, &settings)
}

/// Removes only our entries (and the event arrays or `hooks` object that this leaves empty).
/// Returns whether anything was removed. A missing file is left missing.
pub fn uninstall(settings_path: &Path) -> Result<bool, String> {
    let mut settings = read_settings(settings_path)?;
    let mut removed = false;
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        for groups in hooks.values_mut().filter_map(Value::as_array_mut) {
            removed |= strip(groups);
        }
        if removed {
            hooks.retain(|_, v| v.as_array().is_none_or(|a| !a.is_empty()));
        }
    }
    if removed {
        if settings
            .get("hooks")
            .and_then(Value::as_object)
            .is_some_and(Map::is_empty)
        {
            settings.remove("hooks");
        }
        write_atomic(settings_path, &settings)?;
    }
    Ok(removed)
}

/// The events of [`EVENTS`] that have our hook in `settings_path`.
pub fn status(settings_path: &Path) -> Result<Vec<&'static str>, String> {
    let settings = read_settings(settings_path)?;
    Ok(EVENTS
        .into_iter()
        .filter(|event| {
            settings
                .get("hooks")
                .and_then(|h| h.get(*event))
                .and_then(Value::as_array)
                .is_some_and(|groups| {
                    groups.iter().any(|g| {
                        g.get("hooks")
                            .and_then(Value::as_array)
                            .is_some_and(|h| h.iter().any(is_ours))
                    })
                })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn prompt_and_tool_events_are_working_and_stop_is_idle() {
        let mut tool = event("PostToolUse");
        tool["tool_name"] = json!("Bash");
        assert_eq!(
            status_of(&event("UserPromptSubmit")),
            Some(AgentStatus::Working)
        );
        assert_eq!(status_of(&tool), Some(AgentStatus::Working));
        assert_eq!(status_of(&event("Stop")), Some(AgentStatus::Idle));
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
            json!({"terminal_id":"t4","seq":9,"status":"idle","agent":"claude","session_id":"abc"})
        );
        assert!(request_for("t4", 9, &event("PreToolUse")).is_none());
    }

    fn nowhere() -> PathBuf {
        PathBuf::from("/nonexistent/poimen/s.sock")
    }

    #[test]
    fn the_hook_does_nothing_outside_a_poimen_terminal() {
        // No terminal id: returns at once without reading stdin or touching the socket.
        run_hook(std::io::empty(), None, nowhere());
        run_hook(std::io::empty(), Some(String::new()), nowhere());
    }

    #[test]
    fn the_hook_survives_garbage_and_a_missing_server() {
        let payload = br#"{"hook_event_name":"Stop","session_id":"x"}"#.to_vec();
        run_hook(std::io::Cursor::new(payload), Some("t1".into()), nowhere());
        run_hook(
            std::io::Cursor::new(b"not json".to_vec()),
            Some("t1".into()),
            nowhere(),
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
        install(&path, Path::new("/opt/poimen-server")).unwrap();
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
        assert_eq!(
            start[1]["hooks"][0]["command"],
            "'/opt/poimen-server' hook claude # poimen-agent-hook"
        );
        assert_eq!(start[1]["hooks"][0]["timeout"], 2);
        assert_eq!(status(&path).unwrap(), EVENTS.to_vec());
        cleanup(&path);
    }

    #[test]
    fn install_twice_changes_nothing_and_a_moved_binary_replaces_the_entry() {
        let path = temp("idempotent");
        install(&path, Path::new("/a/poimen-server")).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        install(&path, Path::new("/a/poimen-server")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        install(&path, Path::new("/b/it's/poimen-server")).unwrap();
        let s = read(&path);
        assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(
            s["hooks"]["Stop"][0]["hooks"][0]["command"],
            r"'/b/it'\''s/poimen-server' hook claude # poimen-agent-hook"
        );
        cleanup(&path);
    }

    #[test]
    fn uninstall_removes_only_ours_and_restores_the_original() {
        let path = temp("uninstall");
        std::fs::write(&path, herdr_settings().to_string()).unwrap();
        install(&path, Path::new("/x/poimen-server")).unwrap();
        assert!(uninstall(&path).unwrap());
        assert_eq!(read(&path), herdr_settings());
        assert!(!uninstall(&path).unwrap());
        assert!(status(&path).unwrap().is_empty());
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
        assert!(uninstall(&path).unwrap());
        assert_eq!(
            read(&path),
            json!({"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "mine.sh"}]}]}})
        );
        cleanup(&path);
    }

    #[test]
    fn a_missing_file_is_created_on_install_and_not_on_uninstall() {
        let path = temp("missing");
        assert!(!uninstall(&path).unwrap());
        assert!(!path.exists());
        assert!(status(&path).unwrap().is_empty());
        install(&path, Path::new("/x/poimen-server")).unwrap();
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
            assert!(install(&path, Path::new("/x/p")).is_err(), "{bad}");
            assert_eq!(std::fs::read_to_string(&path).unwrap(), bad);
        }
        cleanup(&path);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_mode_is_kept() {
        use std::os::unix::fs::PermissionsExt;
        let path = temp("mode");
        std::fs::write(&path, "{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        install(&path, Path::new("/x/p")).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        cleanup(&path);
    }
}
