//! An ended terminal (its host is gone, #1195) whose agent session is known comes back as a
//! shell that resumes the session (#1232, ADR 0014): `hibernated` after adoption, a new host
//! under the same id on the first control attach, the resume command typed once, an observer
//! starts nothing.
//!
//! The hosts are scripted fakes (an in-process endpoint speaking the host protocol).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::host::{
    ClientMessage, HostMessage, HostStart, Size, parse_client_line, to_line,
};
use poimen_client::protocol::{
    AgentStatus, AttachMode, TerminalCreate, TerminalStatus, decode_bytes, encode_bytes,
};
use poimen_client::transport::{Conn, Listener};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};

const WAIT: Duration = Duration::from_secs(10);

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-resume-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self { dir }
    }

    fn config(&self, n: u32, pty_host: PtyHost) -> Config {
        let state = self.dir.join("state").join("server.json");
        Config {
            socket: self.dir.join(format!("run{n}")).join("server.sock"),
            history_dir: state.with_file_name("history"),
            state_file: state,
            scrollback: 100,
            home: None,
            pty_host,
        }
    }

    fn state(&self) -> Value {
        let text = std::fs::read_to_string(self.dir.join("state").join("server.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn work(&self) -> String {
        self.dir
            .join("work")
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(config: &Config) -> (Client, Receiver<()>) {
    let server = Server::bind(config.clone()).unwrap();
    let (tx, rx) = mpsc::channel();
    server.on_shutdown(move || {
        let _ = tx.send(());
    });
    thread::spawn(move || server.serve());
    (Client::new(&config.socket), rx)
}

struct Peer {
    conn: Conn,
    lines: BufReader<Conn>,
}

impl Peer {
    fn accept(listener: &Listener, endpoint: &Path) -> Peer {
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
            size: Size { cols: 90, rows: 20 },
            endpoint: endpoint.to_path_buf(),
            seq: 0,
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
}

/// A fake host per terminal; `script` gets the listener, the start line and a sender for what it
/// observed.
fn fake(
    script: impl Fn(Listener, HostStart, Sender<String>) + Send + Sync + 'static,
) -> (PtyHost, Receiver<String>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let script = Arc::new(script);
    let host = PtyHost::launch(move |start: &HostStart| {
        let listener = Listener::bind(&start.endpoint).map_err(|e| e.to_string())?;
        let (script, start, tx) = (
            Arc::clone(&script),
            start.clone(),
            tx.lock().unwrap().clone(),
        );
        thread::spawn(move || script(listener, start, tx));
        Ok(())
    });
    (host, rx)
}

fn no_new_host() -> PtyHost {
    PtyHost::launch(|_: &HostStart| Err("no host is started on adoption".into()))
}

fn screen(client: &Client, id: &str) -> String {
    let a = client.attach(id, AttachMode::Observe, false).unwrap();
    String::from_utf8_lossy(&decode_bytes(&a.info.snapshot).unwrap()).into_owned()
}

fn wait_for_screen(client: &Client, id: &str, needle: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let s = screen(client, id);
        if s.contains(needle) || Instant::now() > deadline {
            return s;
        }
        thread::sleep(Duration::from_millis(20));
    }
}

fn report(client: &Client, id: &str, seq: u64, status: AgentStatus, session: Option<&str>) {
    client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": id, "seq": seq, "status": status, "agent": "claude", "session_id": session}),
        )
        .unwrap();
}

/// A server whose terminal printed "earlier work" and whose agent reported `reports`, stopped
/// keeping it, with the host gone afterwards (a reboot). `reports` are `(status, session)`.
fn host_gone(name: &str, reports: &[(AgentStatus, Option<&str>)]) -> (Env, String) {
    let env = Env::new(name);
    let (host, _) = fake(|listener, start, _| {
        let mut peer = Peer::accept(&listener, &start.endpoint);
        peer.read();
        peer.output(1, "earlier work\r\n");
        while peer.read().is_some() {}
    });
    let (a, a_down) = start(&env.config(1, host));
    let space = a.create_space("work", &env.work()).unwrap().id;
    let id = a
        .create_terminal(&TerminalCreate {
            space_id: space,
            command: Some(vec!["fake-shell".into(), "-l".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    wait_for_screen(&a, &id, "earlier");
    for (n, (status, session)) in reports.iter().enumerate() {
        report(&a, &id, 10 + n as u64, *status, *session);
    }
    a.shutdown_keeping_terminals().unwrap();
    a_down.recv_timeout(WAIT).unwrap();
    let endpoint = env.state()["terminals"][0]["endpoint"].clone();
    let _ = std::fs::remove_file(endpoint.as_str().unwrap());
    (env, id)
}

/// A host for the second server: reports its start line, shows a prompt and reports every input.
fn shell_host() -> (PtyHost, Receiver<String>) {
    fake(|listener, start, tx| {
        let _ = tx.send(format!(
            "start {} cwd={}",
            start.spawn.argv.join(" "),
            start.spawn.cwd
        ));
        let mut peer = Peer::accept(&listener, &start.endpoint);
        while let Some(message) = peer.read() {
            match message {
                ClientMessage::Attach { .. } => peer.output(1, "$ "),
                ClientMessage::Input { bytes } => {
                    let typed = decode_bytes(&bytes).unwrap();
                    let _ = tx.send(format!("input {}", String::from_utf8_lossy(&typed)));
                }
                _ => {}
            }
        }
    })
}

fn said(rx: &Receiver<String>) -> String {
    rx.recv_timeout(WAIT).expect("the host said nothing")
}

#[test]
fn an_ended_terminal_with_a_session_is_resumable_and_a_control_attach_resumes_it_once() {
    let (env, id) = host_gone("resume", &[(AgentStatus::Done, Some("sess-1"))]);
    let (host, rx) = shell_host();
    let (b, _) = start(&env.config(2, host));

    let t = b.list_terminals(None).unwrap().remove(0);
    assert!(t.from_history && t.status == TerminalStatus::Exited);
    let h = t.hibernated.expect("resumable");
    assert_eq!(
        (h.agent.as_str(), h.session_id.as_str()),
        ("claude", "sess-1")
    );

    // An observer starts nothing and sees the history.
    let observer = b.attach(&id, AttachMode::Observe, false).unwrap();
    assert!(observer.info.read_only);
    assert!(rx.recv_timeout(Duration::from_millis(300)).is_err());
    drop(observer);

    // A controller starts a new shell in the same terminal with the program and arguments of the
    // old one and types the resume command.
    let a = b.attach(&id, AttachMode::Control, false).unwrap();
    assert!(!a.info.read_only);
    assert_eq!(a.info.mode, AttachMode::Control);
    assert!(said(&rx).starts_with(&format!("start fake-shell -l cwd={}", env.work())));
    assert_eq!(said(&rx), "input claude --resume sess-1\r");
    let t = b.list_terminals(None).unwrap().remove(0);
    assert_eq!(t.id, id);
    assert!(!t.from_history && t.status == TerminalStatus::Running);
    // The history stays in the grid; the new output follows.
    let s = wait_for_screen(&b, &id, "$\r");
    assert!(s.contains("earlier work") && s.contains("$\r"), "{s:?}");
    // One resume: a second control attach (takeover) types nothing again.
    drop(a);
    let _again = b.attach(&id, AttachMode::Control, false).unwrap();
    assert!(rx.recv_timeout(Duration::from_millis(400)).is_err());
}

#[test]
fn the_latest_session_wins_and_survives_a_gone_report() {
    let (env, _) = host_gone(
        "latest",
        &[
            (AgentStatus::Done, Some("old")),
            (AgentStatus::Working, Some("new")),
            (AgentStatus::Done, None),
            (AgentStatus::Gone, None),
        ],
    );
    assert_eq!(
        env.state()["terminals"][0]["last_agent"]["session_id"],
        "new"
    );
    let (b, _) = start(&env.config(2, no_new_host()));
    let t = b.list_terminals(None).unwrap().remove(0);
    assert_eq!(t.hibernated.unwrap().session_id, "new");
}

#[test]
fn without_a_session_the_ended_terminal_stays_read_only() {
    let (env, id) = host_gone("nosession", &[(AgentStatus::Done, None)]);
    let (b, _) = start(&env.config(2, no_new_host()));
    let t = b.list_terminals(None).unwrap().remove(0);
    assert!(t.from_history && t.hibernated.is_none());
    let a = b.attach(&id, AttachMode::Control, false).unwrap();
    assert!(a.info.read_only);
}

#[test]
fn a_resume_that_gets_no_report_publishes_resume_failed_and_the_shell_stays() {
    let (env, id) = host_gone("timeout", &[(AgentStatus::Done, Some("sess-1"))]);
    let (host, rx) = shell_host();
    let config = env.config(2, host);
    let server = Server::bind(config.clone()).unwrap();
    server.set_resume_timeout(Duration::from_millis(300));
    thread::spawn(move || server.serve());
    let b = Client::new(&config.socket);
    let stream = b.subscribe().unwrap();
    let (tx, events) = mpsc::channel();
    thread::spawn(move || {
        for event in stream.flatten() {
            if tx.send(event).is_err() {
                break;
            }
        }
    });
    let _a = b.attach(&id, AttachMode::Control, false).unwrap();
    let _ = said(&rx);
    let deadline = Instant::now() + WAIT;
    let mut failed = false;
    while !failed && Instant::now() < deadline {
        if let Ok(poimen_client::protocol::Event::TerminalResumeFailed { reason, .. }) =
            events.recv_timeout(Duration::from_millis(200))
        {
            assert!(reason.contains("did not report"), "{reason}");
            failed = true;
        }
    }
    assert!(failed, "terminal.resume_failed");
    let t = b.list_terminals(None).unwrap().remove(0);
    assert!(t.hibernated.is_none() && t.status == TerminalStatus::Running);
}
