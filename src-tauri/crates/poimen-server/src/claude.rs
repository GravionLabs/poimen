//! Claude Code integration: the hook that reports the agent state to the server, and the
//! installer that registers it in `~/.claude/settings.json` next to whatever is there.
//!
//! The hook is `poimen-server hook claude`: Claude Code runs it with the event as JSON on stdin.
//! It does nothing outside a poimen terminal and never blocks or fails Claude.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use poimen_protocol::{AgentStatus, Channel, ENV_TERMINAL_ID, TerminalReportAgent};
use serde_json::{Map, Value, json};

use crate::hook::{HookEnv, Outcome};

/// Marks the command of our entries in `settings.json`; install and uninstall find them by it.
/// One per channel, so the hooks of a stable and an rc installation live side by side: neither
/// marker contains the other, and older stable installers look for the stable one only.
pub const fn marker(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => "# poimen-agent-hook",
        Channel::Rc => "# poimen-rc-agent-hook",
    }
}
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
/// On Windows: the hook runs through PowerShell, whose start alone can take a second.
pub(crate) const WINDOWS_HOOK_TIMEOUT_SECS: u64 = 5;

/// What one hook event says about the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub status: AgentStatus,
    pub session_id: Option<String>,
    /// Notification text (`message` of a permission `Notification`), cleaned and truncated.
    pub message: Option<String>,
}

/// Maps a hook payload to a report: `SessionStart` idle, `Stop` done (the turn finished), `UserPromptSubmit`/`PostToolUse`
/// working, a permission `Notification` blocked, `SessionEnd` gone. Events of subagents (the
/// payload has an `agent_id`) and everything else give `None`.
pub fn map_hook(payload: &Value) -> Option<Report> {
    if payload.get("agent_id").is_some_and(|v| !v.is_null()) {
        return None;
    }
    let status = match payload.get("hook_event_name")?.as_str()? {
        "SessionStart" => AgentStatus::Idle,
        "Stop" => AgentStatus::Done,
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
        agent: Some("claude".into()),
        session_id: report.session_id,
        message: report.message,
    })
}

/// Runs the hook: reads the payload from `stdin` and reports it (see [`crate::hook::run`]). It
/// never fails and gives up after a short time so Claude Code is never held up; a report that
/// does not get through is a line in the hook log. Outside a poimen terminal nothing happens.
pub fn run_hook(stdin: impl Read + Send + 'static, env: &HookEnv) -> Outcome {
    crate::hook::run(stdin, env, "hook claude", request_for)
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

/// The binary the hooks of `integration install` call: `given` ([`poimen_protocol::ENV_HOOK_EXE`])
/// when set, which must then be an absolute path to a file, else the running binary.
pub fn hook_exe(
    given: Option<std::ffi::OsString>,
    current: impl FnOnce() -> std::io::Result<PathBuf>,
) -> Result<PathBuf, String> {
    match given.filter(|g| !g.is_empty()).map(PathBuf::from) {
        Some(exe) if exe.is_absolute() && exe.is_file() => Ok(exe),
        Some(exe) => Err(format!(
            "{} is not an absolute path to a file: {}",
            poimen_protocol::ENV_HOOK_EXE,
            exe.display()
        )),
        None => current().map_err(|e| e.to_string()),
    }
}

/// The shell command of the hook for the binary at `exe`, marked for `channel`.
pub fn hook_command(exe: &Path, channel: Channel) -> String {
    let quoted = exe.display().to_string().replace('\'', r"'\''");
    format!("'{quoted}' hook claude {}", marker(channel))
}

/// The PowerShell command of the hook: `& '<exe>' hook <args> # poimen-agent-hook` (the call
/// operator runs a quoted path; `#` starts a comment there too, so the marker stays inert).
pub fn powershell_command(exe: &Path, args: &str, channel: Channel) -> String {
    format!(
        "& {} hook {args} {}",
        powershell_quote(&exe.display().to_string()),
        marker(channel)
    )
}

/// The binary a hook command of ours runs: the quoted path of [`hook_command`] (`'\''` for a
/// quote) or of [`powershell_command`] (doubled quotes). `None` for any other command.
pub fn hook_target(command: &str) -> Option<PathBuf> {
    let is_quote = |c: char| matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}');
    let mut path = String::new();
    if let Some(rest) = command.strip_prefix("& ") {
        let mut chars = rest.chars().peekable();
        if !chars.next().is_some_and(is_quote) {
            return None;
        }
        while let Some(c) = chars.next() {
            if !is_quote(c) {
                path.push(c);
            } else if chars.peek() == Some(&c) {
                chars.next();
                path.push(c);
            } else {
                return Some(PathBuf::from(path));
            }
        }
        None
    } else {
        let mut rest = command.strip_prefix('\'')?;
        loop {
            let end = rest.find('\'')?;
            path.push_str(&rest[..end]);
            match rest[end..].strip_prefix(r"'\''") {
                Some(more) => {
                    path.push('\'');
                    rest = more;
                }
                None => return Some(PathBuf::from(path)),
            }
        }
    }
}

