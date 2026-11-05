//! Keeps one herdr event subscription alive and forwards events as invalidation signals.
//!
//! herdr requires a pane id for `pane.agent_status_changed` and rejects the whole
//! subscription when one referenced pane is gone, so the watcher resubscribes
//! whenever the pane set changes.

use std::time::Duration;

use serde_json::{Value, json};

use crate::herdr::{Client, Event};

/// A burst of pane changes (a layout template creating panes one by one) is answered by one
/// resubscribe: after the first trigger the watcher waits this long, then reads the pane list once.
/// Events of the burst that arrive meanwhile are covered by the `poimen.resync` after resubscribing.
const RESUBSCRIBE_DEBOUNCE: Duration = Duration::from_millis(400);

/// While panes close, a subscription naming a pane that just vanished is rejected. That is retried
/// with a fresh `pane.list` (after the debounce) this many times before it counts as a lost connection.
const SUBSCRIBE_ATTEMPTS: u32 = 3;

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

/// Lists the panes and subscribes; a failure is retried with a fresh pane list, `pause` between attempts.
fn subscribe_retrying<S, E>(
    mut attempt: impl FnMut() -> Result<S, E>,
    attempts: u32,
    mut pause: impl FnMut(),
) -> Result<S, E> {
    let mut tries = 1;
    loop {
        match attempt() {
            Err(_) if tries < attempts => {
                tries += 1;
                pause();
            }
            other => return other,
        }
    }
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
        let result = subscribe_retrying(
            || {
                client
                    .request("pane.list", json!({}))
                    .and_then(|list| client.subscribe(subscriptions_for(&pane_ids(&list))))
            },
            SUBSCRIBE_ATTEMPTS,
            || std::thread::sleep(RESUBSCRIBE_DEBOUNCE),
        );
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
                                std::thread::sleep(RESUBSCRIBE_DEBOUNCE);
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
    fn resubscribe_waits_for_a_burst_of_pane_changes() {
        // A 3-pane template creates its panes within the window: one resubscribe covers them.
        assert!(RESUBSCRIBE_DEBOUNCE >= Duration::from_millis(250));
    }

    #[test]
    fn rejected_subscription_is_retried_with_a_fresh_attempt() {
        let mut calls = 0;
        let mut pauses = 0;
        let result = subscribe_retrying(
            || {
                calls += 1;
                if calls < 3 {
                    Err("pane vanished")
                } else {
                    Ok(calls)
                }
            },
            SUBSCRIBE_ATTEMPTS,
            || pauses += 1,
        );
        assert_eq!(result, Ok(3));
        assert_eq!(pauses, 2);
    }

    #[test]
    fn persistent_failure_is_returned_after_the_attempts() {
        let mut calls = 0;
        let result: Result<(), &str> = subscribe_retrying(
            || {
                calls += 1;
                Err("down")
            },
            SUBSCRIBE_ATTEMPTS,
            || {},
        );
        assert_eq!(result, Err("down"));
        assert_eq!(calls, SUBSCRIBE_ATTEMPTS);
    }

    #[test]
    fn extracts_pane_ids() {
        let list = json!({ "panes": [{ "pane_id": "w1:p1" }, { "pane_id": "w1:pM" }, { "x": 1 }] });
        assert_eq!(pane_ids(&list), ["w1:p1", "w1:pM"]);
        assert!(pane_ids(&json!({})).is_empty());
    }
}
