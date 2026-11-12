//! `poimen attach` against an in-process poimen-server on a temporary socket (never the user's
//! server). Unix-only: the terminals run POSIX shell scripts.
#![cfg(unix)]

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use poimen_client::protocol::{AttachMode, DetachReason, TerminalCreate, TerminalStatus};
use poimen_client::{Client, ClientError};
use poimen_console::keys::DEFAULT_PREFIX;
use poimen_console::session::{self, Error, Options, Outcome, RESET};
use poimen_console::tty::{RawGuard, Terminal};
use poimen_server::{Config, PtyHost, Server};

struct Fixture {
    dir: PathBuf,
    client: Client,
    space: String,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-co-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let config = Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        scrollback: 1000,
        home: None,
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
    };
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let work = dir.join("work").canonicalize().unwrap();
    let space = client
        .create_space("work", &work.display().to_string())
        .unwrap()
        .id;
    Fixture { dir, client, space }
}

impl Fixture {
    /// A terminal that prints `first`, then echoes what it is typed (`cat`).
    fn cat_terminal(&self) -> String {
        self.client
            .create_terminal(&TerminalCreate {
                space_id: self.space.clone(),
                command: Some(vec![
                    "sh".into(),
                    "-c".into(),
                    "printf first; exec cat".into(),
                ]),
                ..Default::default()
            })
            .unwrap()
            .id
    }
}

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);

impl Write for Sink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Sink {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
    fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.text().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "`{needle}` did not arrive: {:?}",
                self.text()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }
}

struct Session {
    input: Sender<Vec<u8>>,
    out: Sink,
    cols: Arc<AtomicU32>,
    stop: Arc<AtomicBool>,
    handle: JoinHandle<Result<Outcome, Error>>,
}

fn start(f: &Fixture, id: &str, mode: AttachMode, take_over: bool) -> Session {
    let (input, rx) = mpsc::channel();
    let out = Sink::default();
    let cols = Arc::new(AtomicU32::new(100));
    let stop = Arc::new(AtomicBool::new(false));
    let client = Client::new(f.client.socket());
    let opts = Options {
        terminal_id: id.into(),
        mode,
        take_over,
        prefix: DEFAULT_PREFIX,
    };
    let (o, c, s) = (out.clone(), cols.clone(), stop.clone());
    let handle = thread::spawn(move || {
        let size = || (c.load(Ordering::SeqCst) as u16, 30);
        let mut o = o;
        session::run(&client, &opts, rx, &mut o, &size, &s)
    });
    Session {
        input,
        out,
        cols,
        stop,
        handle,
    }
}

fn size_of(f: &Fixture, id: &str) -> (u16, u16) {
    let t = f.client.list_terminals(None).unwrap();
    let t = t.iter().find(|t| t.id == id).unwrap();
    (t.cols, t.rows)
}

