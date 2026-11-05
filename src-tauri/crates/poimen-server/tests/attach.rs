//! `terminal.attach` end to end: snapshot, stream, roles, backpressure.
#![cfg(unix)]

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::protocol::{
    AttachMode, DetachReason, ErrorCode, Event, SetColors, TerminalCreate, decode_bytes,
};
use poimen_client::{Attachment, Client, ClientError};
use poimen_server::{Config, Server};

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-at-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self { dir }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

struct Fixture {
    _env: Env,
    client: Client,
    space: String,
}

fn fixture(name: &str) -> Fixture {
    let env = Env::new(name);
    let config = Config {
        socket: env.dir.join("run").join("server.sock"),
        state_file: env.dir.join("state").join("server.json"),
        scrollback: 1000,
    };
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let work = env.dir.join("work").canonicalize().unwrap();
    let space = client
        .create_space("work", &work.display().to_string())
        .unwrap()
        .id;
    Fixture {
        _env: env,
        client,
        space,
    }
}

impl Fixture {
    /// A terminal running `script` with `sh -c`.
    fn run(&self, script: &str) -> String {
        self.client
            .create_terminal(&TerminalCreate {
                space_id: self.space.clone(),
                command: Some(vec!["sh".into(), "-c".into(), script.into()]),
                ..Default::default()
            })
            .unwrap()
            .id
    }

    fn attach(&self, id: &str, mode: AttachMode) -> Attachment {
        let a = self.client.attach(id, mode, false).unwrap();
        a.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        a
    }

    fn events(&self) -> Receiver<Event> {
        let stream = self.client.subscribe().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for event in stream.flatten() {
                if tx.send(event).is_err() {
                    return;
                }
            }
        });
        rx
    }
}

fn wait_for_event(rx: &Receiver<Event>, pred: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(left).expect("event did not arrive");
        if pred(&event) {
            return event;
        }
    }
}

fn snapshot_text(a: &Attachment) -> String {
    String::from_utf8_lossy(&decode_bytes(&a.info.snapshot).unwrap()).into_owned()
}

/// Reads records until `pred` accepts one. `out` collects the output bytes, which `pred` also
/// sees (those read so far).
fn read_until(
    a: &mut Attachment,
    out: &mut Vec<u8>,
    mut pred: impl FnMut(&Event, &[u8]) -> bool,
) -> Event {
    loop {
        let record = a
            .next_record()
            .expect("stream ended before the record came");
        if let Event::TerminalOutput { data, .. } = &record {
            out.extend(decode_bytes(data).unwrap());
        }
        if pred(&record, out) {
            return record;
        }
    }
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    String::from_utf8_lossy(haystack).contains(needle)
}

fn wait_for_snapshot(f: &Fixture, id: &str, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let probe = f.client.attach(id, AttachMode::Observe, false).unwrap();
        if snapshot_text(&probe).contains(needle) {
            return;
        }
        assert!(Instant::now() < deadline, "{needle} never reached the grid");
        thread::sleep(Duration::from_millis(30));
    }
}

#[test]
fn output_written_before_attach_is_in_the_snapshot() {
    let f = fixture("before");
    let id = f.run("printf 'written-before'; sleep 30");
    wait_for_snapshot(&f, &id, "written-before");
    let a = f.attach(&id, AttachMode::Observe);
    assert!(snapshot_text(&a).contains("written-before"));
    assert_eq!((a.info.cols, a.info.rows), (80, 24));
}

#[test]
fn snapshot_plus_stream_cover_every_line_exactly_once() {
    let f = fixture("cover");
    let id = f.run(
        "i=0; while [ $i -lt 400 ]; do printf 'line%d\\r\\n' $i; i=$((i+1)); sleep 0.005; done; sleep 30",
    );
    wait_for_snapshot(&f, &id, "line20");
    let mut a = f.attach(&id, AttachMode::Observe);
    let snapshot = snapshot_text(&a);
    let mut last_seq = a.info.seq;
    let mut stream = Vec::new();
    read_until(&mut a, &mut stream, |e, out| {
        if let Event::TerminalOutput { seq, .. } = e {
            assert_eq!(
                *seq,
                last_seq + 1,
                "sequence numbers are contiguous from the snapshot"
            );
            last_seq = *seq;
        }
        contains(out, "line399")
    });
    let text = format!("{snapshot}{}", String::from_utf8_lossy(&stream));
    let numbers: Vec<u32> = text
        .split("line")
        .skip(1)
        .filter_map(|rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits.parse().ok()
        })
        .collect();
    assert!(numbers.len() > 20, "{numbers:?}");
    for pair in numbers.windows(2) {
        assert_eq!(pair[1], pair[0] + 1, "lines {pair:?} are not consecutive");
    }
    assert_eq!(*numbers.last().unwrap(), 399);
}

