//! The backend-neutral snapshot (#586): spaces hold tabs hold terminals, in one read. It is the
//! wire shape of the app's `snapshot` and `machine_snapshot` commands (read by `toSnapshot` in
//! `src/app/core/snapshot.ts`); every backend maps its own model onto it (poimen-server directly,
//! herdr's `workspace.list`/`tab.list`/`pane.list` in the app's `herdr.rs`).

use std::collections::BTreeMap;

use poimen_protocol::Node;
use serde::{Deserialize, Serialize};

/// What the agent of a terminal (or the most urgent one of a tab or space) is doing; `unknown`
/// without an agent, and for any status a backend reports that poimen does not know.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    Idle,
    Working,
    Blocked,
    Done,
    #[default]
    #[serde(other)]
    Unknown,
}

impl AgentStatus {
    /// Orders statuses for a tab or space row: blocked first, then working, done, idle.
    pub fn urgency(self) -> u8 {
        match self {
            AgentStatus::Blocked => 4,
            AgentStatus::Working => 3,
            AgentStatus::Done => 2,
            AgentStatus::Idle => 1,
            AgentStatus::Unknown => 0,
        }
    }

    /// The most urgent of `statuses`, `unknown` for none.
    pub fn most_urgent(statuses: impl IntoIterator<Item = AgentStatus>) -> AgentStatus {
        statuses
            .into_iter()
            .max_by_key(|s| s.urgency())
            .unwrap_or_default()
    }
}

/// Spaces, tabs and terminals of a backend in one read.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub spaces: Vec<Space>,
    pub tabs: Vec<Tab>,
    pub terminals: Vec<Terminal>,
}

impl Snapshot {
    /// The terminals of the space `space_id`.
    pub fn terminals_of<'a>(&'a self, space_id: &'a str) -> impl Iterator<Item = &'a Terminal> {
        self.terminals
            .iter()
            .filter(move |t| t.space_id == space_id)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Space {
    pub space_id: String,
    pub label: String,
    /// 1-based position in the backend's order.
    pub number: u32,
    /// The most urgent status of its terminals.
    pub agent_status: AgentStatus,
    /// The space's own directory (poimen-server persists it, so a space without terminals still
    /// has one); herdr workspaces have none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tab {
    pub tab_id: String,
    pub space_id: String,
    pub label: String,
    /// 1-based position within its space.
    pub number: u32,
    /// The most urgent status of its terminals.
    pub agent_status: AgentStatus,
    /// The split tree as poimen-server keeps it (#513); absent for a herdr tab and for a terminal
    /// without a leaf (an exited one), which is a tab of its own.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<Node>,
    /// The focused leaf of the tab, shared by every client (poimen-server).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_terminal: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Terminal {
    pub terminal_id: String,
    pub tab_id: String,
    pub space_id: String,
    /// The agent kind (`claude`, `copilot`, ...); absent for a plain terminal.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// The agent's own session (the transcript's session id in `value`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_session: Option<AgentSessionRef>,
    pub agent_status: AgentStatus,
    /// The latest notification text of the agent, with its time in epoch ms (poimen-server).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_message_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// Directory of the foreground process (herdr); follows `cd`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreground_cwd: Option<String>,
    /// The user-set name; absent while the terminal has its default name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Free-form tokens set by integrations (`$<name>` row tokens); absent when empty.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub tokens: BTreeMap<String, String>,
    /// Runs in a PTY host that a restarted poimen-server adopts (#604).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub survives_restart: bool,
    /// The program the terminal runs (poimen-server).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// Version of the PTY host the terminal runs in and of the bundled one (#606), set together.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_bundled_version: Option<String>,
    /// The terminal title without escape sequences (herdr).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_title_stripped: Option<String>,
    pub focused: bool,
}

impl Terminal {
    /// The directory to match the terminal by: the foreground process's, else its own.
    pub fn dir(&self) -> Option<&str> {
        [&self.foreground_cwd, &self.cwd]
            .into_iter()
            .filter_map(|d| d.as_deref())
            .find(|d| !d.is_empty())
    }
}

/// The session of an agent: `value` is the transcript's session id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSessionRef {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_snapshot_parses_and_serializes_in_the_neutral_shape() {
        let wire = json!({
            "spaces": [{"space_id": "s1", "label": "api", "number": 1,
                        "agent_status": "blocked", "cwd": "/work/api"}],
            "tabs": [{"tab_id": "tab1", "space_id": "s1", "label": "agent", "number": 1,
                      "agent_status": "blocked",
                      "root": {"kind": "split", "split_id": "split1", "direction": "right",
                               "ratio": 0.5, "children": [
                                   {"kind": "terminal", "terminal_id": "t1"},
                                   {"kind": "terminal", "terminal_id": "t2"}]},
                      "active_terminal": "t2"}],
            "terminals": [
                {"terminal_id": "t1", "tab_id": "tab1", "space_id": "s1", "agent": "claude",
                 "agent_session": {"value": "abc"}, "agent_status": "blocked",
                 "agent_message": "Allow Bash?", "agent_message_at": 1700, "cwd": "/work/api",
                 "label": "review", "tokens": {"deliver_step": "build"},
                 "survives_restart": true, "program": "/bin/zsh", "host_version": "0.15.0",
                 "host_bundled_version": "0.16.0", "focused": false},
                {"terminal_id": "t2", "tab_id": "tab1", "space_id": "s1",
                 "agent_status": "unknown", "cwd": "/work/api", "focused": true}
            ]
        });
        let snapshot: Snapshot = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(snapshot.spaces[0].cwd.as_deref(), Some("/work/api"));
        assert_eq!(
            snapshot.tabs[0].root.as_ref().unwrap().terminal_ids(),
            ["t1", "t2"]
        );
        assert_eq!(snapshot.terminals[0].agent_status, AgentStatus::Blocked);
        assert_eq!(snapshot.terminals[0].tokens["deliver_step"], "build");
        // Nothing is added or lost on the way back.
        assert_eq!(serde_json::to_value(&snapshot).unwrap(), wire);
    }

