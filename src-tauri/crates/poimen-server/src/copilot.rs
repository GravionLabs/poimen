//! GitHub Copilot CLI integration: the hook that reports the agent state to the server, and the
//! installer that writes it as `~/.copilot/hooks/poimen-agent-state.json` (a file of our own,
//! so herdr's hook and the user's settings stay untouched).
//!
//! The hook is `poimen-server hook copilot <event>`. Copilot CLI sends camelCase payloads
//! (`sessionId`, `notificationType`) without the event name when the hook file uses camelCase
//! event keys, and the PascalCase style (`hook_event_name`, `session_id`, `notification_type`)
//! otherwise; both are accepted, and the event given on the command line is the fallback.
//! Permission prompts are reported by the `notification` hook (`notificationType`
//! `permission_prompt`, checked in Copilot CLI 1.0.71), so no `preToolUse` fallback is needed.

use std::path::{Path, PathBuf};

use poimen_protocol::{AgentStatus, TerminalReportAgent};
use serde_json::{Map, Value, json};

use crate::claude::Report;

/// Marks the command of our entries; status and uninstall find the file by it.
const MARKER: &str = "# poimen-agent-hook";
/// The hook events poimen registers (camelCase keys of the hook file).
pub const EVENTS: [&str; 6] = [
    "sessionStart",
    "userPromptSubmitted",
    "notification",
    "postToolUse",
    "agentStop",
    "sessionEnd",
];
/// Seconds Copilot CLI waits for the hook before giving up on it.
const HOOK_TIMEOUT_SECS: u64 = 2;
const FILE_NAME: &str = "poimen-agent-state.json";

fn first_str<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|k| payload.get(*k).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
}

/// `sessionStart`, `SessionStart`, `session_start` all compare equal.
fn normalize(event: &str) -> String {
    event
        .chars()
        .filter(|c| *c != '_' && *c != '-')
        .collect::<String>()
        .to_ascii_lowercase()
}

/// Maps a hook payload to a report: `sessionStart`/`agentStop` idle,
/// `userPromptSubmitted`/`postToolUse` working, a `permission_prompt` notification blocked,
/// `sessionEnd` gone. `event_hint` is the event the hook was registered for; the payload's own
/// event name wins. Subagent events (an `agentId`/`agent_id`) and everything else give `None`.
pub fn map_hook(event_hint: Option<&str>, payload: &Value) -> Option<Report> {
    if !payload.is_object() {
        return None;
    }
    if ["agentId", "agent_id"]
        .iter()
        .any(|k| payload.get(*k).is_some_and(|v| !v.is_null()))
    {
        return None;
    }
    let event = first_str(payload, &["hook_event_name", "hookEventName"]).or(event_hint)?;
    let status = match normalize(event).as_str() {
        "sessionstart" | "agentstop" | "stop" => AgentStatus::Idle,
        "userpromptsubmitted" | "userpromptsubmit" | "posttooluse" => AgentStatus::Working,
        "notification" => match first_str(payload, &["notification_type", "notificationType"]) {
            Some("permission_prompt") => AgentStatus::Blocked,
            _ => return None,
        },
        "sessionend" => AgentStatus::Gone,
        _ => return None,
    };
    let session_id = first_str(payload, &["session_id", "sessionId"]).map(str::to_owned);
    Some(Report { status, session_id })
}

/// The report request for a payload in the terminal `terminal_id`.
pub fn request_for(
    event_hint: Option<&str>,
    terminal_id: &str,
    seq: u64,
    payload: &Value,
) -> Option<TerminalReportAgent> {
    let report = map_hook(event_hint, payload)?;
    Some(TerminalReportAgent {
        terminal_id: terminal_id.to_owned(),
        seq,
        status: report.status,
        agent: Some("copilot".into()),
        session_id: report.session_id,
    })
}

/// Runs the hook for `event` (see [`crate::claude::run_hook_with`]).
pub fn run_hook(
    stdin: impl std::io::Read + Send + 'static,
    event: Option<String>,
    terminal_id: Option<String>,
    socket: PathBuf,
) {
    crate::claude::run_hook_with(stdin, terminal_id, socket, move |t, seq, p| {
        request_for(event.as_deref(), t, seq, p)
    });
}

// --- hook file ----------------------------------------------------------------------------------

/// The shell command of the hook for `event` and the binary at `exe`.
pub fn hook_command(exe: &Path, event: &str) -> String {
    let quoted = exe.display().to_string().replace('\'', r"'\''");
    format!("'{quoted}' hook copilot {event} {MARKER}")
}