#[test]
fn the_snapshot_carries_the_terminal_modes() {
    let f = fixture("modes");
    let id = f.run("printf '\\033[?1049h\\033[?1006h\\033[?2004h\\033]2;build\\007'; sleep 30");
    wait_for_snapshot(&f, &id, "\u{1b}[?1049h");
    let text = snapshot_text(&f.attach(&id, AttachMode::Observe));
    for expected in [
        "\u{1b}[?1049h",
        "\u{1b}[?1006h",
        "\u{1b}[?2004h",
        "\u{1b}]2;build",
    ] {
        assert!(text.contains(expected), "{expected:?} missing in {text:?}");
    }
}

#[test]
fn only_the_controller_sends_input_and_sets_the_size() {
    let f = fixture("roles");
    let id = f.run("cat");
    let mut controller = f.attach(&id, AttachMode::Control);
    let mut observer = f.attach(&id, AttachMode::Observe);

    for result in [observer.input("nope\n"), observer.resize(50, 10)] {
        assert!(
            matches!(
                result,
                Err(ClientError::Server {
                    code: ErrorCode::NotController,
                    ..
                })
            ),
            "{result:?}"
        );
    }
    // A short-lived connection may not resize while someone controls.
    assert!(matches!(
        f.client.resize(&id, 50, 10),
        Err(ClientError::Server {
            code: ErrorCode::NotController,
            ..
        })
    ));

    controller.input("hello\n").unwrap();
    controller.resize(100, 30).unwrap();
    for a in [&mut controller, &mut observer] {
        let mut out = Vec::new();
        read_until(a, &mut out, |e, _| {
            matches!(
                e,
                Event::TerminalResized {
                    cols: 100,
                    rows: 30,
                    ..
                }
            )
        });
    }
    let listed = f.client.list_terminals(None).unwrap();
    assert_eq!((listed[0].cols, listed[0].rows), (100, 30));
    // The echo of the controller's input reaches the observer too, the refused input never
    // reached the terminal.
    let mut out = Vec::new();
    read_until(&mut observer, &mut out, |_, out| contains(out, "hello"));
    assert!(!contains(&out, "nope"));
}

#[test]
fn without_a_controller_a_short_lived_connection_may_resize() {
    let f = fixture("noctl");
    let id = f.run("sleep 30");
    f.client.resize(&id, 90, 20).unwrap();
    let a = f.attach(&id, AttachMode::Observe);
    assert_eq!((a.info.cols, a.info.rows), (90, 20));
}

#[test]
fn taking_over_the_control_is_explicit_and_reported() {
    let f = fixture("takeover");
    let id = f.run("sleep 30");
    let events = f.events();
    let mut first = f.attach(&id, AttachMode::Control);
    wait_for_event(&events, |e| {
        matches!(e, Event::TerminalControllerAttached { .. })
    });

    match f.client.attach(&id, AttachMode::Control, false) {
        Err(ClientError::Server { code, .. }) => assert_eq!(code, ErrorCode::Conflict),
        other => panic!("{:?}", other.map(|a| a.info)),
    }
    let mut second = f.client.attach(&id, AttachMode::Control, true).unwrap();
    second
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();

    let mut out = Vec::new();
    let detached = read_until(&mut first, &mut out, |e, _| {
        matches!(e, Event::TerminalDetached { .. })
    });
    assert_eq!(
        detached,
        Event::TerminalDetached {
            terminal_id: id.clone(),
            reason: DetachReason::TakenOver
        }
    );
    // The old controller lost its rights, the new one has them.
    assert!(first.input("x").is_err());
    second.input("true\n").unwrap();
    wait_for_event(&events, |e| {
        matches!(e, Event::TerminalControllerDetached { .. })
    });
    wait_for_event(&events, |e| {
        matches!(e, Event::TerminalControllerAttached { .. })
    });

    // Closing the controller's connection is reported as a detach.
    drop(second);
    wait_for_event(&events, |e| {
        matches!(e, Event::TerminalControllerDetached { .. })
    });
    assert!(f.client.attach(&id, AttachMode::Control, false).is_ok());
}

