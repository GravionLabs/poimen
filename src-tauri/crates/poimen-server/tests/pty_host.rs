//! The server as a client of its PTY hosts (ADR 0009), against a scripted fake host: an
//! in-process endpoint that speaks the host protocol, so no child process runs. Platform free.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::protocol::host::{
    ClientMessage, HostMessage, HostStart, Size, parse_client_line, to_line,
};
use poimen_client::protocol::{
    AttachMode, ErrorCode, Event, TerminalCreate, TerminalStatus, decode_bytes, encode_bytes,
};
use poimen_client::transport::{Conn, Listener};
use poimen_client::{Client, ClientError};
use poimen_server::{Config, PtyHost, Server};

const WAIT: Duration = Duration::from_secs(10);

struct Env {
    dir: PathBuf,
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
    events: Receiver<Event>,
}

fn fixture(name: &str, pty_host: PtyHost) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-ph-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let env = Env { dir };
    let config = Config {
        socket: env.dir.join("run").join("server.sock"),
        state_file: env.dir.join("state").join("server.json"),
        scrollback: 100,
        home: None,
        pty_host,
    };
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let space = client
        .create_space("work", &env.dir.display().to_string())
        .unwrap()
        .id;
    let stream = client.subscribe().unwrap();
    let (tx, events) = mpsc::channel();
    thread::spawn(move || {
        for event in stream.flatten() {
            if tx.send(event).is_err() {
                return;
            }
        }
    });
    Fixture {
        _env: env,
        client,
        space,
        events,
    }
}

impl Fixture {
    fn create(&self) -> Result<String, ClientError> {
        self.client
            .create_terminal(&TerminalCreate {
                space_id: self.space.clone(),
                command: Some(vec!["fake-shell".into(), "-l".into()]),
                cols: Some(90),
                rows: Some(20),
                ..Default::default()
            })
            .map(|t| t.id)
    }

    fn wait_for(&self, pred: impl Fn(&Event) -> bool) -> Event {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let event = self
                .events
                .recv_timeout(left)
                .expect("event did not arrive");
            if pred(&event) {
                return event;
            }
        }
    }

    fn status(&self, id: &str) -> (TerminalStatus, Option<u32>) {
        let t = self
            .client
            .list_terminals(None)
            .unwrap()
            .into_iter()
            .find(|t| t.id == id)
            .unwrap();
        (t.status, t.exit_code)
    }

    /// The output a fresh observer sees in its snapshot.
    fn screen(&self, id: &str) -> String {
        let a = self.client.attach(id, AttachMode::Observe, false).unwrap();
        String::from_utf8_lossy(&decode_bytes(&a.info.snapshot).unwrap()).into_owned()
    }

    fn wait_for_screen(&self, id: &str, needle: &str) -> String {
        let deadline = Instant::now() + WAIT;
        loop {
            let screen = self.screen(id);
            if screen.contains(needle) || Instant::now() > deadline {
                return screen;
            }
            thread::sleep(Duration::from_millis(20));
        }
    }
}

/// One connection of the fake host.
struct Peer {
    conn: Conn,
    lines: BufReader<Conn>,
}

impl Peer {
    fn accept(listener: &Listener, seq: u64, start: &HostStart) -> Peer {
        let conn = listener.accept().unwrap();
        let mut peer = Peer {
            lines: BufReader::new(conn.clone()),
            conn,
        };
        peer.send(&HostMessage::Hello {
            host_version: 1,
            version: "fake".into(),
            pid: 1,
            child_pid: Some(2),
            size: Size {
                cols: start.spawn.cols,
                rows: start.spawn.rows,
            },
            endpoint: start.endpoint.clone(),
            seq,
        });
        peer
    }

    fn send(&mut self, message: &HostMessage) {
        let _ = (&self.conn).write_all(&to_line(message));
    }

    fn output(&mut self, seq: u64, text: &str) {
        self.send(&HostMessage::Output {
            seq,
            bytes: encode_bytes(text.as_bytes()),
        });
    }

    fn read(&mut self) -> Option<ClientMessage> {
        let mut line = String::new();
        match self.lines.read_line(&mut line) {
            Ok(0) | Err(_) => None,
            Ok(_) => parse_client_line(&line).ok(),
        }
    }

