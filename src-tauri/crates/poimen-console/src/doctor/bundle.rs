//! The diagnostics bundle of `poimen doctor --bundle <file>`: one JSON file the user can read
//! before sending it. It holds the report, the shape of `server.json` (ids and counts, no names,
//! directories, commands or agent messages) and the tail of the logs. Nothing is uploaded.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use super::{Inputs, report};

/// How much of the end of each log goes into the bundle.
pub const LOG_TAIL: u64 = 200 * 1024;
/// At most this many host logs (the newest by name).
const MAX_HOST_LOGS: usize = 20;

/// The last `max` bytes of `path`, from a line start, as text; `None` when it cannot be read.
pub fn tail(path: &Path, max: u64) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(max);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(max).read_to_end(&mut bytes).ok()?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if start > 0 {
        // Drop the line the cut went through.
        text = text
            .split_once('\n')
            .map_or(String::new(), |(_, r)| r.into());
    }
    Some(text)
}

/// The ids, counts and sizes of a `server.json` value; every name, directory, command, endpoint
/// and message is left out.
pub fn shape(state: &Value) -> Value {
    let list = |key: &str| {
        state
            .get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_owned);
    let spaces: Vec<Value> = list("spaces")
        .iter()
        .map(|s| {
            json!({
                "id": text(s, "id"),
                "tabs": s.get("tabs").and_then(Value::as_array).map_or(0, Vec::len),
            })
        })
        .collect();
    let tabs: Vec<Value> = list("tabs")
        .iter()
        .map(|t| {
            let mut leaves = Vec::new();
            if let Some(root) = t.get("root") {
                terminal_ids(root, &mut leaves);
            }
            json!({
                "tab_id": text(t, "tab_id"),
                "space_id": text(t, "space_id"),
                "parent": text(t, "parent"),
                "terminals": leaves,
            })
        })
        .collect();
    let terminals: Vec<Value> = list("terminals")
        .iter()
        .map(|t| {
            json!({
                "id": text(t, "id"),
                "space_id": text(t, "space_id"),
                "cols": t.get("cols"),
                "rows": t.get("rows"),
                "seq": t.get("seq"),
                "agent_status": t.get("agent").and_then(|a| text(a, "status")).filter(|s| {
                    s.len() <= 16 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                }),
                "hibernated": t.get("hibernated").is_some(),
            })
        })
        .collect();
    json!({
        "version": state.get("version"),
        "next_id": state.get("next_id"),
        "spaces": spaces,
        "tabs": tabs,
        "terminals": terminals,
    })
}

fn terminal_ids(node: &Value, out: &mut Vec<String>) {
    if let Some(id) = node.get("terminal_id").and_then(Value::as_str) {
        out.push(id.to_owned());
    }
    if let Some(children) = node.get("children").and_then(Value::as_array) {
        children.iter().for_each(|c| terminal_ids(c, out));
    }
}

/// Replaces the directories in `text` that name the user: the state directory and the home.
fn scrub(text: &str, state_dir: &Path, home: Option<&Path>) -> String {
    let mut text = text.to_owned();
    for (dir, label) in [(Some(state_dir), "<state>"), (home, "<home>")] {
        if let Some(dir) = dir.map(|d| d.display().to_string()).filter(|d| d.len() > 1) {
            text = text.replace(&dir, label);
        }
    }
    text
}

fn log(path: &Path, state_dir: &Path, home: Option<&Path>) -> Value {
    match tail(path, LOG_TAIL) {
        Some(text) => json!(scrub(&text, state_dir, home)),
        None => Value::Null,
    }
}

/// The bundle for `inputs`; `state_file` is the `server.json` to describe, `hosts` the directory
/// of the host logs.
pub fn build(inputs: &Inputs, state_file: &Path, hosts: &Path, home: Option<&Path>) -> Value {
    let dir = state_file.parent().unwrap_or_else(|| Path::new("."));
    let state = std::fs::read(state_file)
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .map(|v| shape(&v));
    let mut host_logs: Vec<PathBuf> = std::fs::read_dir(hosts)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "log"))
                .collect()
        })
        .unwrap_or_default();
    host_logs.sort();
    host_logs.truncate(MAX_HOST_LOGS);
    let host_logs: serde_json::Map<String, Value> = host_logs
        .iter()
        .filter_map(|p| {
            let id = p.file_stem()?.to_string_lossy().into_owned();
            Some((id, log(p, dir, home)))
        })
        .collect();
    json!({
        "bundle": 1,
        "report": serde_json::from_str::<Value>(&scrub(
            &report(inputs, false).to_json().to_string(),
            dir,
            home,
        ))
        .unwrap_or(Value::Null),
        "state": state,
        "logs": {
            "server.log": log(&dir.join("server.log"), dir, home),
            "hooks.log": log(&dir.join("hooks.log"), dir, home),
            "hosts": host_logs,
        },
    })
}

/// Writes `bundle` to a new file `out` (mode 0600 on Unix); an existing file is an error.
pub fn write(out: &Path, bundle: &Value) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(out).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            format!("{} exists already; it is not overwritten", out.display())
        } else {
            format!("could not create {}: {e}", out.display())
        }
    })?;
    let text = serde_json::to_string_pretty(bundle).map_err(|e| e.to_string())?;
    writeln!(file, "{text}").map_err(|e| format!("could not write {}: {e}", out.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_keeps_ids_and_drops_names_directories_and_commands() {
        let state = json!({
            "version": 1, "next_id": 9,
            "spaces": [{"id": "s1", "name": "secret-project", "cwd": "/home/me/secret", "tabs": ["t1"]}],
            "tabs": [{"tab_id": "t1", "space_id": "s1", "name": "my tab", "active_terminal": "x1",
                "root": {"split_id": "p1", "direction": "right", "ratio": 0.5, "children": [
                    {"terminal_id": "x1"}, {"terminal_id": "x2"}]}}],
            "terminals": [{"id": "x1", "space_id": "s1", "name": "claude", "cwd": "/home/me/secret",
                "program": "claude", "args": ["--token", "abc"], "cols": 80, "rows": 24,
                "endpoint": "/run/me/hosts/x1", "seq": 5,
                "agent": {"status": "working", "message": "fixing my secret"}}],
        });
        let text = shape(&state).to_string();
        for leak in [
            "secret", "/home", "my tab", "--token", "abc", "claude", "/run",
        ] {
            assert!(!text.contains(leak), "{leak} in {text}");
        }
        assert!(text.contains("\"x2\"") && text.contains("working"));
    }

    #[test]
    fn tail_starts_at_a_line_and_scrub_hides_directories() {
        let dir = std::env::temp_dir().join(format!("poimen-bundle-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("a.log");
        std::fs::write(&log, "first line\nsecond line\nthird\n").unwrap();
        assert_eq!(tail(&log, 12).unwrap(), "third\n");
        assert_eq!(
            tail(&log, 1000).unwrap(),
            "first line\nsecond line\nthird\n"
        );
        assert_eq!(
            scrub("at /a/b/c here", Path::new("/a/b"), None),
            "at <state>/c here"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("poimen-bundle-w-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("b.json");
        write(&out, &json!({"a": 1})).unwrap();
        let err = write(&out, &json!({"a": 2})).unwrap_err();
        assert!(err.contains("exists"), "{err}");
        assert!(std::fs::read_to_string(&out).unwrap().contains('1'));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
