//! The stdio bridge against a server on a temp socket. Unix-only (directory modes). A real user
//! server is never touched: every socket is under the temp dir and the "start" is a closure.

#![cfg(unix)]

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use poimen_client::Client;
use poimen_client::protocol::TerminalCreate;
use poimen_server::stdio::{bridge, connect_or_start};
use poimen_server::{Config, PtyHost, Server};

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-stdio-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self { dir }
    }

    fn config(&self) -> Config {
        Config {
            socket: self.dir.join("run").join("server.sock"),
            state_file: self.dir.join("state").join("server.json"),
            scrollback: 1000,
            home: None,
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(config: &Config) -> Client {
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    Client::new(&config.socket)
}

/// A stdin that delivers what is sent and ends when the sender is dropped.
struct Input(Receiver<Vec<u8>>, Vec<u8>);

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.1.is_empty() {
            match self.0.recv() {
                Ok(chunk) => self.1 = chunk,
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.1.len());
        buf[..n].copy_from_slice(&self.1[..n]);
        self.1.drain(..n);
        Ok(n)
    }
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn lines_pass_through_unchanged_in_both_directions() {
    let env = Env::new("roundtrip");
    let config = env.config();
    let client = start(&config);
    let conn =
        connect_or_start(&config.socket, || panic!("must not start"), Duration::ZERO).unwrap();
    let (tx, rx) = mpsc::channel();
    let sink = Sink::default();
    let out = sink.clone();
    let handle =
        thread::spawn(move || bridge(&conn, Input(rx, Vec::new()), out, Duration::from_secs(5)));
    // Odd spacing and a second request in one chunk: the bridge must not reframe.
    tx.send(
        b"{\"id\":\"1\",\"method\":\"ping\" }\n{\"id\":\"2\",\"method\":\"space.list\"}\n".to_vec(),
    )
    .unwrap();
    drop(tx);
    handle.join().unwrap().unwrap();
    let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "{text}");
    let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(first["id"], "1");
    assert_eq!(
        first["result"]["protocol"],
        poimen_protocol::PROTOCOL_VERSION
    );
    let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
    assert_eq!(second["id"], "2");
    assert!(client.ping().is_ok());
}

#[test]
fn server_and_terminals_survive_the_bridge_ending() {
    let env = Env::new("survive");
    let config = env.config();
    let client = start(&config);
    let space = client
        .create_space("s", &env.dir.join("work").display().to_string())
        .unwrap();
    let terminal = client
        .create_terminal(&TerminalCreate {
            space_id: space.id.clone(),
            ..Default::default()
        })
        .unwrap();
    let conn =
        connect_or_start(&config.socket, || panic!("must not start"), Duration::ZERO).unwrap();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        bridge(
            &conn,
            Input(rx, Vec::new()),
            Sink::default(),
            Duration::from_millis(200),
        )
    });
    tx.send(b"{\"id\":\"1\",\"method\":\"ping\"}\n".to_vec())
        .unwrap();
    drop(tx); // stdin closed, like a dropped SSH session
    handle.join().unwrap().unwrap();
    let terminals = client.list_terminals(None).unwrap();
    assert!(terminals.iter().any(|t| t.id == terminal.id));
}

#[test]
fn starts_the_server_when_none_answers() {
    let env = Env::new("start");
    let config = env.config();
    let started = Arc::new(Mutex::new(0));
    let count = started.clone();
    let cfg = config.clone();
    let conn = connect_or_start(
        &config.socket,
        move || {
            *count.lock().unwrap() += 1;
            let server = Server::bind(cfg).map_err(io::Error::other)?;
            thread::spawn(move || server.serve());
            Ok(())
        },
        Duration::from_secs(10),
    )
    .unwrap();
    assert_eq!(*started.lock().unwrap(), 1);
    drop(conn);
    assert!(Client::new(&config.socket).ping().is_ok());
}

#[test]
fn a_failing_start_is_reported() {
    let env = Env::new("startfail");
    let config = env.config();
    let err = connect_or_start(
        &config.socket,
        || Err(io::Error::other("no binary")),
        Duration::from_secs(1),
    )
    .unwrap_err();
    assert!(err.to_string().contains("no binary"), "{err}");
}

#[test]
fn a_server_that_never_answers_times_out() {
    let env = Env::new("timeout");
    let config = env.config();
    let err = connect_or_start(&config.socket, || Ok(()), Duration::from_millis(300)).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::TimedOut);
}
