//! `agent.transcript` (#698): the agent transcript of a terminal, followed per client.

use std::collections::HashSet;

use poimen_protocol::{AgentTranscript, AgentTranscriptResult, TranscriptState};
use serde_json::json;

use crate::{ApiResult, Shared, lock, terminal_mut};

/// Most transcript readers kept; one per view that follows an agent, so this is never reached by
/// normal use. A reader past it replaces an arbitrary one, which then starts over (`fresh`).
const MAX_FOLLOWERS: usize = 64;

/// A transcript reader and the session it follows.
pub(crate) struct Followed {
    session: String,
    follow: poimen_core::transcripts::Follow,
}

/// `agent.transcript` (#698): the updates the agent transcript of a terminal gained since this
/// follower's last request, read from the files of this machine (`Config.home`) through
/// `poimen_core::transcripts`. The agent and session are those the hooks reported; Claude Code
/// also needs the terminal's directory. `not_found` for an unknown terminal; every other miss is
/// a `state` of the result, so a client can tell it from a failed request.
pub(crate) fn agent_transcript(shared: &Shared, p: AgentTranscript) -> ApiResult {
    use poimen_core::transcripts::{OpenError, open};
    let (agent, session, cwd, live) = {
        let mut inner = lock(&shared.inner);
        let entry = terminal_mut(&mut inner, &p.terminal_id)?;
        let agent = entry.info.agent.as_ref();
        let found = (
            agent.and_then(|a| a.agent.clone()),
            agent.and_then(|a| a.session_id.clone()),
            entry.info.cwd.clone(),
        );
        let live: HashSet<String> = inner.terminals.iter().map(|t| t.info.id.clone()).collect();
        (found.0, found.1, found.2, live)
    };
    let state = |state, reason: Option<String>| {
        Ok(json!(AgentTranscriptResult {
            state,
            reason,
            session: None,
            fresh: false,
            updates: Vec::new(),
        }))
    };
    let mut followers = lock(&shared.transcripts);
    followers.retain(|(terminal, _), _| live.contains(terminal));
    let key = (p.terminal_id, p.follower);
    let (Some(agent), Some(session)) = (agent, session) else {
        followers.remove(&key);
        return state(TranscriptState::NoAgent, None);
    };
    let fresh = p.restart || followers.get(&key).is_none_or(|f| f.session != session);
    if fresh {
        followers.remove(&key);
        let Some(home) = shared.home.as_deref() else {
            return state(TranscriptState::Missing, None);
        };
        match open(home, &agent, &session, &cwd) {
            Ok(follow) => {
                if followers.len() >= MAX_FOLLOWERS
                    && let Some(old) = followers.keys().next().cloned()
                {
                    followers.remove(&old);
                }
                followers.insert(
                    key.clone(),
                    Followed {
                        session: session.clone(),
                        follow,
                    },
                );
            }
            Err(OpenError::NoAgent) => return state(TranscriptState::NoAgent, None),
            Err(OpenError::Missing) => return state(TranscriptState::Missing, None),
            Err(OpenError::Unreadable(reason)) => {
                return state(TranscriptState::Unreadable, Some(reason));
            }
        }
    }
    let followed = followers.get_mut(&key).expect("inserted above or kept");
    let updates = followed
        .follow
        .refresh()
        .into_iter()
        .map(|u| json!(u))
        .collect();
    Ok(json!(AgentTranscriptResult {
        state: TranscriptState::Ok,
        reason: None,
        session: Some(session),
        fresh,
        updates,
    }))
}
