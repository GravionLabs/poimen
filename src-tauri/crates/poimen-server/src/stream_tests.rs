use super::*;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, sync_channel};

fn stream() -> (TermStream, Arc<Mutex<Vec<Event>>>) {
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let s = TermStream::new(
        "t1",
        20,
        5,
        100,
        Arc::default(),
        Arc::new(move |e| sink.lock().unwrap().push(e)),
    );
    (s, events)
}

fn client(
    s: &TermStream,
    mode: AttachMode,
    take_over: bool,
    cap: usize,
) -> Result<(Attached, Receiver<Line>), (ErrorCode, String)> {
    let (tx, rx) = sync_channel(cap);
    let link = Arc::new(Link::new());
    s.attach("1", mode, take_over, tx, link).map(|a| (a, rx))
}

fn texts(rx: &Receiver<Line>) -> Vec<String> {
    rx.try_iter().map(|l| l.text.to_string()).collect()
}

#[test]
fn output_before_attach_is_in_the_snapshot_and_after_it_in_the_stream() {
    let (s, _) = stream();
    s.output(b"before");
    let (_, rx) = client(&s, AttachMode::Observe, false, 8).unwrap();
    s.output(b"after");
    let lines = texts(&rx);
    let attach: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(attach["result"]["seq"], 1);
    let snap =
        poimen_protocol::decode_bytes(attach["result"]["snapshot"].as_str().unwrap()).unwrap();
    assert!(String::from_utf8_lossy(&snap).contains("before"));
    assert!(!String::from_utf8_lossy(&snap).contains("after"));
    let record: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
    assert_eq!(record["data"]["seq"], 2);
    assert_eq!(
        poimen_protocol::decode_bytes(record["data"]["data"].as_str().unwrap()).unwrap(),
        b"after"
    );
}

#[test]
fn a_second_controller_needs_take_over_and_the_first_is_detached() {
    let (s, events) = stream();
    let (first, rx1) = client(&s, AttachMode::Control, false, 8).unwrap();
    let err = client(&s, AttachMode::Control, false, 8).unwrap_err();
    assert_eq!(err.0, ErrorCode::Conflict);
    let (second, _rx2) = client(&s, AttachMode::Control, true, 8).unwrap();
    assert!(!s.is_controller(first.client_id));
    assert!(s.is_controller(second.client_id));
    let last = rx1.try_iter().last().unwrap();
    assert!(last.last && last.text.contains("taken_over"));
    let events = events.lock().unwrap();
    assert_eq!(
        events.as_slice(),
        [
            Event::TerminalControllerAttached {
                terminal_id: "t1".into()
            },
            Event::TerminalControllerDetached {
                terminal_id: "t1".into()
            },
            Event::TerminalControllerAttached {
                terminal_id: "t1".into()
            },
        ]
    );
}

#[test]
fn observers_are_many_and_do_not_take_the_control() {
    let (s, _) = stream();
    let (_c, _rc) = client(&s, AttachMode::Control, false, 8).unwrap();
    let (a, _ra) = client(&s, AttachMode::Observe, false, 8).unwrap();
    let (b, _rb) = client(&s, AttachMode::Observe, true, 8).unwrap();
    assert!(!s.is_controller(a.client_id) && !s.is_controller(b.client_id));
    assert!(s.has_controller());
}

#[test]
fn a_full_client_is_dropped_and_the_others_keep_receiving() {
    let (s, events) = stream();
    let (slow, slow_rx) = client(&s, AttachMode::Control, false, 2).unwrap();
    let (_ok, ok_rx) = client(&s, AttachMode::Observe, false, 64).unwrap();
    for i in 0..5 {
        s.output(format!("{i}").as_bytes());
    }
    assert!(!s.is_controller(slow.client_id));
    assert_eq!(texts(&ok_rx).len(), 1 + 5);
    assert_eq!(slow_rx.try_iter().count(), 2);
    assert!(
        events
            .lock()
            .unwrap()
            .contains(&Event::TerminalControllerDetached {
                terminal_id: "t1".into()
            })
    );
}

#[test]
fn detaching_the_controller_is_reported_once() {
    let (s, events) = stream();
    let (c, _rx) = client(&s, AttachMode::Control, false, 8).unwrap();
    s.detach(c.client_id);
    s.detach(c.client_id);
    assert!(!s.has_controller());
    assert_eq!(events.lock().unwrap().len(), 2);
}

