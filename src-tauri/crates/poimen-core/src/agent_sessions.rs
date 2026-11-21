//! The earlier sessions of a directory (#986): Claude Code sessions are the `<session>.jsonl`
//! files of `~/.claude/projects/<dir>/`, Copilot CLI sessions the directories of
//! `~/.copilot/session-state/` whose `workspace.yaml` names the directory. A session whose file is
//! gone is not listed. Reads only; the files are the agents' own.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::{copilot, transcripts};

/// Sessions listed per directory (the newest by last activity).
pub const LIMIT: usize = 50;
/// Longest first prompt kept, in characters.
const PROMPT_CHARS: usize = 120;
/// Lines read looking for the first prompt.
const PROMPT_LINES: usize = 400;
/// Bytes read from the end of a transcript to find its last timestamp.
const TAIL_BYTES: u64 = 64 * 1024;

/// One earlier session of a directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    /// The harness id (`claude`, `copilot`) whose `resumeArgs` resume it.
    pub agent: String,
    pub session: String,
    /// ISO 8601 time of the first record, when known.
    pub started: Option<String>,
    /// ISO 8601 time of the last record, when known.
    pub last_activity: Option<String>,
    /// The first prompt, whitespace collapsed and shortened; empty when there is none.
    pub first_prompt: String,
}

/// The sessions of `cwd` for the agents in `agents` (`claude`, `copilot`), newest activity first,
/// at most [`LIMIT`].
pub fn list(home: &Path, cwd: &str, agents: &[String]) -> Vec<SessionInfo> {
    let wants = |id: &str| agents.iter().any(|a| a == id);
    let mut out = Vec::new();
    if wants("claude") {
        out.extend(claude_sessions(&transcripts::projects_root(home), cwd));
    }
    if wants("copilot") {
        out.extend(copilot_sessions(&copilot::sessions_root(home), cwd));
    }
    out.sort_by(|a, b| {
        let key = |s: &SessionInfo| s.last_activity.clone().or_else(|| s.started.clone());
        key(b).cmp(&key(a)).then_with(|| a.session.cmp(&b.session))
    });
    out.truncate(LIMIT);
    out
}

/// Collapses whitespace and cuts to [`PROMPT_CHARS`] characters (with `…`).
fn shorten(text: &str) -> String {
    let one: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= PROMPT_CHARS {
        return one;
    }
    let mut cut: String = one.chars().take(PROMPT_CHARS - 1).collect();
    cut.push('…');
    cut
}

fn claude_sessions(root: &Path, cwd: &str) -> Vec<SessionInfo> {
    let dir = root.join(transcripts::project_dir_name(cwd));
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .filter_map(|p| claude_session(&p, cwd))
        .collect()
}

fn claude_session(path: &Path, cwd: &str) -> Option<SessionInfo> {
    let session = path.file_stem()?.to_str()?.to_owned();
    if !transcripts::is_safe_session_id(&session) {
        return None;
    }
    let mut started = None;
    let mut prompt = None;
    let mut other_dir = false;
    let lines = BufReader::new(File::open(path).ok()?).lines();
    for line in lines.take(PROMPT_LINES).map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if started.is_none() {
            started = timestamp(&value);
        }
        // Two directories can share a name (`/a/b-c` and `/a/b/c`): the recorded cwd decides.
        if let Some(c) = value.get("cwd").and_then(Value::as_str)
            && c != cwd
            && prompt.is_none()
        {
            other_dir = true;
            break;
        }
        if prompt.is_none() {
            prompt = claude_prompt(&value);
        }
        if prompt.is_some() && started.is_some() {
            break;
        }
    }
    if other_dir {
        return None;
    }
    Some(SessionInfo {
        agent: "claude".into(),
        session,
        last_activity: last_timestamp(path).or_else(|| started.clone()),
        started,
        first_prompt: prompt.map(|p| shorten(&p)).unwrap_or_default(),
    })
}

fn timestamp(value: &Value) -> Option<String> {
    value.get("timestamp")?.as_str().map(str::to_owned)
}

/// The timestamp of the last record that has one, from the end of the file.
fn last_timestamp(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    file.seek(SeekFrom::Start(len.saturating_sub(TAIL_BYTES)))
        .ok()?;
    let mut tail = Vec::new();
    file.read_to_end(&mut tail).ok()?;
    String::from_utf8_lossy(&tail)
        .lines()
        .rev()
        .find_map(|l| timestamp(&serde_json::from_str::<Value>(l).ok()?))
}