/// A PowerShell single-quoted string: every single quote, including the typographic ones
/// PowerShell also accepts as quotes, is doubled.
pub fn powershell_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('\'');
    for c in text.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// Our hook entry in `settings.json`. Unix: a shell command. Windows: the same in PowerShell
/// (`"shell": "powershell"`; shell-form hooks would otherwise run in Git Bash when installed),
/// with a longer timeout for the PowerShell start.
pub fn hook_entry(exe: &Path, windows: bool, channel: Channel) -> Value {
    if windows {
        json!({
            "type": "command",
            "shell": "powershell",
            "command": powershell_command(exe, "claude", channel),
            "timeout": WINDOWS_HOOK_TIMEOUT_SECS
        })
    } else {
        json!({"type": "command", "command": hook_command(exe, channel), "timeout": HOOK_TIMEOUT_SECS})
    }
}

/// `<home>/.claude/settings.json`.
pub fn default_settings_path(home: &Path) -> PathBuf {
    home.join(".claude").join("settings.json")
}

fn is_ours(hook: &Value, channel: Channel) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains(marker(channel)))
}

/// Whether a command of ours still has its binary: an entry whose binary is gone (moved, or the
/// mount of an AppImage run that ended) does not count as installed. A command whose path cannot
/// be read back counts.
pub(crate) fn binary_present(command: &str) -> bool {
    hook_target(command).is_none_or(|exe| exe.is_file())
}

/// One of ours whose binary is there.
fn is_live(hook: &Value, channel: Channel) -> bool {
    is_ours(hook, channel)
        && hook
            .get("command")
            .and_then(Value::as_str)
            .is_some_and(binary_present)
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
fn strip(groups: &mut Vec<Value>, channel: Channel) -> bool {
    let mut removed = false;
    for group in groups.iter_mut() {
        if let Some(hooks) = group.get_mut("hooks").and_then(Value::as_array_mut) {
            let before = hooks.len();
            hooks.retain(|h| !is_ours(h, channel));
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
/// keeping everything else (the entries of the other channel too). Idempotent.
pub fn install(settings_path: &Path, exe: &Path, channel: Channel) -> Result<(), String> {
    install_events(
        settings_path,
        &EVENTS,
        &hook_entry(exe, cfg!(windows), channel),
        channel,
    )
}

/// [`install`] for any agent whose settings file has the `hooks.<event>[].hooks[]` shape (Claude
/// Code, Gemini CLI): `entry` under every one of `events`.
pub(crate) fn install_events(
    settings_path: &Path,
    events: &[&str],
    entry: &Value,
    channel: Channel,
) -> Result<(), String> {
    let mut settings = read_settings(settings_path)?;
    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("\"hooks\" in settings.json is not an object; left untouched")?;
    for event in events.iter().copied() {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| format!("hooks.{event} is not an array; left untouched"))?;
        strip(groups, channel);
        groups.push(json!({ "hooks": [entry.clone()] }));
    }
    write_atomic(settings_path, &settings)
}

/// Removes only our entries (and the event arrays or `hooks` object that this leaves empty).
/// Returns whether anything was removed. A missing file is left missing.
pub fn uninstall(settings_path: &Path, channel: Channel) -> Result<bool, String> {
    let mut settings = read_settings(settings_path)?;
    let mut removed = false;
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        for groups in hooks.values_mut().filter_map(Value::as_array_mut) {
            removed |= strip(groups, channel);
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

/// The events of [`EVENTS`] that have our hook, with its binary present, in `settings_path`.
pub fn status(settings_path: &Path, channel: Channel) -> Result<Vec<&'static str>, String> {
    status_events(settings_path, &EVENTS, channel)
}

/// [`status`] for the events `events` of any settings file of that shape.
pub(crate) fn status_events(
    settings_path: &Path,
    events: &[&'static str],
    channel: Channel,
) -> Result<Vec<&'static str>, String> {
    let settings = read_settings(settings_path)?;
    Ok(events
        .iter()
        .copied()
        .filter(|event| {
            settings
                .get("hooks")
                .and_then(|h| h.get(*event))
                .and_then(Value::as_array)
                .is_some_and(|groups| {
                    groups.iter().any(|g| {
                        g.get("hooks")
                            .and_then(Value::as_array)
                            .is_some_and(|h| h.iter().any(|hook| is_live(hook, channel)))
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
}