#[test]
fn exit_ends_the_streams_and_late_attachers_get_the_snapshot_and_the_exit() {
    let (s, _) = stream();
    let (_, rx) = client(&s, AttachMode::Observe, false, 8).unwrap();
    s.output(b"bye");
    s.exited(Some(3));
    let lines = texts(&rx);
    assert!(lines.last().unwrap().contains(r#""exit_code":3"#));
    let (_, late) = client(&s, AttachMode::Control, false, 8).unwrap();
    let lines = texts(&late);
    assert!(lines[0].contains("snapshot"));
    assert!(lines[1].contains("terminal.exited"));
    assert!(!s.has_controller());
}

#[test]
fn resize_is_recorded_with_a_sequence_number() {
    let (s, _) = stream();
    let (_, rx) = client(&s, AttachMode::Observe, false, 8).unwrap();
    s.resized(30, 9);
    let lines = texts(&rx);
    let r: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
    assert_eq!(r["event"], "terminal.resized");
    assert_eq!(
        (
            r["data"]["cols"].as_u64(),
            r["data"]["rows"].as_u64(),
            r["data"]["seq"].as_u64()
        ),
        (Some(30), Some(9), Some(1))
    );
}

/// A writer that collects what the stream answers.
#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn answering() -> (TermStream, Sink) {
    let (s, _) = stream();
    let sink = Sink::default();
    s.set_writer(PtyWriter::new(Box::new(sink.clone())));
    (s, sink)
}

fn answered(sink: &Sink) -> String {
    String::from_utf8_lossy(&sink.0.lock().unwrap()).into_owned()
}

#[test]
fn without_a_controller_queries_are_answered_on_the_pty() {
    let (s, sink) = answering();
    s.output(b"ab\x1b[6n\x1b[c\x1b[>c");
    let version = env!("CARGO_PKG_VERSION");
    assert!(
        answered(&sink).starts_with("\x1b[1;3R\x1b[?6c\x1b[>0;"),
        "{version}"
    );
}

#[test]
fn color_queries_use_the_colors_set_by_the_app() {
    let (s, sink) = answering();
    {
        let mut colors = s.colors.write().unwrap();
        colors.background = alacritty_terminal::vte::ansi::Rgb {
            r: 0x11,
            g: 0x22,
            b: 0x33,
        };
    }
    s.output(b"\x1b]11;?\x07");
    assert_eq!(answered(&sink), "\x1b]11;rgb:1111/2222/3333\x07");
}

#[test]
fn with_a_controller_the_server_stays_silent_and_answers_again_after_it_left() {
    let (s, sink) = answering();
    let (c, _rx) = client(&s, AttachMode::Control, false, 8).unwrap();
    s.output(b"\x1b[c\x1b]10;?\x07\x1b[6n");
    assert_eq!(answered(&sink), "");
    s.detach(c.client_id);
    s.output(b"\x1b[c");
    assert_eq!(answered(&sink), "\x1b[?6c");
}

#[test]
fn the_detection_query_is_answered_with_and_without_a_controller_once() {
    let (s, sink) = answering();
    s.output(b"\x1b]7501;?\x07\x1b[c");
    assert_eq!(
        answered(&sink),
        format!("{}\x1b[?6c", crate::emulator::DETECTION_REPLY)
    );
    let (c, _rx) = client(&s, AttachMode::Control, false, 8).unwrap();
    let before = answered(&sink).len();
    s.output(b"\x1b]7501;?\x07\x1b[c\x1b[6n");
    assert_eq!(&answered(&sink)[before..], crate::emulator::DETECTION_REPLY);
    s.detach(c.client_id);
}

#[test]
fn replayed_output_reaches_the_grid_and_the_clients_but_its_queries_get_no_answer() {
    let (s, sink) = answering();
    let (_, rx) = client(&s, AttachMode::Observe, false, 8).unwrap();
    s.replay(b"prompt$ \x1b[c\x1b[6n\x1b]7501;?\x07");
    assert_eq!(answered(&sink), "");
    let lines = texts(&rx);
    assert!(lines[1].contains("terminal.output"), "{lines:?}");
    let (_, late) = client(&s, AttachMode::Observe, false, 8).unwrap();
    let snap: serde_json::Value = serde_json::from_str(&texts(&late)[0]).unwrap();
    let snap = poimen_protocol::decode_bytes(snap["result"]["snapshot"].as_str().unwrap()).unwrap();
    assert!(String::from_utf8_lossy(&snap).contains("prompt$"));
    // Live output after the replay is answered again.
    s.output(b"\x1b[c");
    assert_eq!(answered(&sink), "\x1b[?6c");
}

#[test]
fn release_ends_every_attachment_without_a_record_and_frees_the_controller() {
    let (s, _) = stream();
    let (tx, rx) = sync_channel(8);
    let link = Arc::new(Link::new());
    s.attach("1", AttachMode::Control, false, tx, Arc::clone(&link))
        .unwrap();
    let _ = rx.try_recv(); // the attach response
    s.release();
    assert!(link.released.load(Ordering::SeqCst));
    assert!(!s.has_controller());
    let last = rx.try_recv().unwrap();
    assert!(last.last && last.text.is_empty());
    assert!(!link.overflowed.load(Ordering::SeqCst));
}

#[test]
fn replies_of_a_silent_period_are_not_delivered_late() {
    let (s, sink) = answering();
    let (c, _rx) = client(&s, AttachMode::Control, false, 8).unwrap();
    s.output(b"\x1b[c");
    s.detach(c.client_id);
    s.output(b"x");
    assert_eq!(answered(&sink), "");
}

#[test]
fn a_replay_hands_all_its_signals_over_in_one_call() {
    let (s, _events) = stream();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&calls);
    s.set_signal_sink(Arc::new(move |signals| sink.lock().unwrap().push(signals)));
    s.replay(b"\x1b]7501;state=working\x07$ \x1b]133;A\x07\x1b]7501;state=done\x07");
    let calls = calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].len(), 3);
    drop(calls);
}

