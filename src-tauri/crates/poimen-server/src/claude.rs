//! Claude Code integration: the hook that reports the agent state to the server, and the
//! installer that registers it in `~/.claude/settings.json` next to whatever is there.
//!
//! The hook is `poimen-server hook claude`: Claude Code runs it with the event as JSON on stdin.
//! It does nothing outside a poimen terminal and never blocks or fails Claude.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
/// The hook events every Claude Code version knows; poimen always registers them.
pub const EVENTS: [&str; 6] = [
    "SessionStart",
    "UserPromptSubmit",
    "Notification",
    "PostToolUse",
    "Stop",
    "SessionEnd",
];
/// A CLI version `(major, minor, patch)`.
pub type Version = (u32, u32, u32);
/// Events of later Claude Code versions with the first version that knows them. A settings file
/// with an event the CLI does not know is rejected as a whole, so an event is written only when
/// the installed CLI is at least this version. Versions from the table of
/// stablyai/orca (`claude-hook-event-versions.ts`, 2026-11-19), which follows the release notes
/// of Claude Code and its hooks reference (code.claude.com/docs/en/hooks).
pub const LATER_EVENTS: [(&str, Version); 4] = [
    ("PermissionRequest", (2, 0, 45)),
    ("PostToolUseFailure", (2, 0, 56)),
    ("StopFailure", (2, 1, 78)),
    ("PostToolBatch", (2, 1, 118)),
];

/// The events to register for the installed CLI `cli`: the six of [`EVENTS`] and every one of
/// [`LATER_EVENTS`] that `cli` knows. An unknown version (`None`) gets the six.
pub fn events_for(cli: Option<Version>) -> Vec<&'static str> {
    EVENTS
        .into_iter()
        .chain(
            LATER_EVENTS
                .into_iter()
                .filter(|(_, since)| cli.is_some_and(|v| v >= *since))
                .map(|(name, _)| name),
        )
        .collect()
}

/// The version in the output of `claude --version`: `2.1.329 (Claude Code)`.
pub fn parse_version(output: &str) -> Option<Version> {
    let word = output.lines().next()?.split_whitespace().next()?;
    let mut parts = word.split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

/// Runs the installed agent CLI to learn its version; faked in tests.
pub trait CliVersion {
    /// The version of the CLI `program` (`claude`); `None` when it is not found or answers
    /// something else.
    fn version(&self, program: &str) -> Option<Version>;
}

/// Looks `program` up on the login-shell `PATH` and runs `<program> --version`, waiting at most
/// 5 s.
pub struct SystemCli;

impl CliVersion for SystemCli {
    fn version(&self, program: &str) -> Option<Version> {
        use poimen_core::gui_path;
        let exe = gui_path::resolve_program(program, &gui_path::child_path())?;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let out = gui_path::command(exe)
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .output()
                .ok();
            let _ = tx.send(out);
        });
        let out = rx.recv_timeout(Duration::from_secs(5)).ok()??;
        out.status
            .success()
            .then(|| parse_version(&String::from_utf8_lossy(&out.stdout)))
            .flatten()
    }
}

/// What a settings file has of the events an agent CLI needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookStatus {
    /// The events whose hook is there, with its binary present.
    pub installed: Vec<&'static str>,
    /// The events the installed CLI knows and that are not there.
    pub missing: Vec<&'static str>,
}

impl HookStatus {
    /// `expected` are the events the CLI needs; `installed` any events found.
    pub fn new(installed: Vec<&'static str>, expected: &[&'static str]) -> Self {
        let missing = expected
            .iter()
            .copied()
            .filter(|e| !installed.contains(e))
            .collect();
        Self { installed, missing }
    }
}
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
    /// The message is a stand-in (the tool of a `PermissionRequest`): the server keeps the text
    /// of a `Notification` that is already there.
    pub fallback_message: bool,
}

