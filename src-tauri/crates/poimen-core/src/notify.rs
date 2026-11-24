//! `notify` in the user config (#1017): a command or a URL for the moments an agent is `blocked`
//! or `done`. The server runs it (so it works with the app closed), the app runs the same code
//! for the Test button.
//!
//! ```json
//! "notify": {
//!   "timeoutSeconds": 10,
//!   "blocked": { "command": ["ntfy", "publish", "mytopic", "{agent} in {space}: {text}"] },
//!   "done": { "url": "https://ntfy.sh/mytopic" }
//! }
//! ```
//!
//! `command` is an argv (no shell; `{agent}`, `{space}`, `{status}`, `{text}` and `{machine}` are
//! replaced inside each element), `url` gets a JSON POST of the same five fields. Both may be set.

use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

/// Default of `notify.timeoutSeconds`.
pub const DEFAULT_TIMEOUT_SECS: u64 = 10;
/// Upper bound of `notify.timeoutSeconds`.
pub const MAX_TIMEOUT_SECS: u64 = 120;

/// What to do for one status.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Hook {
    /// The argv to run, with placeholders.
    pub command: Option<Vec<String>>,
    /// The URL that gets the JSON POST.
    pub url: Option<String>,
}

impl Hook {
    pub fn is_empty(&self) -> bool {
        self.command.is_none() && self.url.is_none()
    }
}

/// The resolved `notify` setting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Notify {
    pub blocked: Hook,
    pub done: Hook,
    pub timeout_seconds: u64,
}

impl Default for Notify {
    fn default() -> Self {
        Self {
            blocked: Hook::default(),
            done: Hook::default(),
            timeout_seconds: DEFAULT_TIMEOUT_SECS,
        }
    }
}

impl Notify {
    /// The hook for `status` (`blocked` or `done`), `None` for any other or an empty one.
    pub fn hook_for(&self, status: &str) -> Option<&Hook> {
        let hook = match status {
            "blocked" => &self.blocked,
            "done" => &self.done,
            _ => return None,
        };
        (!hook.is_empty()).then_some(hook)
    }
}

/// The facts of one notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Message {
    pub agent: String,
    pub space: String,
    pub status: String,
    pub text: String,
    pub machine: String,
}

fn resolve_hook(value: &Value, key: &str, warnings: &mut Vec<String>) -> Hook {
    let mut hook = Hook::default();
    let Some(object) = value.as_object() else {
        warnings.push(format!("notify.{key} must be an object"));
        return hook;
    };
    match object.get("command") {
        None | Some(Value::Null) => {}
        Some(Value::Array(items)) => {
            let argv: Option<Vec<String>> = items
                .iter()
                .map(|i| i.as_str().map(str::to_owned))
                .collect();
            match argv.filter(|a| a.first().is_some_and(|p| !p.trim().is_empty())) {
                Some(argv) => hook.command = Some(argv),
                None => warnings.push(format!(
                    "notify.{key}.command must be an array of strings with the program first"
                )),
            }
        }
        Some(_) => warnings.push(format!(
            "notify.{key}.command must be an array of strings with the program first"
        )),
    }
    match object.get("url") {
        None | Some(Value::Null) => {}
        Some(Value::String(u)) if u.starts_with("http://") || u.starts_with("https://") => {
            hook.url = Some(u.clone());
        }
        Some(_) => warnings.push(format!("notify.{key}.url must be an http(s) URL")),
    }
    hook
}

/// Resolves the value of the `notify` key; an invalid field warns and is ignored.
pub fn resolve(value: &Value, warnings: &mut Vec<String>) -> Notify {
    let mut out = Notify::default();
    let Some(object) = value.as_object() else {
        warnings.push("notify must be an object".into());
        return out;
    };
    if let Some(v) = object.get("blocked") {
        out.blocked = resolve_hook(v, "blocked", warnings);
    }
    if let Some(v) = object.get("done") {
        out.done = resolve_hook(v, "done", warnings);
    }
    match object.get("timeoutSeconds") {
        None | Some(Value::Null) => {}
        Some(v) => match v.as_u64().filter(|n| (1..=MAX_TIMEOUT_SECS).contains(n)) {
            Some(n) => out.timeout_seconds = n,
            None => warnings.push(format!(
                "notify.timeoutSeconds {v} must be a whole number from 1 to {MAX_TIMEOUT_SECS}; using {DEFAULT_TIMEOUT_SECS}"
            )),
        },
    }
    out
}

/// The setting of a whole config `root`; absent is empty.
pub fn from_root(root: &Value, warnings: &mut Vec<String>) -> Notify {
    root.get("notify")
        .map_or_else(Notify::default, |v| resolve(v, warnings))
}

/// `text` with the placeholders replaced.
pub fn expand(text: &str, m: &Message) -> String {
    text.replace("{agent}", &m.agent)
        .replace("{space}", &m.space)
        .replace("{status}", &m.status)
        .replace("{text}", &m.text)
        .replace("{machine}", &m.machine)
}

/// The text of a `Test` notification.
pub fn test_message(status: &str, machine: &str) -> Message {
    Message {
        agent: "test".into(),
        space: "poimen".into(),
        status: status.into(),
        text: "This is a test notification from poimen".into(),
        machine: machine.into(),
    }
}