fn output_lines(lines: &[String]) -> Vec<Vec<u8>> {
    lines
        .iter()
        .filter(|l| l.contains("terminal.output"))
        .map(|l| {
            let v: serde_json::Value = serde_json::from_str(l).unwrap();
            poimen_protocol::decode_bytes(v["data"]["data"].as_str().unwrap()).unwrap()
        })
        .collect()
}

#[test]
fn an_observer_whose_queue_overflows_is_thinned_and_resynced_not_detached() {
    let (s, _) = stream();
    let (tx, rx) = sync_channel(3);
    let link = Arc::new(Link::new());
    let a = s
        .attach("1", AttachMode::Observe, false, tx, Arc::clone(&link))
        .unwrap();
    // The attach response and two records fill the queue; the rest is dropped.
    for n in 0..10 {
        s.output(format!("line {n}\r\n").as_bytes());
    }
    assert!(link.lagging.load(Ordering::SeqCst));
    assert!(!link.overflowed.load(Ordering::SeqCst));
    // Still full: no resync yet.
    s.resync(a.client_id);
    assert!(link.lagging.load(Ordering::SeqCst));
    let queued = texts(&rx);
    assert_eq!(output_lines(&queued).len(), 2, "{queued:?}");
    // Drained: the resync is the size and the whole grid, one record.
    s.resync(a.client_id);
    assert!(!link.lagging.load(Ordering::SeqCst));
    let lines = texts(&rx);
    assert!(lines[0].contains("terminal.resized"), "{lines:?}");
    let outputs = output_lines(&lines);
    assert_eq!(outputs.len(), 1);
    assert!(outputs[0].starts_with(b"\x1bc"));
    let grid = String::from_utf8_lossy(&outputs[0]);
    assert!(grid.contains("line 0") && grid.contains("line 9"), "{grid}");
    // Live again.
    s.output(b"more");
    assert_eq!(output_lines(&texts(&rx)), [b"more".to_vec()]);
}

#[test]
fn the_controller_that_overflows_is_still_detached() {
    let (s, _) = stream();
    let (tx, _rx) = sync_channel(2);
    let link = Arc::new(Link::new());
    s.attach("1", AttachMode::Control, false, tx, Arc::clone(&link))
        .unwrap();
    for _ in 0..4 {
        s.output(b"x");
    }
    assert!(link.overflowed.load(Ordering::SeqCst));
    assert!(!link.lagging.load(Ordering::SeqCst));
    assert!(!s.has_controller());
}

#[test]
fn queries_in_output_dropped_for_a_lagging_observer_are_still_answered() {
    let (s, sink) = answering();
    let (tx, _rx) = sync_channel(1);
    let link = Arc::new(Link::new());
    s.attach("1", AttachMode::Observe, false, tx, Arc::clone(&link))
        .unwrap();
    s.output(b"a");
    assert!(link.lagging.load(Ordering::SeqCst));
    s.output(b"\x1b[c");
    assert_eq!(answered(&sink), "\x1b[?6c");
}

fn logged(name: &str) -> (TermStream, PathBuf, Arc<History>) {
    let dir = std::env::temp_dir().join(format!("poimen-stream-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let history = Arc::new(History::start(
        dir.join("history"),
        crate::history::Limits::default(),
        dir.join("server.log"),
    ));
    let (s, _) = stream();
    s.set_history(Arc::clone(&history));
    (s, dir.join("history").join("t1"), history)
}

#[test]
fn a_checkpoint_is_written_after_the_set_number_of_output_bytes_and_covers_the_seq() {
    let (s, path, history) = logged("interval");
    s.start_log();
    s.set_checkpoint_every(100);
    s.output_at(&[b'a'; 60], 1);
    s.output_at(&[b'b'; 60], 2);
    s.output_at(b"tail", 3);
    history.flush();
    let records = crate::history::read(&path);
    let checkpoints: Vec<u64> = records
        .iter()
        .filter_map(|r| match r {
            crate::history::Record::Checkpoint(c) => Some(c.seq),
            _ => None,
        })
        .collect();
    // The empty one at the start, one after 120 bytes.
    assert_eq!(checkpoints, [0, 2]);
    let rebuilt = crate::adopt::rebuild::rebuild(&path, (20, 5), 100).unwrap();
    assert_eq!(rebuilt.seq, 3);
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}

#[test]
fn a_replay_is_logged_with_its_seq_too() {
    let (s, path, history) = logged("replay");
    s.start_log();
    s.replay_at(b"ring", 5);
    s.checkpoint();
    history.flush();
    let rebuilt = crate::adopt::rebuild::rebuild(&path, (20, 5), 100).unwrap();
    assert_eq!(rebuilt.seq, 5);
    let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
}
