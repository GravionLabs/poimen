//! Wire types of what an agent writes through `poimen mcp` and `run_session` (ADR 0012, #615) and
//! the placement of a terminal's tab (ADR 0010, #744).

use serde::{Deserialize, Serialize};

/// What an agent writes through `poimen mcp` (ADR 0012, #615, #1181, #1183, #1184).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentWriteAction {
    /// Starts an agent in a terminal of the space.
    StartAgent,
    /// Types a prompt into the agent of a terminal.
    PromptAgent,
    /// Runs a command in a new shell pane.
    StartShell,
    /// Interrupts or closes a terminal.
    StopTerminal,
    /// Types into an existing terminal.
    WriteTerminal,
}

/// Params of `agent.confirm` (#615): an agent (the terminal `caller_terminal_id`) wants to write
/// into `space_id`, which is not its own space. The server asks the connected app and answers
/// when the user did, or after `wait_ms` (default and cap [`CONFIRM_WAIT_MS`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfirm {
    pub caller_terminal_id: String,
    pub action: AgentWriteAction,
    /// The space written into; the server resolves its name.
    pub space_id: String,
    /// The terminal prompted; absent for a start in a new terminal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
    /// The prompt or the command line, shown to the user as is.
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait_ms: Option<u64>,
}

/// How long `agent.confirm` waits for the user at most.
pub const CONFIRM_WAIT_MS: u64 = 60_000;

/// Why `agent.confirm` answered as it did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmReason {
    Approved,
    Declined,
    /// The user did not answer in time.
    Timeout,
    /// No app is subscribed, so nobody can be asked.
    NoApp,
}

/// Result of `agent.confirm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmResult {
    pub approved: bool,
    pub reason: ConfirmReason,
}

/// Params of `agent.confirm_answer`: the app's answer to an [`crate::Event::AgentConfirmRequested`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentConfirmAnswer {
    pub id: String,
    pub approve: bool,
}

/// Params of `agent.write_notice` (#615): an agent wrote through `poimen mcp`; the server tells
/// the apps ([`crate::Event::AgentWrote`]) so the notification center lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentWriteNotice {
    pub caller_terminal_id: String,
    pub action: AgentWriteAction,
    pub space_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
    pub text: String,
    /// The user approved it in the app; false for a write in the caller's own space.
    #[serde(default)]
    pub confirmed: bool,
}

/// The place of a terminal's tab in the app (ADR 0010).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    /// An editor tab.
    Editor,
    /// The terminal panel at the bottom.
    Panel,
}