/// The prompt a Claude Code record holds: a typed message, or a slash command with its arguments.
fn claude_prompt(value: &Value) -> Option<String> {
    let flag = |k: &str| value.get(k).and_then(Value::as_bool).unwrap_or(false);
    if value.get("type")?.as_str()? != "user" || flag("isSidechain") || flag("isMeta") {
        return None;
    }
    let content = value.get("message")?.get("content")?;
    let text = match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks.iter().find_map(|b| {
            (b.get("type")?.as_str()? == "text")
                .then(|| b.get("text")?.as_str().map(str::to_owned))?
        })?,
        _ => return None,
    };
    if let Some(name) = between(&text, "<command-name>", "</command-name>") {
        let args = between(&text, "<command-args>", "</command-args>").unwrap_or_default();
        return Some(format!("{name} {args}"));
    }
    // Reminders, local command output and the like are not what the user typed.
    (!text.trim_start().starts_with('<') && !text.trim().is_empty()).then_some(text)
}

fn between(text: &str, open: &str, close: &str) -> Option<String> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(text[start..end].trim().to_owned())
}

fn copilot_sessions(root: &Path, cwd: &str) -> Vec<SessionInfo> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| copilot_session(&p, cwd))
        .collect()
}

/// The value of `key:` in a flat `workspace.yaml`.
fn yaml_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let rest = l.strip_prefix(key)?.strip_prefix(':')?;
        let v = rest.trim().trim_matches(|c| c == '"' || c == '\'');
        (!v.is_empty()).then(|| v.to_owned())
    })
}

