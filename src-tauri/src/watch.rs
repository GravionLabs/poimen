//! Keeps one herdr event subscription alive and forwards events as invalidation signals.
//!
//! herdr requires a pane id for `pane.agent_status_changed` and rejects the whole
//! subscription when one referenced pane is gone, so the watcher resubscribes
//! whenever the pane set changes.

use std::time::Duration;

use serde_json::{Value, json};

use crate::herdr::{Client, Event};

const GLOBAL_EVENTS: &[&str] = &[
    "workspace.created",
    "workspace.updated",
    "workspace.renamed",
    "workspace.closed",
    "workspace.focused",
    "tab.created",
    "tab.closed",
    "tab.renamed",
    "tab.focused",
    "pane.created",
    "pane.closed",
    "pane.updated",
    "pane.focused",
    "pane.moved",
    "pane.exited",
    "pane.agent_detected",
];

pub fn subscriptions_for(pane_ids: &[String]) -> Vec<Value> {
    GLOBAL_EVENTS
        .iter()
        .map(|kind| json!({ "type": kind }))
        .chain(
            pane_ids
                .iter()
                .map(|id| json!({ "type": "pane.agent_status_changed", "pane_id": id })),
        )
        .collect()
}

/// Pane set changes invalidate the per-pane status subscriptions.
pub fn needs_resubscribe(kind: &str) -> bool {
    matches!(kind, "pane.created" | "pane.closed" | "pane.moved")
}

pub fn pane_ids(pane_list: &Value) -> Vec<String> {
    pane_list["panes"]
        .as_array()
        .map(|panes| {
            panes
                .iter()
                .filter_map(|pane| pane["pane_id"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Runs forever on the calling thread. `emit` receives herdr events plus the
/// synthetic `poimen.connected` / `poimen.disconnected` kinds.
pub fn run(client: Client, emit: impl Fn(&Event)) {
    let synthetic = |kind: &str, data: Value| Event {
        kind: kind.to_owned(),
        data,
    };
    let mut connected = None;
    loop {
        let result = client
            .request("pane.list", json!({}))
            .and_then(|list| client.subscribe(subscriptions_for(&pane_ids(&list))));
        match result {
            Ok(mut subscription) => {
                if connected != Some(true) {
                    emit(&synthetic("poimen.connected", Value::Null));
                }
                connected = Some(true);
                // Something may have changed while we were resubscribing.
                emit(&synthetic("poimen.resync", Value::Null));
                loop {
                    match subscription.next_event() {
                        Ok(event) => {
                            emit(&event);
                            if needs_resubscribe(&event.kind) {
                                break;
                            }
                        }
                        Err(err) => {
                            emit(&synthetic(
                                "poimen.resync",
                                json!({ "reason": err.to_string() }),
                            ));
                            break;
                        }
                    }
                }
            }
            Err(err) => {
                if connected != Some(false) {
                    emit(&synthetic(
                        "poimen.disconnected",
                        json!({ "reason": err.to_string() }),
                    ));
                }
                connected = Some(false);
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscribes_status_per_pane() {
        let subs = subscriptions_for(&["w1:p1".into(), "w1:p2".into()]);
        assert_eq!(subs.len(), GLOBAL_EVENTS.len() + 2);
        assert_eq!(
            subs.last().unwrap(),
            &json!({ "type": "pane.agent_status_changed", "pane_id": "w1:p2" })
        );
    }

    #[test]
    fn resubscribes_on_pane_set_changes_only() {
        assert!(needs_resubscribe("pane.created"));
        assert!(needs_resubscribe("pane.closed"));
        assert!(!needs_resubscribe("pane.agent_status_changed"));
        assert!(!needs_resubscribe("pane.updated"));
    }

    #[test]
    fn extracts_pane_ids() {
        let list = json!({ "panes": [{ "pane_id": "w1:p1" }, { "pane_id": "w1:pM" }, { "x": 1 }] });
        assert_eq!(pane_ids(&list), ["w1:p1", "w1:pM"]);
        assert!(pane_ids(&json!({})).is_empty());
    }
}
