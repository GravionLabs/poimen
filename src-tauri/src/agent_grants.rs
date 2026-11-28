//! The commands of the app's answers to agents: the confirmation of a write into another space
//! (ADR 0012, #615) and the scoped write grant of a lead agent (ADR 0018, #1211). The server holds
//! the agent's call until the answer arrives.

use poimen_client::protocol::{AgentGrantAnswer, AgentGrants};
use tauri::State;

use crate::{Shared, on_backend};

/// The user's answer to an agent that asks to write into another space (#615).
#[tauri::command]
pub async fn agent_confirm_answer(
    backend: State<'_, Shared>,
    id: String,
    approve: bool,
) -> Result<(), String> {
    on_backend(backend.inner(), move |b| {
        b.agent_confirm_answer(&id, approve)
    })
    .await
}

/// The user's answer to a lead agent that asks for a grant: no, or yes with fewer spaces, a lower
/// limit or without worktree starts; the server refuses a wider one.
#[tauri::command]
pub async fn agent_grant_answer(
    backend: State<'_, Shared>,
    id: String,
    approve: bool,
    spaces: Option<Vec<String>>,
    max_agents: Option<u32>,
    worktrees: Option<bool>,
) -> Result<(), String> {
    let answer = AgentGrantAnswer {
        id,
        approve,
        spaces,
        max_agents,
        worktrees,
    };
    on_backend(backend.inner(), move |b| b.agent_grant_answer(&answer)).await
}

/// Revokes an active grant; running agents are left alone.
#[tauri::command]
pub async fn agent_grant_revoke(backend: State<'_, Shared>, id: String) -> Result<(), String> {
    on_backend(backend.inner(), move |b| b.agent_grant_revoke(&id)).await
}

/// The active grants.
#[tauri::command]
pub async fn agent_grants(backend: State<'_, Shared>) -> Result<AgentGrants, String> {
    on_backend(backend.inner(), |b| b.agent_grants()).await
}
