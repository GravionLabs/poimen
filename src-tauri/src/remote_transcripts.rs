//! Agent transcripts of the panes of a remote machine (#698). The transcript files are on the
//! machine, so its `poimen-server` reads them (`agent.transcript`) and this follows the answers:
//! one request at a time per follower, every [`INTERVAL`], each update handed to `emit` in the
//! shape of a local watch's `subagents` event. The server keeps the reader's position per
//! follower, so an update arrives once.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use poimen_client::protocol::TranscriptState;
use serde_json::Value;

use crate::backend::Backend;

/// Between two requests of a followed transcript. The machine's files cannot be watched from
/// here, so this is a poll over the connection that exists anyway; it runs only while a view
/// follows the pane.
pub const INTERVAL: Duration = Duration::from_millis(1500);

/// A running follow; dropping it stops the thread.
pub struct Poller {
    stop: Arc<AtomicBool>,
}

impl Drop for Poller {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// What starting a follow found.
pub enum Started {
    /// Following; the first updates were emitted already.
    Following(Poller),
    /// No transcript to follow (yet): no agent reported, or its file is not there. Nothing runs;
    /// the view asks again.
    NotYet,
}

/// Starts following the transcript of `terminal_id` (the machine's own id) on `backend` as
/// `follower`: the first request reads from the start. A failed request (the machine went away)
/// and an unreadable transcript are errors naming the reason; the poll itself rides out later
/// failures and stops when the server lost the reader (`fresh`), since its updates would repeat
/// what was sent.
pub fn start(
    backend: Arc<dyn Backend>,
    terminal_id: &str,
    follower: &str,
    interval: Duration,
    emit: impl Fn(&str, Value) + Send + 'static,
) -> Result<Started, String> {
    let first = backend.agent_transcript(terminal_id, follower, true)?;
    match first.state {
        TranscriptState::Ok => {}
        TranscriptState::Unreadable => {
            return Err(format!(
                "the agent transcript cannot be read: {}",
                first.reason.unwrap_or_default()
            ));
        }
        TranscriptState::NoAgent | TranscriptState::Missing => return Ok(Started::NotYet),
    }
    let session = first.session.unwrap_or_default();
    for update in first.updates {
        emit(&session, update);
    }
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let (terminal, follower) = (terminal_id.to_owned(), follower.to_owned());
    std::thread::spawn(move || {
        loop {
            // Short sleeps, so a dropped follow ends without waiting a whole interval.
            let mut waited = Duration::ZERO;
            while waited < interval && !flag.load(Ordering::Relaxed) {
                let step = Duration::from_millis(50).min(interval - waited);
                std::thread::sleep(step);
                waited += step;
            }
            if flag.load(Ordering::Relaxed) {
                return;
            }
            let Ok(next) = backend.agent_transcript(&terminal, &follower, false) else {
                continue;
            };
            if next.state != TranscriptState::Ok {
                continue;
            }
            if next.fresh {
                return;
            }
            let session = next.session.unwrap_or_default();
            for update in next.updates {
                emit(&session, update);
            }
        }
    });
    Ok(Started::Following(Poller { stop }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend_fake::FakeBackend;
    use poimen_client::protocol::AgentTranscriptResult;
    use serde_json::json;
    use std::sync::Mutex;
    use std::sync::mpsc;

    fn result(state: TranscriptState, updates: Vec<Value>) -> AgentTranscriptResult {
        AgentTranscriptResult {
            state,
            reason: (state == TranscriptState::Unreadable).then(|| "denied".to_owned()),
            session: (state == TranscriptState::Ok).then(|| "s1".to_owned()),
            fresh: false,
            updates,
        }
    }

    fn backend(answers: Vec<Result<AgentTranscriptResult, String>>) -> Arc<FakeBackend> {
        let fake = Arc::new(FakeBackend::default());
        fake.transcripts.lock().unwrap().extend(answers);
        fake
    }

    const FAST: Duration = Duration::from_millis(5);

    #[test]
    fn the_first_request_restarts_and_its_updates_are_emitted_then_the_poll_follows() {
        let fake = backend(vec![
            Ok(result(TranscriptState::Ok, vec![json!({"kind": "list"})])),
            Ok(result(TranscriptState::Ok, vec![json!({"kind": "main"})])),
        ]);
        let (tx, rx) = mpsc::channel();
        let started = start(fake.clone(), "t7", "panel", FAST, move |session, u| {
            tx.send((session.to_owned(), u)).unwrap();
        })
        .unwrap();
        assert!(matches!(started, Started::Following(_)));
        let wait = Duration::from_secs(5);
        assert_eq!(
            rx.recv_timeout(wait).unwrap(),
            ("s1".to_owned(), json!({"kind": "list"}))
        );
        assert_eq!(
            rx.recv_timeout(wait).unwrap(),
            ("s1".to_owned(), json!({"kind": "main"}))
        );
        let calls = fake.calls();
        assert_eq!(calls[0], "agent_transcript t7 panel true");
        assert_eq!(calls[1], "agent_transcript t7 panel false");
    }

    #[test]
    fn dropping_the_follow_stops_the_requests() {
        let fake = backend(vec![Ok(result(TranscriptState::Ok, vec![]))]);
        let Started::Following(poller) =
            start(fake.clone(), "t1", "panel", FAST, |_, _| {}).unwrap()
        else {
            panic!("expected a follow");
        };
        drop(poller);
        std::thread::sleep(Duration::from_millis(100));
        let seen = fake.calls().len();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(fake.calls().len(), seen);
    }

    #[test]
    fn no_agent_or_a_missing_file_starts_nothing() {
        for state in [TranscriptState::NoAgent, TranscriptState::Missing] {
            let fake = backend(vec![Ok(result(state, vec![]))]);
            let started = start(fake.clone(), "t1", "panel", FAST, |_, _| {}).unwrap();
            assert!(matches!(started, Started::NotYet));
            std::thread::sleep(Duration::from_millis(30));
            assert_eq!(fake.calls().len(), 1, "no poll after {state:?}");
        }
    }

    #[test]
    fn an_unreadable_transcript_and_a_failed_request_are_errors() {
        let fake = backend(vec![Ok(result(TranscriptState::Unreadable, vec![]))]);
        let err = start(fake, "t1", "panel", FAST, |_, _| {}).err().unwrap();
        assert_eq!(err, "the agent transcript cannot be read: denied");
        let fake = backend(vec![Err("mac is offline".into())]);
        let err = start(fake, "t1", "panel", FAST, |_, _| {}).err().unwrap();
        assert_eq!(err, "mac is offline");
    }

    #[test]
    fn a_failed_poll_is_ridden_out_and_a_lost_reader_ends_the_follow() {
        let mut lost = result(TranscriptState::Ok, vec![json!({"kind": "repeat"})]);
        lost.fresh = true;
        let fake = backend(vec![
            Ok(result(TranscriptState::Ok, vec![])),
            Err("connection reset".into()),
            Ok(result(TranscriptState::Ok, vec![json!({"kind": "main"})])),
            Ok(lost),
        ]);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let _poller = start(fake.clone(), "t1", "panel", FAST, move |_, u| {
            sink.lock().unwrap().push(u);
        })
        .unwrap();
        std::thread::sleep(Duration::from_millis(300));
        // The update after the failure arrived; the fresh answer was not emitted, and the poll
        // stopped after it.
        assert_eq!(*seen.lock().unwrap(), vec![json!({"kind": "main"})]);
        assert_eq!(fake.calls().len(), 4);
    }
}