    fn close(self) {
        let _ = self.conn.shutdown();
    }
}

/// A fake host: binds the endpoint like the real one and runs `script` on a thread with the
/// listener and the start line. What the script reports goes to the returned receiver.
fn fake(
    script: impl Fn(Listener, HostStart, Sender<String>) + Send + Sync + 'static,
) -> (PtyHost, Receiver<String>, Arc<Mutex<Vec<HostStart>>>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let starts = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&starts);
    let script = Arc::new(script);
    let host = PtyHost::launch(move |start: &HostStart| {
        let listener = Listener::bind(&start.endpoint).map_err(|e| e.to_string())?;
        seen.lock().unwrap().push(start.clone());
        let (script, start, tx) = (
            Arc::clone(&script),
            start.clone(),
            tx.lock().unwrap().clone(),
        );
        thread::spawn(move || script(listener, start, tx));
        Ok(())
    });
    (host, rx, starts)
}

fn report(tx: &Sender<String>, message: Option<ClientMessage>) {
    let _ = tx.send(format!("{message:?}"));
}

#[test]
fn a_missing_host_binary_fails_terminal_create_with_a_clear_error() {
    let f = fixture(
        "missing",
        PtyHost::Binary(PathBuf::from("/nonexistent/dir/poimen-pty-host")),
    );
    match f.create() {
        Err(ClientError::Server { code, message }) => {
            assert_eq!(code, ErrorCode::SpawnFailed);
            assert_eq!(
                message,
                "poimen-pty-host is missing: /nonexistent/dir/poimen-pty-host (it ships next to poimen-server)"
            );
        }
        other => panic!("expected spawn_failed, got {other:?}"),
    }
    assert!(f.client.list_terminals(None).unwrap().is_empty());
    // A space cannot be built without its root terminal either.
    let err = f
        .client
        .create_space_with(&poimen_client::protocol::SpaceCreate {
            name: "x".into(),
            cwd: std::env::temp_dir().display().to_string(),
            layout: true,
            trust: None,
            command: None,
        })
        .unwrap_err();
    assert!(
        err.to_string().contains("poimen-pty-host is missing"),
        "{err}"
    );
}

#[test]
fn a_new_terminal_starts_a_host_with_its_spawn_spec_and_reads_it_by_seq() {
    let (host, said, starts) = fake(|listener, start, tx| {
        let mut peer = Peer::accept(&listener, 0, &start);
        report(&tx, peer.read());
        // `kill` comes on a connection of its own.
        let (kill_tx, kill_start) = (tx.clone(), start.clone());
        thread::spawn(move || {
            let mut killer = Peer::accept(&listener, 3, &kill_start);
            let _ = kill_tx.send(format!(
                "own connection: {:?} {:?}",
                killer.read(),
                killer.read()
            ));
        });
        // Records strictly by seq: repeated and older ones are not fed again.
        peer.output(1, "one ");
        peer.output(2, "two ");
        peer.output(2, "two ");
        peer.output(1, "one ");
        peer.output(3, "three");
        loop {
            let message = peer.read();
            let end = message.is_none();
            report(&tx, message);
            if end {
                return;
            }
        }
    });
    let f = fixture("spawn", host);
    let id = f.create().unwrap();
    assert_eq!(
        said.recv_timeout(WAIT).unwrap(),
        "Some(Attach { since: 0 })"
    );
    let start = starts.lock().unwrap()[0].clone();
    assert_eq!(start.spawn.argv, ["fake-shell", "-l"]);
    assert_eq!((start.spawn.cols, start.spawn.rows), (90, 20));
    assert_eq!(
        start
            .spawn
            .env
            .get("POIMEN_TERMINAL_ID")
            .map(String::as_str),
        Some(id.as_str())
    );
    assert!(start.endpoint.ends_with(PathBuf::from("hosts").join(&id)));

    let screen = f.wait_for_screen(&id, "three");
    assert!(screen.contains("one two three"), "{screen:?}");
    assert_eq!(screen.matches("two").count(), 1, "{screen:?}");

    // Input, resize and close go to the host.
    f.client.input(&id, "ls\r").unwrap();
    assert_eq!(
        said.recv_timeout(WAIT).unwrap(),
        format!("Some(Input {{ bytes: {:?} }})", encode_bytes(b"ls\r"))
    );
    f.client.resize(&id, 100, 30).unwrap();
    assert_eq!(
        said.recv_timeout(WAIT).unwrap(),
        "Some(Resize { cols: 100, rows: 30 })"
    );
    f.client.close_terminal(&id).unwrap();
    // Attached after the last record read, so the host can hand over `exit` and end.
    assert_eq!(
        said.recv_timeout(WAIT).unwrap(),
        "own connection: Some(Attach { since: 3 }) Some(Kill)"
    );
}