    #[test]
    fn missing_fields_take_their_defaults_and_empty_ones_are_left_out() {
        let snapshot: Snapshot =
            serde_json::from_value(json!({"terminals": [{"terminal_id": "t1"}]})).unwrap();
        assert!(snapshot.spaces.is_empty() && snapshot.tabs.is_empty());
        let t = &snapshot.terminals[0];
        assert_eq!(t.agent_status, AgentStatus::Unknown);
        assert!(!t.focused && !t.survives_restart && t.tokens.is_empty());
        assert_eq!(
            serde_json::to_value(t).unwrap(),
            json!({"terminal_id": "t1", "tab_id": "", "space_id": "",
                   "agent_status": "unknown", "focused": false})
        );
        assert_eq!(
            serde_json::to_value(Snapshot::default()).unwrap(),
            json!({"spaces": [], "tabs": [], "terminals": []})
        );
    }

    #[test]
    fn an_unknown_status_is_unknown() {
        let s: AgentStatus = serde_json::from_value(json!("thinking")).unwrap();
        assert_eq!(s, AgentStatus::Unknown);
        assert!(serde_json::from_value::<AgentStatus>(json!(3)).is_err());
    }

    #[test]
    fn the_most_urgent_status_wins() {
        use AgentStatus::*;
        assert_eq!(AgentStatus::most_urgent([Idle, Blocked, Working]), Blocked);
        assert_eq!(AgentStatus::most_urgent([Idle, Done]), Done);
        assert_eq!(AgentStatus::most_urgent([]), Unknown);
    }

    #[test]
    fn a_terminal_is_found_by_its_foreground_directory_first() {
        let t = |fg: Option<&str>, cwd: Option<&str>| Terminal {
            foreground_cwd: fg.map(str::to_owned),
            cwd: cwd.map(str::to_owned),
            ..Terminal::default()
        };
        assert_eq!(t(Some("/a/b"), Some("/a")).dir(), Some("/a/b"));
        assert_eq!(t(Some(""), Some("/a")).dir(), Some("/a"));
        assert_eq!(t(None, Some("")).dir(), None);
        assert_eq!(t(None, None).dir(), None);
    }
}