fn copilot_session(dir: &Path, cwd: &str) -> Option<SessionInfo> {
    let session = dir.file_name()?.to_str()?.to_owned();
    let events = dir.join(copilot::EVENTS);
    if !transcripts::is_safe_session_id(&session) || !events.is_file() {
        return None;
    }
    let yaml = std::fs::read_to_string(dir.join("workspace.yaml")).unwrap_or_default();
    let mut started = yaml_value(&yaml, "created_at");
    let mut recorded = yaml_value(&yaml, "cwd");
    let mut prompt = None;
    let lines = BufReader::new(File::open(&events).ok()?).lines();
    for line in lines.take(PROMPT_LINES).map_while(Result::ok) {
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let data = value.get("data");
        match value.get("type").and_then(Value::as_str) {
            Some("session.start") => {
                if recorded.is_none() {
                    recorded = data
                        .and_then(|d| d.get("context")?.get("cwd")?.as_str())
                        .map(str::to_owned);
                }
                if started.is_none() {
                    started = data
                        .and_then(|d| d.get("startTime")?.as_str())
                        .map(str::to_owned);
                }
            }
            Some("user.message") => {
                prompt = data.and_then(|d| d.get("content")?.as_str()).map(shorten);
                break;
            }
            _ => {}
        }
    }
    if recorded.as_deref() != Some(cwd) {
        return None;
    }
    Some(SessionInfo {
        agent: "copilot".into(),
        session,
        last_activity: yaml_value(&yaml, "updated_at").or_else(|| started.clone()),
        started,
        first_prompt: prompt.unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("poimen-sessions-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn claude_file(home: &Path, cwd: &str, session: &str, body: &str) {
        let dir = transcripts::projects_root(home).join(transcripts::project_dir_name(cwd));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{session}.jsonl")), body).unwrap();
    }

    fn copilot_dir(home: &Path, session: &str, yaml: &str, events: Option<&str>) {
        let dir = copilot::sessions_root(home).join(session);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("workspace.yaml"), yaml).unwrap();
        if let Some(e) = events {
            fs::write(dir.join("events.jsonl"), e).unwrap();
        }
    }

    const BOTH: [&str; 2] = ["claude", "copilot"];

    fn both() -> Vec<String> {
        BOTH.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn claude_session_has_times_and_the_first_typed_prompt() {
        let home = temp("claude");
        claude_file(
            &home,
            "/w/proj",
            "abc-1",
            concat!(
                r#"{"type":"session-owner","sessionId":"abc-1"}"#,
                "\n",
                r#"{"type":"user","isMeta":true,"message":{"content":"meta"},"timestamp":"2026-11-01T10:00:00Z","cwd":"/w/proj"}"#,
                "\n",
                r#"{"type":"user","message":{"content":"<system-reminder>x</system-reminder>"},"timestamp":"2026-11-01T10:00:01Z"}"#,
                "\n",
                r#"{"type":"user","message":{"content":[{"type":"text","text":"fix   the\nbug"}]},"timestamp":"2026-11-01T10:00:02Z"}"#,
                "\n",
                r#"{"type":"assistant","message":{"content":"ok"},"timestamp":"2026-11-01T11:30:00Z"}"#,
                "\n",
            ),
        );
        let got = list(&home, "/w/proj", &both());
        assert_eq!(
            got,
            [SessionInfo {
                agent: "claude".into(),
                session: "abc-1".into(),
                started: Some("2026-11-01T10:00:00Z".into()),
                last_activity: Some("2026-11-01T11:30:00Z".into()),
                first_prompt: "fix the bug".into(),
            }]
        );
    }

    #[test]
    fn a_slash_command_is_the_prompt_and_a_long_one_is_cut() {
        let home = temp("slash");
        claude_file(
            &home,
            "/w/p",
            "s1",
            r#"{"type":"user","message":{"content":"<command-name>/deliver</command-name>\n<command-args>12 --base x</command-args>"},"timestamp":"2026-11-01T10:00:00Z"}"#,
        );
        let long = "a ".repeat(200);
        claude_file(
            &home,
            "/w/p",
            "s2",
            &format!(
                r#"{{"type":"user","message":{{"content":"{long}"}},"timestamp":"2026-11-02T10:00:00Z"}}"#
            ),
        );
        let got = list(&home, "/w/p", &both());
        assert_eq!(got[0].session, "s2");
        assert_eq!(got[0].first_prompt.chars().count(), PROMPT_CHARS);
        assert!(got[0].first_prompt.ends_with('…'));
        assert_eq!(got[1].first_prompt, "/deliver 12 --base x");
    }

    #[test]
    fn a_session_of_another_directory_with_the_same_name_is_skipped() {
        let home = temp("clash");
        // `/w/b-c` and `/w/b/c` share the directory name `-w-b-c`.
        claude_file(
            &home,
            "/w/b-c",
            "mine",
            r#"{"type":"user","message":{"content":"hi"},"timestamp":"2026-11-01T10:00:00Z","cwd":"/w/b-c"}"#,
        );
        claude_file(
            &home,
            "/w/b-c",
            "other",
            r#"{"type":"user","message":{"content":"hi"},"timestamp":"2026-11-01T10:00:00Z","cwd":"/w/b/c"}"#,
        );
        let got = list(&home, "/w/b-c", &both());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].session, "mine");
    }

    #[test]
    fn copilot_sessions_are_filtered_by_directory_and_need_their_events_file() {
        let home = temp("copilot");
        let events = concat!(
            r#"{"type":"session.start","data":{"context":{"cwd":"/w/p"}}}"#,
            "\n",
            r#"{"type":"user.message","data":{"content":"plan it"}}"#,
            "\n"
        );
        copilot_dir(
            &home,
            "c-1",
            "id: c-1\ncwd: /w/p\ncreated_at: 2026-11-03T08:00:00Z\nupdated_at: 2026-11-03T09:00:00Z\n",
            Some(events),
        );
        copilot_dir(&home, "c-2", "id: c-2\ncwd: /w/other\n", Some(events));
        copilot_dir(&home, "c-gone", "id: c-gone\ncwd: /w/p\n", None);
        let got = list(&home, "/w/p", &both());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].agent, "copilot");
        assert_eq!(got[0].session, "c-1");
        assert_eq!(got[0].started.as_deref(), Some("2026-11-03T08:00:00Z"));
        assert_eq!(
            got[0].last_activity.as_deref(),
            Some("2026-11-03T09:00:00Z")
        );
        assert_eq!(got[0].first_prompt, "plan it");
    }

    #[test]
    fn only_the_asked_agents_are_scanned_and_newest_comes_first() {
        let home = temp("order");
        claude_file(
            &home,
            "/w/p",
            "old",
            r#"{"type":"user","message":{"content":"a"},"timestamp":"2026-01-01T00:00:00Z"}"#,
        );
        copilot_dir(
            &home,
            "new",
            "cwd: /w/p\nupdated_at: 2026-06-01T00:00:00Z\n",
            Some("{}\n"),
        );
        let ids = |agents: &[&str]| -> Vec<String> {
            let a: Vec<String> = agents.iter().map(|s| (*s).to_owned()).collect();
            list(&home, "/w/p", &a)
                .into_iter()
                .map(|s| s.session)
                .collect()
        };
        assert_eq!(ids(&["claude", "copilot"]), ["new", "old"]);
        assert_eq!(ids(&["claude"]), ["old"]);
        assert!(ids(&[]).is_empty());
    }

    #[test]
    fn a_missing_root_lists_nothing() {
        let home = temp("none");
        assert!(list(&home, "/w/p", &both()).is_empty());
    }
}