/// `<home>/.copilot/hooks/poimen-agent-state.json`.
pub fn default_hook_path(home: &Path) -> PathBuf {
    home.join(".copilot").join("hooks").join(FILE_NAME)
}

fn hook_file(exe: &Path) -> Value {
    let mut hooks = Map::new();
    for event in EVENTS {
        hooks.insert(
            event.to_owned(),
            json!([{
                "type": "command",
                "bash": hook_command(exe, event),
                "timeoutSec": HOOK_TIMEOUT_SECS
            }]),
        );
    }
    json!({"version": 1, "hooks": hooks})
}

fn read_ours(path: &Path) -> Result<Option<Value>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.contains(MARKER) => Ok(serde_json::from_str(&text).ok()),
        Ok(_) => Err(format!(
            "{} exists and is not ours; left untouched",
            path.display()
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// Writes the hook file for every event, replacing an earlier one of ours. A file at that path
/// that does not carry our marker is left alone. Idempotent.
pub fn install(path: &Path, exe: &Path) -> Result<(), String> {
    read_ours(path)?;
    let err = |e: std::io::Error| format!("cannot write {}: {e}", path.display());
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(err)?;
    let tmp = dir.join(format!(".{FILE_NAME}.poimen-{}", std::process::id()));
    let mut text = serde_json::to_string_pretty(&hook_file(exe)).map_err(|e| e.to_string())?;
    text.push('\n');
    std::fs::write(&tmp, text).map_err(err)?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        err(e)
    })
}

/// Removes our hook file. Returns whether it existed; a file that is not ours stays.
pub fn uninstall(path: &Path) -> Result<bool, String> {
    if read_ours(path)?.is_none() && !path.exists() {
        return Ok(false);
    }
    std::fs::remove_file(path).map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
    Ok(true)
}