/// Input of `n` bytes in `parts` requests, as ASCII letters by position.
fn flood(n: usize, parts: usize) -> Vec<String> {
    let text: String = (0..n).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
    let size = n / parts;
    (0..parts)
        .map(|p| text[p * size..(p + 1) * size].to_string())
        .collect()
}

#[test]
fn close_and_resize_do_not_wait_behind_input_the_host_does_not_take() {
    // A child that never reads stdin: the host stops reading its connection.
    let (host, said, _) = fake(|listener, start, tx| {
        let mut peer = Peer::accept(&listener, 0, &start);
        report(&tx, peer.read());
        let mut killer = Peer::accept(&listener, 0, &start);
        let _ = tx.send(format!(
            "own connection: {:?} {:?}",
            killer.read(),
            killer.read()
        ));
        drop(peer);
    });
    let f = fixture("stuck", host);
    let id = f.create().unwrap();
    assert_eq!(
        said.recv_timeout(WAIT).unwrap(),
        "Some(Attach { since: 0 })"
    );
    // Far more than a socket buffer holds.
    for part in flood(512 * 1024, 16) {
        f.client.input(&id, &part).unwrap();
    }
    let started = Instant::now();
    f.client.resize(&id, 120, 40).unwrap();
    f.client.list_terminals(None).unwrap();
    f.client.close_terminal(&id).unwrap();
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "requests waited {:?} behind the input",
        started.elapsed()
    );
    assert_eq!(
        said.recv_timeout(WAIT).unwrap(),
        "own connection: Some(Attach { since: 0 }) Some(Kill)"
    );
    f.wait_for(|e| matches!(e, Event::TerminalClosed { terminal_id, .. } if *terminal_id == id));
    assert!(f.client.list_terminals(None).unwrap().is_empty());
}

#[test]
fn input_the_host_takes_late_arrives_in_whole_lines_and_in_order() {
    const TOTAL: usize = 256 * 1024;
    let (host, said, _) = fake(|listener, start, tx| {
        let mut peer = Peer::accept(&listener, 0, &start);
        report(&tx, peer.read());
        // A 5 s write timeout (the server's before this fix) gives up within this stall, after a
        // partial write, and left half a line on the connection.
        thread::sleep(Duration::from_secs(11));
        let (mut input, mut invalid, mut resized) = (Vec::new(), 0, None);
        while input.len() < TOTAL || resized.is_none() {
            let mut line = String::new();
            match peer.lines.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            match parse_client_line(&line) {
                Ok(ClientMessage::Input { bytes }) => {
                    input.extend(decode_bytes(&bytes).unwrap());
                }
                Ok(ClientMessage::Resize { cols, rows }) => resized = Some((cols, rows)),
                _ => invalid += 1,
            }
        }
        let _ = tx.send(format!(
            "invalid {invalid}, resized {resized:?}, input {}",
            String::from_utf8_lossy(&input)
        ));
    });
    let f = fixture("late", host);
    let id = f.create().unwrap();
    assert_eq!(
        said.recv_timeout(WAIT).unwrap(),
        "Some(Attach { since: 0 })"
    );
    let parts = flood(TOTAL, 32);
    for part in &parts {
        f.client.input(&id, part).unwrap();
    }
    f.client.resize(&id, 120, 40).unwrap();
    assert_eq!(
        said.recv_timeout(WAIT + WAIT).unwrap(),
        format!(
            "invalid 0, resized Some((120, 40)), input {}",
            parts.concat()
        )
    );
}