/// Maps a hook payload to a report: `SessionStart` idle, `Stop` done (the turn finished),
/// `UserPromptSubmit`/`PostToolUse`/`PostToolUseFailure`/`PostToolBatch` working (the last two
/// end a permission the user denied), a permission `Notification` or a `PermissionRequest`
/// blocked, `StopFailure` error, `SessionEnd` gone. Events of subagents (the payload has an
/// `agent_id`) and everything else give `None`.
pub fn map_hook(payload: &Value) -> Option<Report> {
    if payload.get("agent_id").is_some_and(|v| !v.is_null()) {
        return None;
    }
    let status = match payload.get("hook_event_name")?.as_str()? {
        "SessionStart" => AgentStatus::Idle,
        "Stop" => AgentStatus::Done,
        "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" | "PostToolBatch" => {
            AgentStatus::Working
        }
        "StopFailure" => AgentStatus::Error,
        "PermissionRequest" => AgentStatus::Blocked,
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
    let permission =
        payload.get("hook_event_name").and_then(Value::as_str) == Some("PermissionRequest");
    let message = if permission {
        permission_message(
            payload.get("tool_name").and_then(Value::as_str),
            payload.get("tool_input"),
        )
    } else {
        (status == AgentStatus::Blocked)
            .then(|| {
                payload
                    .get("message")
                    .and_then(Value::as_str)
                    .and_then(poimen_protocol::clean_message)
            })
            .flatten()
    };
    Some(Report {
        status,
        session_id,
        message,
        fallback_message: permission,
    })
}

/// The text of a permission request: the tool and a short form of its input, e.g.
/// `Bash: rm -rf build`. `None` without a tool name.
pub(crate) fn permission_message(tool: Option<&str>, input: Option<&Value>) -> Option<String> {
    let tool = tool.filter(|t| !t.trim().is_empty())?;
    let short = match input {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Object(map)) => [
            "command",
            "file_path",
            "path",
            "url",
            "pattern",
            "description",
        ]
        .iter()
        .find_map(|k| map.get(*k).and_then(Value::as_str))
        .map(str::to_owned)
        .or_else(|| (!map.is_empty()).then(|| Value::Object(map.clone()).to_string())),
        _ => None,
    };
    let text = match short {
        Some(short) if !short.trim().is_empty() => format!("{tool}: {short}"),
        _ => format!("{tool} needs your permission"),
    };
    poimen_protocol::clean_message(&text)
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
        message_fallback: report.fallback_message,
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

pub(crate) fn read_settings(path: &Path) -> Result<Map<String, Value>, String> {
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
pub(crate) fn write_atomic(path: &Path, settings: &Map<String, Value>) -> Result<(), String> {
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

/// Registers the hook for the events of [`events_for`]`(cli)` in `settings_path`, replacing
/// earlier entries of ours and keeping everything else (the entries of the other channel too).
/// Entries of ours for later events that this CLI does not know are removed (a CLI that was
/// downgraded would reject the file). Idempotent.
pub fn install(
    settings_path: &Path,
    exe: &Path,
    channel: Channel,
    cli: Option<Version>,
) -> Result<(), String> {
    let events = events_for(cli);
    let unknown: Vec<&str> = LATER_EVENTS
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !events.contains(name))
        .collect();
    install_events(
        settings_path,
        &events,
        &unknown,
        &hook_entry(exe, cfg!(windows), channel),
        channel,
    )
}

/// [`install`] for any agent whose settings file has the `hooks.<event>[].hooks[]` shape (Claude
/// Code, Gemini CLI): `entry` under every one of `events`, ours removed from `unknown`.
pub(crate) fn install_events(
    settings_path: &Path,
    events: &[&str],
    unknown: &[&str],
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
    for event in unknown {
        if let Some(groups) = hooks.get_mut(*event).and_then(Value::as_array_mut) {
            strip(groups, channel);
            if groups.is_empty() {
                hooks.remove(*event);
            }
        }
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

/// The events that have our hook, with its binary present, in `settings_path`, and the events
/// of [`events_for`]`(cli)` that are missing.
pub fn status(
    settings_path: &Path,
    channel: Channel,
    cli: Option<Version>,
) -> Result<HookStatus, String> {
    let all: Vec<&'static str> = EVENTS
        .into_iter()
        .chain(LATER_EVENTS.map(|(name, _)| name))
        .collect();
    let installed = status_events(settings_path, &all, channel)?;
    Ok(HookStatus::new(installed, &events_for(cli)))
}

/// The events of `events` that have our hook in any settings file of that shape.
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
#[path = "claude_tests.rs"]
mod tests;
