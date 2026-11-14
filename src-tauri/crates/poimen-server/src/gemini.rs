//! Gemini CLI integration: the hook that reports the agent state to the server, and the installer
//! that registers it in `~/.gemini/settings.json` next to whatever is there.
//!
//! The hook is `poimen-server hook gemini`: Gemini CLI (hooks, documented in its `hooks`
//! reference) runs it with the event as JSON on stdin (`session_id`, `transcript_path`, `cwd`,
//! `hook_event_name`, `timestamp`; a `Notification` adds `notification_type`, `message`). The
//! settings file has the same `hooks.<event>[].hooks[]` shape as Claude Code's, but its `timeout`
//! is in milliseconds. Transcripts of Gemini CLI are not read (no documented format), so the
//! kind gives the agent status only.

use std::path::{Path, PathBuf};

use poimen_protocol::{AgentStatus, Channel, TerminalReportAgent};
use serde_json::{Value, json};

use crate::claude::Report;
use crate::hook::{HookEnv, Outcome};

/// The hook events poimen registers.
pub const EVENTS: [&str; 6] = [
    "SessionStart",
    "BeforeAgent",
    "Notification",
    "AfterTool",
    "AfterAgent",
    "SessionEnd",
];
/// Milliseconds Gemini CLI waits for the hook.
const HOOK_TIMEOUT_MS: u64 = 2000;
const WINDOWS_HOOK_TIMEOUT_MS: u64 = 5000;

