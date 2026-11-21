//! Backend-neutral agent types: the session of an agent whose transcript poimen can read, and what
//! happened to a prompt sent to an agent.

use serde::Serialize;

pub use poimen_core::transcripts::is_safe_session_id;

/// An agent whose session transcript poimen can read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptAgent {
    /// Claude Code: `~/.claude/projects/<cwd>/<session>.jsonl`.
    Claude,
    /// GitHub Copilot CLI: `~/.copilot/session-state/<session>/events.jsonl`.
    Copilot,
}

/// The agent session of a terminal and the directory the session was started in; the transcript
/// files are found from these.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSession {
    pub agent: TranscriptAgent,
    pub session: String,
    /// Empty when the backend reports none; only Claude Code needs it.
    pub cwd: String,
}

/// The session of the agent `agent` (as a backend names it) with the session id `session`, started
/// in `cwd`; `None` for an agent without a readable transcript (`codex`, ...), without a session
/// id or with an unsafe one, and for Claude Code without a directory.
pub fn session_of(agent: Option<&str>, session: Option<&str>, cwd: &str) -> Option<AgentSession> {
    let agent = match agent? {
        "claude" => TranscriptAgent::Claude,
        "copilot" => TranscriptAgent::Copilot,
        _ => return None,
    };
    let session = session?;
    let needs_cwd = agent == TranscriptAgent::Claude;
    (is_safe_session_id(session) && !(needs_cwd && cwd.is_empty())).then(|| AgentSession {
        agent,
        session: session.to_owned(),
        cwd: cwd.to_owned(),
    })
}

/// What happened to a prompt sent to an agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PromptOutcome {
    /// Submitted; the agent then reported `status` (`working` or `blocked`).
    Started { status: String },
    /// Submitted, but the agent showed no activity.
    Stalled,
    /// The agent waits at an approval or question dialog; nothing was sent.
    Blocked,
    /// Any other failure, with a message for the user.
    Failed { message: String },
}

/// The agent harnesses with availability (program found on the `PATH` of a child).
#[tauri::command]
pub(crate) async fn get_harnesses() -> Vec<poimen_core::harness::HarnessInfo> {
    let harnesses = crate::get_settings().harnesses;
    tauri::async_runtime::spawn_blocking(move || poimen_core::harness::with_availability(harnesses))
        .await
        .unwrap_or_default()
}

/// The earlier agent sessions of `dir` (#986) for the harnesses that can resume one (`resumeArgs`):
/// Claude Code and Copilot CLI, newest activity first.
#[tauri::command]
pub(crate) async fn list_agent_sessions(
    dir: String,
) -> Vec<poimen_core::agent_sessions::SessionInfo> {
    let Some(home) = crate::home_dir() else {
        return Vec::new();
    };
    let agents: Vec<String> = crate::settings::load(&home)
        .harnesses
        .into_iter()
        .filter(|h| !h.resume_args.is_empty())
        .map(|h| h.id)
        .collect();
    tauri::async_runtime::spawn_blocking(move || {
        poimen_core::agent_sessions::list(&home, &dir, &agents)
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn claude_and_copilot_sessions_are_read() {
        assert_eq!(
            session_of(Some("claude"), Some("abc-123"), "/home/u/proj"),
            Some(AgentSession {
                agent: TranscriptAgent::Claude,
                session: "abc-123".into(),
                cwd: "/home/u/proj".into()
            })
        );
        // Copilot sessions are found by id alone; Claude needs the directory.
        assert_eq!(
            session_of(Some("copilot"), Some("0b7e-c1"), "").map(|s| (s.agent, s.cwd)),
            Some((TranscriptAgent::Copilot, String::new()))
        );
        assert_eq!(session_of(Some("claude"), Some("abc-124"), ""), None);
    }

    #[test]
    fn other_agents_and_missing_sessions_have_none() {
        assert_eq!(session_of(Some("codex"), Some("zzz"), "/x"), None);
        assert_eq!(session_of(None, Some("zzz"), "/x"), None);
        assert_eq!(session_of(Some("claude"), None, "/x"), None);
    }

    #[test]
    fn session_ids_must_be_plain_file_names() {
        for bad in ["../x", "/abs", "", "a/b", "..", "a.b"] {
            assert_eq!(session_of(Some("claude"), Some(bad), "/x"), None, "{bad}");
        }
        let uuid = "0b7e1c52-3f4a-4d2e-9a61-5c8d7e2f1a90";
        assert_eq!(
            session_of(Some("claude"), Some(uuid), "/x")
                .unwrap()
                .session,
            uuid
        );
    }

    #[test]
    fn a_prompt_outcome_serializes_with_a_kind_tag() {
        let json = serde_json::to_value(PromptOutcome::Stalled).unwrap();
        assert_eq!(json, json!({"kind":"stalled"}));
    }
}