#[test]
fn an_observer_that_does_not_read_is_dropped_while_the_controller_keeps_receiving() {
    let f = fixture("slow");
    // Starts after a second, so both are attached before the flood.
    let id = f.run("sleep 1; yes 0123456789abcdef | head -c 20000000; sleep 30");
    let mut controller = f.attach(&id, AttachMode::Control);
    let mut slow = f.attach(&id, AttachMode::Observe);

    // The controller keeps reading on a thread of its own and signals once it received more
    // than the observer's queue (128 records of at most 32 KiB) and socket buffer can hold, so
    // the observer has overflowed by then.
    let (overflowed, signal) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut received = 0usize;
        let mut out = Vec::new();
        read_until(&mut controller, &mut out, |e, _| {
            if let Event::TerminalOutput { data, .. } = e {
                received += data.len() / 4 * 3;
            }
            if received >= 6_000_000 {
                let _ = overflowed.send(());
            }
            received >= 20_000_000
        });
        controller
    });
    // The observer must start reading before its writer's blocked write gives up (5 s, then the
    // connection closes without a record), so it waits for the overflow, not for the whole flood.
    signal
        .recv_timeout(Duration::from_secs(30))
        .expect("the controller did not receive the flood");

    // The slow observer finds a detach record after what was queued for it.
    let mut ignored = Vec::new();
    let end = read_until(&mut slow, &mut ignored, |e, _| {
        matches!(e, Event::TerminalDetached { .. })
    });
    assert_eq!(
        end,
        Event::TerminalDetached {
            terminal_id: id.clone(),
            reason: DetachReason::Overflow
        }
    );
    // The controller received everything, is still attached and can still type.
    let mut controller = reader.join().unwrap();
    controller.input("\n").unwrap();
}

#[test]
fn exit_and_detach_are_distinct_records() {
    let f = fixture("exit");
    let id = f.run("sleep 0.5; printf bye; exit 3");
    let mut a = f.attach(&id, AttachMode::Observe);
    let mut out = Vec::new();
    let end = read_until(&mut a, &mut out, |e, _| {
        matches!(
            e,
            Event::TerminalExited { .. } | Event::TerminalDetached { .. }
        )
    });
    assert_eq!(
        end,
        Event::TerminalExited {
            terminal_id: id.clone(),
            exit_code: Some(3)
        }
    );
    assert!(
        contains(&out, "bye"),
        "the output comes before the exit record"
    );

    // Attaching to an exited terminal gives the final screen and the exit record.
    let mut late = f.attach(&id, AttachMode::Observe);
    assert!(snapshot_text(&late).contains("bye"));
    assert_eq!(
        late.next_record().unwrap(),
        Event::TerminalExited {
            terminal_id: id,
            exit_code: Some(3)
        }
    );
}

#[test]
fn closing_a_terminal_detaches_its_clients() {
    let f = fixture("close");
    let id = f.run("sleep 30");
    let mut a = f.attach(&id, AttachMode::Observe);
    f.client.close_terminal(&id).unwrap();
    let mut out = Vec::new();
    let end = read_until(&mut a, &mut out, |e, _| {
        matches!(e, Event::TerminalDetached { .. })
    });
    assert_eq!(
        end,
        Event::TerminalDetached {
            terminal_id: id,
            reason: DetachReason::Closed
        }
    );
}

#[test]
fn attach_errors() {
    let f = fixture("errors");
    match f.client.attach("t999", AttachMode::Observe, false) {
        Err(ClientError::Server { code, .. }) => assert_eq!(code, ErrorCode::NotFound),
        other => panic!("{:?}", other.map(|a| a.info)),
    }
    let bad = f.client.request(
        "terminal.attach",
        serde_json::json!({"terminal_id": "t1", "mode": "spectate"}),
    );
    assert!(matches!(
        bad,
        Err(ClientError::Server {
            code: ErrorCode::InvalidParams,
            ..
        })
    ));
}

/// Waits for `file` in the work directory to have the given content.
fn wait_for_file(f: &Fixture, name: &str, expected: &str) {
    let path = f._env.dir.join("work").join(name);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = std::fs::read(&path).unwrap_or_default();
        if text == expected.as_bytes() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{name}: got {:?}",
            String::from_utf8_lossy(&text)
        );
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_program_without_an_app_gets_answers_to_its_terminal_queries() {
    let f = fixture("queries");
    f.client
        .set_colors(&SetColors {
            foreground: "#c0c0c0".into(),
            background: "#112233".into(),
            cursor: None,
            palette: None,
        })
        .unwrap();
    // 25 bytes: ESC ] 11 ; rgb:1111/2222/3333 ESC \
    f.run(
        "stty raw -echo; printf '\\033]11;?\\033\\\\'; head -c 25 > osc.txt; \
         printf '\\033[c'; head -c 5 > da.txt",
    );
    wait_for_file(&f, "osc.txt", "\x1b]11;rgb:1111/2222/3333\x1b\\");
    wait_for_file(&f, "da.txt", "\x1b[?6c");
}

#[test]
fn set_colors_rejects_a_bad_color_or_palette() {
    let f = fixture("colors-bad");
    let set = |bg: &str, palette: Option<Vec<String>>| {
        f.client.set_colors(&SetColors {
            foreground: "#ffffff".into(),
            background: bg.into(),
            cursor: None,
            palette,
        })
    };
    for result in [
        set("blue", None),
        set("#12345", None),
        set("#000000", Some(vec![])),
    ] {
        match result {
            Err(ClientError::Server { code, .. }) => assert_eq!(code, ErrorCode::InvalidParams),
            other => panic!("{other:?}"),
        }
    }
    set("#000000", Some(vec!["#010203".into(); 16])).unwrap();
}