/// Maps a hook payload to a report: `SessionStart` idle, `AfterAgent` done (the turn finished),
/// `BeforeAgent`/`AfterTool` working, a `ToolPermission` `Notification` blocked, `SessionEnd`
/// gone. Everything else gives `None`.
pub fn map_hook(payload: &Value) -> Option<Report> {
    let status = match payload.get("hook_event_name")?.as_str()? {
        "SessionStart" => AgentStatus::Idle,
        "AfterAgent" => AgentStatus::Done,
        "BeforeAgent" | "AfterTool" => AgentStatus::Working,
        "Notification" => match payload.get("notification_type").and_then(Value::as_str) {
            Some("ToolPermission") => AgentStatus::Blocked,
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
    let message = (status == AgentStatus::Blocked)
        .then(|| {
            payload
                .get("message")
                .and_then(Value::as_str)
                .and_then(poimen_protocol::clean_message)
        })
        .flatten();
    Some(Report {
        status,
        session_id,
        message,
    })
}

/// The report request for a payload in the terminal `terminal_id`.
pub fn request_for(terminal_id: &str, seq: u64, payload: &Value) -> Option<TerminalReportAgent> {
    let report = map_hook(payload)?;
    Some(TerminalReportAgent {
        terminal_id: terminal_id.to_owned(),
        seq,
        status: report.status,
        agent: Some("gemini".into()),
        session_id: report.session_id,
        message: report.message,
    })
}

/// Runs the hook (see [`crate::hook::run`]).
pub fn run_hook(stdin: impl std::io::Read + Send + 'static, env: &HookEnv) -> Outcome {
    crate::hook::run(stdin, env, "hook gemini", request_for)
}

/// `<home>/.gemini/settings.json`.
pub fn default_settings_path(home: &Path) -> PathBuf {
    home.join(".gemini").join("settings.json")
}

/// Our hook entry (`timeout` in milliseconds; PowerShell command on Windows).
pub fn hook_entry(exe: &Path, windows: bool, channel: Channel) -> Value {
    let name = channel.named("poimen-agent-hook");
    if windows {
        json!({
            "name": name,
            "type": "command",
            "command": crate::claude::powershell_command(exe, "gemini", channel),
            "timeout": WINDOWS_HOOK_TIMEOUT_MS
        })
    } else {
        let quoted = exe.display().to_string().replace('\'', r"'\''");
        json!({
            "name": name,
            "type": "command",
            "command": format!("'{quoted}' hook gemini {}", crate::claude::marker(channel)),
            "timeout": HOOK_TIMEOUT_MS
        })
    }
}

/// Registers the hook for every event, keeping everything else. Idempotent.
pub fn install(settings_path: &Path, exe: &Path, channel: Channel) -> Result<(), String> {
    crate::claude::install_events(
        settings_path,
        &EVENTS,
        &hook_entry(exe, cfg!(windows), channel),
        channel,
    )
}

/// Removes only our entries; same file shape as Claude Code's. Delegating to
/// `claude::uninstall` is safe only because `settings_path` is Gemini's own file: the entries
/// are found by our marker, never by the file's name or location.
pub fn uninstall(settings_path: &Path, channel: Channel) -> Result<bool, String> {
    crate::claude::uninstall(settings_path, channel)
}

/// The events of [`EVENTS`] that have our hook with its binary present.
pub fn status(settings_path: &Path, channel: Channel) -> Result<Vec<&'static str>, String> {
    crate::claude::status_events(settings_path, &EVENTS, channel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st(p: Value) -> Option<AgentStatus> {
        map_hook(&p).map(|r| r.status)
    }

    fn event(name: &str) -> Value {
        json!({"session_id": "g1", "transcript_path": "/t", "cwd": "/w", "hook_event_name": name, "timestamp": "2026-11-14T10:00:00Z"})
    }

    #[test]
    fn events_map_onto_the_status() {
        let r = map_hook(&event("SessionStart")).unwrap();
        assert_eq!(
            (r.status, r.session_id.as_deref()),
            (AgentStatus::Idle, Some("g1"))
        );
        assert_eq!(st(event("BeforeAgent")), Some(AgentStatus::Working));
        assert_eq!(st(event("AfterTool")), Some(AgentStatus::Working));
        assert_eq!(st(event("AfterAgent")), Some(AgentStatus::Done));
        assert_eq!(st(event("SessionEnd")), Some(AgentStatus::Gone));
    }

    #[test]
    fn a_tool_permission_notification_blocks_with_its_cleaned_text() {
        let fixture: Value =
            serde_json::from_str(include_str!("../fixtures/gemini-notification.json")).unwrap();
        let r = map_hook(&fixture).unwrap();
        assert_eq!(r.status, AgentStatus::Blocked);
        assert_eq!(
            r.message.as_deref(),
            Some("Gemini wants to run: rm -rf build Allow?")
        );
        let mut other = fixture.clone();
        other["notification_type"] = json!("Other");
        assert_eq!(st(other), None);
        let mut done = event("AfterAgent");
        done["message"] = json!("x");
        assert_eq!(map_hook(&done).unwrap().message, None);
    }

    #[test]
    fn unknown_and_malformed_payloads_are_ignored() {
        assert_eq!(st(event("BeforeTool")), None);
        assert_eq!(st(event("PreCompress")), None);
        assert_eq!(st(json!({"session_id": "x"})), None);
        assert_eq!(st(json!("nope")), None);
        assert_eq!(st(json!({"hook_event_name": 3})), None);
        assert_eq!(map_hook(&event("SessionEnd")).unwrap().message, None);
    }

    #[test]
    fn the_request_names_gemini() {
        let r = request_for("t4", 9, &event("AfterAgent")).unwrap();
        assert_eq!(
            serde_json::to_value(r).unwrap(),
            json!({"terminal_id":"t4","seq":9,"status":"done","agent":"gemini","session_id":"g1"})
        );
        assert!(request_for("t4", 9, &event("BeforeTool")).is_none());
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-gemini-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("settings.json")
    }

    #[test]
    fn install_keeps_the_users_settings_and_is_idempotent() {
        let path = temp("install");
        let exe = path.with_file_name("poimen-server");
        std::fs::write(&exe, "").unwrap();
        let theirs = json!({"theme": "Dark", "hooks": {"AfterTool": [{"matcher": "*", "hooks": [{"type": "command", "command": "mine"}]}]}});
        std::fs::write(&path, theirs.to_string()).unwrap();
        install(&path, &exe, Channel::Stable).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();
        install(&path, &exe, Channel::Stable).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);
        let file: Value = serde_json::from_str(&first).unwrap();
        assert_eq!(file["theme"], "Dark");
        assert_eq!(file["hooks"]["AfterTool"].as_array().unwrap().len(), 2);
        assert_eq!(
            file["hooks"]["AfterAgent"][0]["hooks"][0],
            hook_entry(&exe, cfg!(windows), Channel::Stable)
        );
        assert_eq!(status(&path, Channel::Stable).unwrap(), EVENTS.to_vec());
        assert!(status(&path, Channel::Rc).unwrap().is_empty());
        assert!(uninstall(&path, Channel::Stable).unwrap());
        let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after, theirs);
        assert!(!uninstall(&path, Channel::Stable).unwrap());
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn the_entry_uses_milliseconds_and_a_quoted_command() {
        if !cfg!(windows) {
            let e = hook_entry(Path::new("/b/it's/p"), false, Channel::Stable);
            assert_eq!(e["timeout"], 2000);
            assert_eq!(
                e["command"],
                r"'/b/it'\''s/p' hook gemini # poimen-agent-hook"
            );
        }
        let w = hook_entry(Path::new(r"C:\P\p.exe"), true, Channel::Rc);
        assert_eq!(w["timeout"], 5000);
        assert_eq!(
            w["command"],
            r"& 'C:\P\p.exe' hook gemini # poimen-rc-agent-hook"
        );
    }

    #[test]
    fn a_settings_file_that_is_not_an_object_is_refused() {
        let path = temp("bad");
        std::fs::write(&path, "[1]").unwrap();
        assert!(install(&path, Path::new("/x/p"), Channel::Stable).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "[1]");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
