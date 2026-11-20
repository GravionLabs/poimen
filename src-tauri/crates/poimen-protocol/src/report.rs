//! `terminal.report_done` (#980).

use serde::{Deserialize, Serialize};

/// The result an agent reported at its end (`terminal.report_done`, #980).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentReport {
    /// `succeeded`, `failed` or `cancelled`.
    pub outcome: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    /// When it was reported, milliseconds since the epoch.
    pub at: u64,
    /// The server typed the line about it into the starting agent (or there is none to tell).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub notified: bool,
}

/// Params of `terminal.report_done`: the agent of the terminal reports its result; the answer is
/// the stored [`AgentReport`]. `outcome` is `succeeded`, `failed` or `cancelled`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalReportDone {
    pub terminal_id: String,
    pub outcome: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
}