/// The events of [`EVENTS`] that our hook file registers.
pub fn status(path: &Path) -> Result<Vec<&'static str>, String> {
    let file = read_ours(path)?;
    Ok(EVENTS
        .into_iter()
        .filter(|event| {
            file.as_ref()
                .and_then(|f| f.get("hooks")?.get(*event)?.as_array())
                .is_some_and(|entries| {
                    entries.iter().any(|h| {
                        h.get("bash")
                            .and_then(Value::as_str)
                            .is_some_and(|c| c.contains(MARKER))
                    })
                })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(hint: Option<&str>, p: Value) -> Option<AgentStatus> {
        map_hook(hint, &p).map(|r| r.status)
    }

    // camelCase style: no event name in the payload, the registered event is the hint.
    #[test]
    fn camel_case_payloads_map_by_the_registered_event() {
        let start = json!({"sessionId": "s1", "timestamp": 1, "cwd": "/w", "source": "new"});
        let r = map_hook(Some("sessionStart"), &start).unwrap();
        assert_eq!(
            (r.status, r.session_id.as_deref()),
            (AgentStatus::Idle, Some("s1"))
        );
        let prompt = json!({"sessionId": "s1", "prompt": "fix it"});
        assert_eq!(
            st(Some("userPromptSubmitted"), prompt),
            Some(AgentStatus::Working)
        );
        let tool = json!({"sessionId": "s1", "toolName": "bash", "toolResult": {}});
        assert_eq!(st(Some("postToolUse"), tool), Some(AgentStatus::Working));
        let stop = json!({"sessionId": "s1", "stopReason": "end_turn"});
        assert_eq!(st(Some("agentStop"), stop), Some(AgentStatus::Idle));
        let end = json!({"sessionId": "s1", "reason": "user_exit"});
        assert_eq!(st(Some("sessionEnd"), end), Some(AgentStatus::Gone));
    }

    // PascalCase style: event name and snake_case fields in the payload.
    #[test]
    fn pascal_case_payloads_map_by_their_own_event_name() {
        let ev = |name: &str| json!({"hook_event_name": name, "session_id": "s2", "cwd": "/w"});
        let r = map_hook(None, &ev("SessionStart")).unwrap();
        assert_eq!(
            (r.status, r.session_id.as_deref()),
            (AgentStatus::Idle, Some("s2"))
        );
        assert_eq!(st(None, ev("UserPromptSubmit")), Some(AgentStatus::Working));
        assert_eq!(st(None, ev("PostToolUse")), Some(AgentStatus::Working));
        assert_eq!(st(None, ev("Stop")), Some(AgentStatus::Idle));
        assert_eq!(st(None, ev("SessionEnd")), Some(AgentStatus::Gone));
        // The payload's name wins over the hint.
        assert_eq!(
            st(Some("sessionStart"), ev("SessionEnd")),
            Some(AgentStatus::Gone)
        );
    }

    #[test]
    fn only_a_permission_notification_blocks_in_both_styles() {
        let camel = json!({"sessionId": "s", "message": "Permission needed", "title": "Permission needed", "notificationType": "permission_prompt"});
        assert_eq!(st(Some("notification"), camel), Some(AgentStatus::Blocked));
        let pascal = json!({"hook_event_name": "Notification", "session_id": "s", "notification_type": "permission_prompt"});
        assert_eq!(st(None, pascal), Some(AgentStatus::Blocked));
        let other = json!({"sessionId": "s", "notificationType": "agent_idle"});
        assert_eq!(st(Some("notification"), other), None);
        assert_eq!(st(Some("notification"), json!({"sessionId": "s"})), None);
    }

    #[test]
    fn subagent_unknown_and_malformed_payloads_are_ignored() {
        let sub = json!({"sessionId": "s", "agentId": "a1"});
        assert_eq!(st(Some("postToolUse"), sub), None);
        let sub = json!({"hook_event_name": "PostToolUse", "agent_id": "a1"});
        assert_eq!(st(None, sub), None);
        assert_eq!(st(Some("preToolUse"), json!({"sessionId": "s"})), None);
        assert_eq!(st(Some("subagentStop"), json!({"sessionId": "s"})), None);
        assert_eq!(st(None, json!({"sessionId": "s"})), None);
        assert_eq!(st(Some("agentStop"), json!("nope")), None);
        let r = map_hook(Some("agentStop"), &json!({})).unwrap();
        assert_eq!(r.session_id, None);
    }

    #[test]
    fn the_request_names_copilot_and_the_terminal() {
        let r = request_for(Some("agentStop"), "t4", 9, &json!({"sessionId": "s1"})).unwrap();
        assert_eq!(
            serde_json::to_value(r).unwrap(),
            json!({"terminal_id":"t4","seq":9,"status":"idle","agent":"copilot","session_id":"s1"})
        );
        assert!(request_for(Some("preToolUse"), "t4", 9, &json!({})).is_none());
    }

    #[test]
    fn the_hook_survives_garbage_and_a_missing_server() {
        let nowhere = || PathBuf::from("/nonexistent/poimen/s.sock");
        run_hook(std::io::empty(), Some("stop".into()), None, nowhere());
        run_hook(
            std::io::Cursor::new(b"not json".to_vec()),
            Some("agentStop".into()),
            Some("t1".into()),
            nowhere(),
        );
        run_hook(
            std::io::Cursor::new(br#"{"sessionId":"x"}"#.to_vec()),
            Some("agentStop".into()),
            Some("t1".into()),
            nowhere(),
        );
    }

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("poimen-copilot-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(FILE_NAME)
    }

    fn cleanup(path: &Path) {
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn install_writes_one_entry_per_event_and_is_idempotent() {
        let path = temp("install");
        install(&path, Path::new("/opt/poimen-server")).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        let file: Value = serde_json::from_str(&first).unwrap();
        assert_eq!(file["version"], 1);
        assert_eq!(
            file["hooks"]["agentStop"][0],
            json!({"type": "command", "bash": "'/opt/poimen-server' hook copilot agentStop # poimen-agent-hook", "timeoutSec": 2})
        );
        assert_eq!(status(&path).unwrap(), EVENTS.to_vec());
        install(&path, Path::new("/opt/poimen-server")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        install(&path, Path::new("/b/it's/poimen-server")).unwrap();
        let moved = std::fs::read_to_string(&path).unwrap();
        assert!(moved.contains(r"'/b/it'\\''s/poimen-server' hook copilot sessionEnd"));
        cleanup(&path);
    }

    #[test]
    fn uninstall_removes_only_our_file() {
        let path = temp("uninstall");
        assert!(!uninstall(&path).unwrap());
        assert!(status(&path).unwrap().is_empty());
        install(&path, Path::new("/x/poimen-server")).unwrap();
        assert!(uninstall(&path).unwrap());
        assert!(!path.exists());
        assert!(!uninstall(&path).unwrap());
        cleanup(&path);
    }

    #[test]
    fn a_foreign_file_at_the_path_is_refused_and_left_untouched() {
        let path = temp("foreign");
        let theirs = r#"{"version":1,"hooks":{}}"#;
        std::fs::write(&path, theirs).unwrap();
        assert!(install(&path, Path::new("/x/p")).is_err());
        assert!(uninstall(&path).is_err());
        assert!(status(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), theirs);
        cleanup(&path);
    }
}
