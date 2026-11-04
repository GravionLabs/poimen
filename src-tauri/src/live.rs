//! Smoke tests against a running herdr server. They only read and observe; they
//! never take control of a pane. Run with `cargo test -- --ignored`.

use std::sync::mpsc;
use std::time::Duration;

use serde_json::json;

use crate::herdr::Client;
use crate::stream::{self, Mode, OpenRequest, Record, Sessions};
use crate::watch;

fn first_pane(client: &Client) -> String {
    let list = client.request("pane.list", json!({})).expect("pane.list");
    watch::pane_ids(&list)
        .into_iter()
        .next()
        .expect("herdr has at least one pane")
}

#[test]
#[ignore = "needs a running herdr server"]
fn snapshot_methods_answer() {
    let client = Client::from_env();
    for (method, key) in [
        ("workspace.list", "workspaces"),
        ("tab.list", "tabs"),
        ("pane.list", "panes"),
    ] {
        let result = client
            .request(method, json!({}))
            .unwrap_or_else(|e| panic!("{method}: {e}"));
        assert!(result[key].is_array(), "{method} returns {key}");
    }
}

#[test]
#[ignore = "needs a running herdr server"]
fn subscription_is_accepted() {
    let client = Client::from_env();
    let list = client.request("pane.list", json!({})).unwrap();
    client
        .subscribe(watch::subscriptions_for(&watch::pane_ids(&list)))
        .expect("subscription accepted");
}

#[test]
#[ignore = "needs a running herdr server"]
fn observe_stream_delivers_a_full_frame() {
    let client = Client::from_env();
    let target = first_pane(&client);
    let bin = stream::resolve_bin_from_env();
    let sessions = Sessions::default();
    let (tx, rx) = mpsc::channel();
    let id = sessions
        .open(
            OpenRequest {
                bin: &bin,
                mode: Mode::Observe,
                target: &target,
                cols: 80,
                rows: 24,
                takeover: false,
            },
            move |_, record| {
                let _ = tx.send(record);
            },
        )
        .expect("spawn observer");
    let record = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("a record within 5s");
    sessions.close(id);
    match record {
        Record::Frame {
            full,
            width,
            height,
            bytes,
            ..
        } => {
            assert!(full, "first frame is a full redraw");
            assert_eq!((width, height), (80, 24));
            assert!(!bytes.is_empty());
        }
        Record::Closed { reason, .. } => panic!("stream closed: {reason}"),
    }
}