#[test]
fn the_exit_of_the_child_ends_the_terminal_with_its_code_after_its_output() {
    let (host, _said, _) = fake(|listener, start, _tx| {
        let mut peer = Peer::accept(&listener, 0, &start);
        peer.read();
        peer.output(1, "bye");
        peer.send(&HostMessage::Exit { code: Some(3) });
        thread::sleep(Duration::from_secs(5));
    });
    let f = fixture("exit", host);
    let id = f.create().unwrap();
    let exited = f.wait_for(|e| matches!(e, Event::TerminalExited { .. }));
    assert_eq!(
        exited,
        Event::TerminalExited {
            terminal_id: id.clone(),
            exit_code: Some(3)
        }
    );
    assert_eq!(f.status(&id), (TerminalStatus::Exited, Some(3)));
    // An exited terminal has nothing left that a restart would keep.
    let listed = f.client.list_terminals(None).unwrap();
    assert!(!listed.iter().any(|t| t.survives_restart));
    assert!(f.screen(&id).contains("bye"));
    // As on any exit, the terminal left its tab.
    f.wait_for(|e| matches!(e, Event::TabClosed { .. }));
}

#[test]
fn a_host_that_dies_ends_the_terminal_as_an_exit_without_a_code() {
    let (host, _said, _) = fake(|listener, start, _tx| {
        let mut peer = Peer::accept(&listener, 0, &start);
        peer.read();
        peer.output(1, "partial");
        // The host goes away: the connection ends and nothing listens any more.
        drop(listener);
        let _ = std::fs::remove_file(&start.endpoint);
        peer.close();
    });
    let f = fixture("dies", host);
    let id = f.create().unwrap();
    let exited = f.wait_for(|e| matches!(e, Event::TerminalExited { .. }));
    assert_eq!(
        exited,
        Event::TerminalExited {
            terminal_id: id.clone(),
            exit_code: None
        }
    );
    f.wait_for(|e| matches!(e, Event::TabClosed { .. }));
    assert_eq!(f.status(&id), (TerminalStatus::Exited, None));
    assert!(f.screen(&id).contains("partial"));
    assert!(matches!(
        f.client.input(&id, "x"),
        Err(ClientError::Server {
            code: ErrorCode::Exited,
            ..
        })
    ));
}

#[test]
fn a_dropped_connection_is_attached_again_after_the_last_seq_read() {
    let (host, said, _) = fake(|listener, start, tx| {
        let mut first = Peer::accept(&listener, 0, &start);
        report(&tx, first.read());
        first.output(1, "a");
        first.output(2, "b");
        // The host drops a client that falls behind; it reconnects with `since`.
        first.close();
        let mut second = Peer::accept(&listener, 2, &start);
        report(&tx, second.read());
        // The replay of the ring repeats what was read already.
        second.output(1, "a");
        second.output(2, "b");
        second.output(3, "c");
        second.send(&HostMessage::Exit { code: Some(0) });
        thread::sleep(Duration::from_secs(5));
    });
    let f = fixture("again", host);
    let id = f.create().unwrap();
    assert_eq!(
        said.recv_timeout(WAIT).unwrap(),
        "Some(Attach { since: 0 })"
    );
    assert_eq!(
        said.recv_timeout(WAIT).unwrap(),
        "Some(Attach { since: 2 })"
    );
    f.wait_for(|e| matches!(e, Event::TerminalExited { .. }));
    let screen = f.screen(&id);
    assert!(screen.contains("abc"), "{screen:?}");
    assert_eq!(screen.matches('a').count(), 1, "{screen:?}");
    assert_eq!(f.status(&id), (TerminalStatus::Exited, Some(0)));
}

#[test]
fn a_host_that_answers_with_an_error_fails_terminal_create() {
    let host = PtyHost::launch(|_start: &HostStart| Err("could not start fake-shell: no".into()));
    let f = fixture("refused", host);
    match f.create() {
        Err(ClientError::Server { code, message }) => {
            assert_eq!(code, ErrorCode::SpawnFailed);
            assert_eq!(message, "could not start fake-shell: no");
        }
        other => panic!("expected spawn_failed, got {other:?}"),
    }
}
