//! Runs of the remote machines (#692): a machine's `poimen-server` forwards the progress events
//! of the run files in its spaces' repositories as `run.events` (asked for with `run_events` on
//! the machine's subscription, `machines.rs`). Here they are folded per machine, repository and
//! run with the same reducer as the local files (`poimen_core::agent_events`) into the `Read` of
//! `deliver.rs`, which goes to the frontend as a `deliver` event with the machine name.
//!
//! The reducer drops an event whose `seq` it has folded already, so the replay a machine sends
//! on every (re)connect adds only what is new: dedupe by `(run, seq)`. The runs of a machine stay
//! while it is offline (the frontend marks them); only a machine removed from the settings drops
//! them. The writer's pid is on the machine and cannot be checked here: a remote run without
//! `run.finished` is abandoned by the age rule alone.

use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;

use poimen_core::agent_events::Reducer;
use serde_json::{Value, json};

use crate::deliver::{self, Read};

/// The runs of the machines: per machine and repository root, a reducer per run id.
#[derive(Default)]
pub struct RemoteRuns {
    repos: Mutex<HashMap<(String, String), BTreeMap<String, Reducer>>>,
}

impl RemoteRuns {
    /// Folds the `data` of a `run.events` event of `machine` (`{root, run, events}`) and returns
    /// the payload of the frontend's `deliver` event for that repository, `{root, machine, read}`
    /// (`read` holds every run of the repository on the machine); `None` for malformed data.
    pub fn apply(&self, machine: &str, data: &Value, now: f64) -> Option<Value> {
        let root = data.get("root")?.as_str()?;
        let run = data.get("run")?.as_str()?;
        let events = data.get("events")?.as_array()?;
        let mut repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
        let runs = repos
            .entry((machine.to_owned(), root.to_owned()))
            .or_default();
        let reducer = runs.entry(run.to_owned()).or_default();
        for event in events.iter().filter(|e| e.is_object()) {
            reducer.push(event);
        }
        let mut unknown_version = None;
        let mut list = Vec::new();
        for (run, reducer) in runs.iter() {
            unknown_version = unknown_version.or(reducer.unknown_v);
            // The writer pid lives on the machine: only the age rule applies.
            list.extend(
                reducer
                    .snapshot(now, &|_| true)
                    .map(|r| deliver::named(r, run)),
            );
        }
        let read = Read::Events {
            runs: deliver::recent(list, now),
            unknown_version,
        };
        Some(json!({ "root": root, "machine": machine, "read": read }))
    }

