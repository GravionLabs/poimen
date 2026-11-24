//! `notify` (#1017): a command or a URL when an agent is `blocked` or `done`. The server runs it,
//! so it works with the app closed. At most one call per agent and transition: a status is
//! announced when it differs from the last one seen for the terminal. The call runs on a thread of
//! its own with the configured timeout, and a failure is one line in `server.log`; the server
//! never waits for it.

use std::collections::HashMap;
use std::thread;
use std::time::Duration;

use poimen_core::notify::{self, Message};
use poimen_core::settings::config_path;
use poimen_protocol::{AgentStatus, Event};

use crate::{Shared, lock};

/// The last status seen per terminal, to announce a transition once.
pub(crate) type Seen = HashMap<String, AgentStatus>;

fn word(status: AgentStatus) -> Option<&'static str> {
    match status {
        AgentStatus::Blocked => Some("blocked"),
        AgentStatus::Done => Some("done"),
        _ => None,
    }
}

impl Shared {
    /// Called for every published event (with the `inner` lock held): starts the notification of
    /// an agent that turned `blocked` or `done`. Takes no lock the caller could hold.
    pub(crate) fn notify_event(&self, event: &Event) {
        let Event::AgentStatusChanged {
            terminal_id,
            status,
            agent,
            message,
            ..
        } = event
        else {
            return;
        };
        let previous = lock(&self.notified).insert(terminal_id.clone(), *status);
        if previous == Some(*status) {
            return;
        }
        let Some(word) = word(*status) else {
            return;
        };
        let Some(shared) = self.me.get().and_then(std::sync::Weak::upgrade) else {
            return;
        };
        let terminal_id = terminal_id.clone();
        let (agent, text) = (agent.clone(), message.clone());
        thread::spawn(move || shared.notify_now(&terminal_id, word, agent, text));
    }

    fn notify_now(
        &self,
        terminal_id: &str,
        status: &str,
        agent: Option<String>,
        text: Option<String>,
    ) {
        let Some(home) = self.home.as_deref() else {
            return;
        };
        let Some(root) = std::fs::read_to_string(config_path(home))
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        else {
            return;
        };
        let setting = notify::from_root(&root, &mut Vec::new());
        let Some(hook) = setting.hook_for(status) else {
            return;
        };
        let (space, name) = {
            let inner = lock(&self.inner);
            let Some(t) = inner.terminals.iter().find(|t| t.info.id == terminal_id) else {
                return;
            };
            let space = inner
                .layout
                .spaces
                .iter()
                .find(|s| s.id == t.info.space_id)
                .map_or_else(String::new, |s| s.name.clone());
            (space, t.info.name.clone())
        };
        let message = Message {
            agent: agent.unwrap_or(name),
            space,
            status: status.to_owned(),
            text: text.unwrap_or_default(),
            machine: notify::machine_name(),
        };
        let timeout = Duration::from_secs(setting.timeout_seconds);
        if let Err(e) = notify::run(hook, &message, timeout) {
            self.log("notify", &format!("{status} of {terminal_id}: {e}"));
        }
    }
}