/// Runs `argv` with `stdin` written to it; kills it after `timeout`.
fn run_process(
    mut cmd: Command,
    stdin: Option<&str>,
    timeout: Duration,
    what: &str,
) -> Result<(), String> {
    cmd.stdout(Stdio::null())
        .stderr(Stdio::piped())
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    let mut child = cmd.spawn().map_err(|e| format!("{what}: {e}"))?;
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        // A program that ends before reading is judged by its exit status.
        let _ = pipe.write_all(text.as_bytes());
    }
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let mut buf = Vec::new();
                    let _ = std::io::Read::read_to_end(&mut e, &mut buf);
                    err = String::from_utf8_lossy(&buf).trim().to_owned();
                }
                return Err(if err.is_empty() {
                    format!("{what} failed: {status}")
                } else {
                    format!("{what} failed: {status}: {err}")
                });
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{what} timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("{what}: {e}")),
        }
    }
}

/// Runs the command and posts the URL of `hook` for `m`, each with `timeout`. Both are tried;
/// the errors are joined. Blocks up to twice the timeout: call it from a thread of its own.
pub fn run(hook: &Hook, m: &Message, timeout: Duration) -> Result<(), String> {
    let mut errors = Vec::new();
    if let Some(argv) = &hook.command {
        let args: Vec<String> = argv.iter().map(|a| expand(a, m)).collect();
        let mut cmd = crate::gui_path::command(&args[0]);
        cmd.args(&args[1..]);
        let what = format!("notify command {}", args[0]);
        if let Err(e) = run_process(cmd, None, timeout, &what) {
            errors.push(e);
        }
    }
    if let Some(url) = &hook.url {
        let body = serde_json::to_string(m).unwrap_or_default();
        let mut cmd = crate::gui_path::command("curl");
        cmd.args(["--silent", "--show-error", "--fail", "--max-time"])
            .arg(timeout.as_secs().max(1).to_string())
            .args([
                "--request",
                "POST",
                "--header",
                "Content-Type: application/json",
            ])
            .args(["--data-binary", "@-", "--", url]);
        if let Err(e) = run_process(
            cmd,
            Some(&body),
            timeout + Duration::from_secs(1),
            "notify url",
        ) {
            errors.push(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// The name of this machine for `{machine}`: the environment, else `/etc/hostname`.
pub fn machine_name() -> String {
    ["HOSTNAME", "COMPUTERNAME"]
        .iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.trim().is_empty()))
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg() -> Message {
        Message {
            agent: "claude".into(),
            space: "poimen".into(),
            status: "blocked".into(),
            text: "Allow Bash?".into(),
            machine: "box".into(),
        }
    }

    #[test]
    fn absent_is_empty_and_a_full_config_resolves() {
        assert_eq!(from_root(&json!({}), &mut Vec::new()), Notify::default());
        let mut w = Vec::new();
        let n = resolve(
            &json!({
                "timeoutSeconds": 5,
                "blocked": { "command": ["ntfy", "publish", "t", "{text}"] },
                "done": { "url": "https://ntfy.sh/t" }
            }),
            &mut w,
        );
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(n.timeout_seconds, 5);
        assert_eq!(
            n.hook_for("blocked").unwrap().command.as_ref().unwrap()[0],
            "ntfy"
        );
        assert_eq!(
            n.hook_for("done").unwrap().url.as_deref(),
            Some("https://ntfy.sh/t")
        );
        assert!(n.hook_for("working").is_none());
    }

    #[test]
    fn invalid_fields_warn_and_are_ignored() {
        let mut w = Vec::new();
        let n = resolve(
            &json!({
                "timeoutSeconds": 0,
                "blocked": { "command": "ntfy publish", "url": "ftp://x" },
                "done": 3
            }),
            &mut w,
        );
        assert_eq!(w.len(), 4, "{w:?}");
        assert!(n.hook_for("blocked").is_none() && n.hook_for("done").is_none());
        assert_eq!(n.timeout_seconds, DEFAULT_TIMEOUT_SECS);
        assert_eq!(resolve(&json!(true), &mut Vec::new()), Notify::default());
    }

    #[test]
    fn placeholders_are_replaced_inside_each_element_without_a_shell() {
        assert_eq!(
            expand("{agent}/{space}: {status} {text} @{machine}", &msg()),
            "claude/poimen: blocked Allow Bash? @box"
        );
    }

    #[test]
    fn the_json_body_has_the_five_fields() {
        let v: Value = serde_json::from_str(&serde_json::to_string(&msg()).unwrap()).unwrap();
        for k in ["agent", "space", "status", "text", "machine"] {
            assert!(v.get(k).is_some(), "{k}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_command_runs_with_the_text_and_a_failure_or_a_hang_is_an_error() {
        let dir = std::env::temp_dir().join(format!("poimen-notify-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("out");
        let ok = Hook {
            command: Some(vec![
                "sh".into(),
                "-c".into(),
                "printf '%s' \"$0\" > \"$1\"".into(),
                "{agent} {text}".into(),
                out.display().to_string(),
            ]),
            url: None,
        };
        run(&ok, &msg(), Duration::from_secs(5)).unwrap();
        assert_eq!(std::fs::read_to_string(&out).unwrap(), "claude Allow Bash?");
        let failing = Hook {
            command: Some(vec![
                "sh".into(),
                "-c".into(),
                "echo boom >&2; exit 3".into(),
            ]),
            url: None,
        };
        let e = run(&failing, &msg(), Duration::from_secs(5)).unwrap_err();
        assert!(e.contains("boom"), "{e}");
        let hang = Hook {
            command: Some(vec!["sleep".into(), "30".into()]),
            url: None,
        };
        let start = Instant::now();
        let e = run(&hang, &msg(), Duration::from_secs(1)).unwrap_err();
        assert!(e.contains("timed out"), "{e}");
        assert!(start.elapsed() < Duration::from_secs(10));
        let missing = Hook {
            command: Some(vec!["/nonexistent/poimen-notify".into()]),
            url: None,
        };
        assert!(run(&missing, &msg(), Duration::from_secs(1)).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }
}