fn wait_until(what: &str, mut pred: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !pred() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn observer_gets_snapshot_then_stream_and_detaches_leaving_the_terminal_running() {
    let f = fixture("observe");
    let id = f.cat_terminal();
    let s = start(&f, &id, AttachMode::Observe, false);
    s.out.wait_for("first");

    f.client.input(&id, "later\n").unwrap();
    s.out.wait_for("later");

    s.input.send(vec![DEFAULT_PREFIX, b'd']).unwrap();
    assert_eq!(s.handle.join().unwrap().unwrap(), Outcome::Detached);

    let text = s.out.text();
    assert!(text.find("first").unwrap() < text.find("later").unwrap());
    assert!(text.ends_with(std::str::from_utf8(RESET).unwrap()));
    let t = f.client.list_terminals(None).unwrap();
    assert_eq!(t[0].status, TerminalStatus::Running);
}

#[test]
fn observer_input_is_not_sent_and_keeps_the_controller_size() {
    let f = fixture("observer-size");
    let id = f.cat_terminal();
    let before = size_of(&f, &id);
    let s = start(&f, &id, AttachMode::Observe, false);
    s.out.wait_for("first");

    s.input.send(b"typed\n".to_vec()).unwrap();
    s.cols.store(120, Ordering::SeqCst);
    thread::sleep(Duration::from_millis(400));
    assert!(!s.out.text().contains("typed"));
    assert_eq!(size_of(&f, &id), before);

    s.input.send(vec![DEFAULT_PREFIX, b'd']).unwrap();
    assert_eq!(s.handle.join().unwrap().unwrap(), Outcome::Detached);
}

#[test]
fn controller_sends_input_and_window_size_to_the_pty() {
    let f = fixture("control");
    let id = f.cat_terminal();
    let s = start(&f, &id, AttachMode::Control, false);
    s.out.wait_for("first");
    wait_until("initial size", || size_of(&f, &id) == (100, 30));

    s.input.send(b"hello\n".to_vec()).unwrap();
    s.out.wait_for("hello");

    s.cols.store(132, Ordering::SeqCst);
    wait_until("resized pty", || size_of(&f, &id) == (132, 30));

    // The prefix twice reaches the terminal as one Ctrl+B, which cat echoes as ^B.
    s.input.send(vec![DEFAULT_PREFIX, DEFAULT_PREFIX]).unwrap();
    s.out.wait_for("^B");

    s.input.send(vec![DEFAULT_PREFIX, b'd']).unwrap();
    assert_eq!(s.handle.join().unwrap().unwrap(), Outcome::Detached);
}

#[test]
fn second_controller_needs_takeover() {
    let f = fixture("takeover");
    let id = f.cat_terminal();
    let first = start(&f, &id, AttachMode::Control, false);
    first.out.wait_for("first");

    let refused = start(&f, &id, AttachMode::Control, false);
    let err = refused.handle.join().unwrap().unwrap_err();
    assert!(matches!(err, Error::Client(ClientError::Server { .. })));
    assert!(err.to_string().contains("--takeover"));

    let second = start(&f, &id, AttachMode::Control, true);
    second.out.wait_for("first");
    assert_eq!(
        first.handle.join().unwrap().unwrap(),
        Outcome::ServerDetached(DetachReason::TakenOver)
    );
    second.input.send(vec![DEFAULT_PREFIX, b'd']).unwrap();
    assert_eq!(second.handle.join().unwrap().unwrap(), Outcome::Detached);
}

#[test]
fn stop_flag_ends_the_session_and_resets_the_terminal() {
    let f = fixture("stop");
    let id = f.cat_terminal();
    let s = start(&f, &id, AttachMode::Observe, false);
    s.out.wait_for("first");
    s.stop.store(true, Ordering::SeqCst);
    assert_eq!(s.handle.join().unwrap().unwrap(), Outcome::Interrupted);
    assert!(s.out.text().ends_with(std::str::from_utf8(RESET).unwrap()));
}

#[test]
fn exited_terminal_ends_the_session_with_its_code() {
    let f = fixture("exit");
    let id = f
        .client
        .create_terminal(&TerminalCreate {
            space_id: f.space.clone(),
            command: Some(vec!["sh".into(), "-c".into(), "sleep 1; exit 3".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    let s = start(&f, &id, AttachMode::Observe, false);
    assert_eq!(s.handle.join().unwrap().unwrap(), Outcome::Exited(Some(3)));
}

#[derive(Default)]
struct FakeTty {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl Terminal for FakeTty {
    fn enable_raw(&mut self) -> std::io::Result<()> {
        self.log.lock().unwrap().push("raw");
        Ok(())
    }
    fn disable_raw(&mut self) -> std::io::Result<()> {
        self.log.lock().unwrap().push("restored");
        Ok(())
    }
    fn size(&self) -> (u16, u16) {
        (100, 30)
    }
}

#[test]
fn terminal_mode_is_restored_after_detach_and_after_an_error() {
    let f = fixture("raw");
    let id = f.cat_terminal();
    let mut tty = FakeTty::default();
    let log = tty.log.clone();

    {
        let guard = RawGuard::new(&mut tty).unwrap();
        let (tx, rx) = mpsc::channel();
        tx.send(vec![DEFAULT_PREFIX, b'd']).unwrap();
        let opts = Options {
            terminal_id: id,
            mode: AttachMode::Observe,
            take_over: false,
            prefix: DEFAULT_PREFIX,
        };
        let size = || guard.size();
        let mut out = Sink::default();
        let stop = AtomicBool::new(false);
        let outcome = session::run(&f.client, &opts, rx, &mut out, &size, &stop).unwrap();
        assert_eq!(outcome, Outcome::Detached);
        assert_eq!(*log.lock().unwrap(), ["raw"]);
    }
    assert_eq!(*log.lock().unwrap(), ["raw", "restored"]);

    // An unknown terminal fails inside the guard; the mode is restored anyway.
    {
        let guard = RawGuard::new(&mut tty).unwrap();
        let (_tx, rx) = mpsc::channel();
        let opts = Options {
            terminal_id: "nope".into(),
            mode: AttachMode::Observe,
            take_over: false,
            prefix: DEFAULT_PREFIX,
        };
        let size = || guard.size();
        let mut out = Sink::default();
        let stop = AtomicBool::new(false);
        assert!(session::run(&f.client, &opts, rx, &mut out, &size, &stop).is_err());
    }
    assert_eq!(*log.lock().unwrap(), ["raw", "restored", "raw", "restored"]);
}