    /// Drops the runs of the machines not among `names` (removed from the settings).
    pub fn retain(&self, names: &[&str]) {
        self.repos
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .retain(|(machine, _), _| names.contains(&machine.as_str()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poimen_core::agent_events::parse_ts;

    fn now() -> f64 {
        parse_ts("2026-11-13T10:30:00Z").unwrap()
    }

    fn event(seq: i64, ty: &str, extra: Value) -> Value {
        let mut e = json!({
            "v": 1,
            "seq": seq,
            "ts": format!("2026-11-13T10:0{seq}:00Z"),
            "run": "r1",
            "type": ty,
            "tone": "info",
        });
        e.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        e
    }

    fn data(root: &str, run: &str, events: Vec<Value>) -> Value {
        json!({ "root": root, "run": run, "events": events })
    }

    fn runs(payload: &Value) -> &Vec<Value> {
        assert_eq!(payload["read"]["kind"], "events");
        payload["read"]["runs"].as_array().unwrap()
    }

    #[test]
    fn a_replay_after_a_reconnect_adds_only_the_new_events() {
        let store = RemoteRuns::default();
        let first = vec![
            event(1, "run.started", json!({})),
            event(2, "progress", json!({"done": 1, "total": 3})),
        ];
        let p = store.apply("box", &data("/src/api", "r1", first.clone()), now());
        assert_eq!(p.as_ref().unwrap()["machine"], "box");
        assert_eq!(p.as_ref().unwrap()["root"], "/src/api");
        assert_eq!(
            runs(p.as_ref().unwrap())[0]["events"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        // The machine connects again and replays from the start, plus one new event.
        let mut replay = first;
        replay.push(event(3, "progress", json!({"done": 2, "total": 3})));
        let p = store
            .apply("box", &data("/src/api", "r1", replay), now())
            .unwrap();
        let run = &runs(&p)[0];
        let seqs: Vec<i64> = run["events"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["seq"].as_i64().unwrap())
            .collect();
        assert_eq!(seqs, [1, 2, 3]);
        assert_eq!(run["progress"]["done"], 2);
        assert_eq!(run["status"], "running");
    }

    #[test]
    fn the_same_run_id_on_two_machines_or_repositories_stays_two_runs() {
        let store = RemoteRuns::default();
        let started = vec![event(1, "run.started", json!({}))];
        store.apply("box", &data("/src/api", "r1", started.clone()), now());
        let finished = vec![
            event(1, "run.started", json!({})),
            event(2, "run.finished", json!({"outcome": "succeeded"})),
        ];
        let gpu = store
            .apply("gpu", &data("/src/api", "r1", finished), now())
            .unwrap();
        assert_eq!(runs(&gpu)[0]["status"], "finished");
        let other = store
            .apply("box", &data("/src/web", "r1", vec![]), now())
            .unwrap();
        assert_eq!(runs(&other).len(), 0);
        let again = store
            .apply("box", &data("/src/api", "r1", vec![]), now())
            .unwrap();
        assert_eq!(runs(&again)[0]["status"], "running");
    }

    #[test]
    fn several_runs_of_a_repository_come_in_one_read_and_the_file_names_a_run() {
        let store = RemoteRuns::default();
        store.apply(
            "box",
            &data("/r", "r1", vec![event(1, "run.started", json!({}))]),
            now(),
        );
        let nameless =
            json!({"v": 1, "seq": 1, "ts": "2026-11-13T10:09:00Z", "type": "run.started"});
        let p = store
            .apply("box", &data("/r", "b", vec![nameless]), now())
            .unwrap();
        let ids: Vec<&str> = runs(&p)
            .iter()
            .map(|r| r["run"].as_str().unwrap())
            .collect();
        // Newest first.
        assert_eq!(ids, ["b", "r1"]);
    }

    #[test]
    fn events_of_an_unknown_version_are_reported() {
        let store = RemoteRuns::default();
        let p = store
            .apply(
                "box",
                &data(
                    "/r",
                    "x",
                    vec![json!({"v": 9, "seq": 1, "type": "run.started"})],
                ),
                now(),
            )
            .unwrap();
        assert_eq!(p["read"]["unknownVersion"], 9);
        assert!(runs(&p).is_empty());
    }

    #[test]
    fn malformed_data_is_ignored() {
        let store = RemoteRuns::default();
        for bad in [
            json!(null),
            json!({"root": "/r", "run": "x"}),
            json!({"root": 1, "run": "x", "events": []}),
            json!({"run": "x", "events": []}),
        ] {
            assert_eq!(store.apply("box", &bad, now()), None, "{bad}");
        }
        // A line that is not an object is skipped, the rest folds.
        let p = store
            .apply(
                "box",
                &data(
                    "/r",
                    "x",
                    vec![json!(3), event(1, "run.started", json!({}))],
                ),
                now(),
            )
            .unwrap();
        assert_eq!(runs(&p).len(), 1);
    }

    #[test]
    fn a_removed_machine_loses_its_runs() {
        let store = RemoteRuns::default();
        store.apply(
            "box",
            &data("/r", "x", vec![event(1, "run.started", json!({}))]),
            now(),
        );
        store.apply(
            "gpu",
            &data("/r", "x", vec![event(1, "run.started", json!({}))]),
            now(),
        );
        store.retain(&["gpu"]);
        let p = store.apply("box", &data("/r", "x", vec![]), now()).unwrap();
        assert!(runs(&p).is_empty());
        let p = store.apply("gpu", &data("/r", "x", vec![]), now()).unwrap();
        assert_eq!(runs(&p).len(), 1);
    }
}
