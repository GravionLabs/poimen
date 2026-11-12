//! A new server adopts the PTY hosts of the terminal list in `server.json` (ADR 0009, #604):
//! start a server, stop it with `keep_terminals`, start a second one on the same state file, and
//! the terminal, its screen, its agent status and its tokens are back under the same id.
//!
//! The scripted fake host (an in-process endpoint speaking the host protocol) runs on every
//! platform; the real `poimen-pty-host` (in-process) with a real shell on Unix. Every server and
//! host lives in a temp directory: no test touches the user's server.

use std::io::{BufRead, BufReader, Write};
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
    AgentStatus, AttachMode, Node, TerminalCreate, TerminalStatus, decode_bytes, encode_bytes,
};
use poimen_client::transport::{Conn, Listener};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(10);

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-adopt-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self { dir }
    }

    /// The config of server number `n`: a socket of its own (the first server keeps its lock in
    /// this process), the one state file all of them share.
    fn config(&self, n: u32, pty_host: PtyHost) -> Config {
        Config {
            socket: self.dir.join(format!("run{n}")).join("server.sock"),
            state_file: self.state_file(),
            scrollback: 100,
            home: None,
            pty_host,
        }
    }

    fn state_file(&self) -> PathBuf {
        self.dir.join("state").join("server.json")
    }

    fn state(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.state_file()).unwrap()).unwrap()
    }

    fn work(&self) -> String {
        let work = self.dir.join("work");
        if cfg!(windows) {
            work.display().to_string()
        } else {
            work.canonicalize().unwrap().display().to_string()
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Binds and serves a server; the receiver gets a value once a `server.shutdown` is done.
fn start(config: &Config) -> (Client, Receiver<()>) {
    let server = Server::bind(config.clone()).unwrap();
    let (tx, rx) = mpsc::channel();
    // Stands in for the process exit of the binary; the test process lives on.
    server.on_shutdown(move || {
        let _ = tx.send(());
    });
    thread::spawn(move || server.serve());
    (Client::new(&config.socket), rx)
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

fn report_agent(client: &Client, id: &str, seq: u64, status: AgentStatus) -> Value {
    client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": id, "seq": seq, "status": status, "agent": "claude", "session_id": "sess-1"}),
        )
        .unwrap()
}

/// One connection of the fake host.
struct Peer {
    conn: Conn,
    lines: BufReader<Conn>,
}

impl Peer {
    fn accept(listener: &Listener, seq: u64, endpoint: &Path) -> Peer {
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
}

/// A fake host per terminal: binds the endpoint like the real one and runs `script` with the
/// listener and the start line; what the script reports goes to the receiver.
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

/// A host the second server must not start: adopted terminals already have theirs.
fn no_new_host() -> PtyHost {
    PtyHost::launch(|_: &HostStart| Err("no host is started on adoption".into()))
}

fn said(rx: &Receiver<String>) -> String {
    rx.recv_timeout(WAIT)
        .expect("the host did not say anything")
}

#[test]
fn a_second_server_adopts_the_host_with_its_id_screen_status_and_tokens() {
    let env = Env::new("fake");
    let (host, rx) = fake(|listener, start, tx| {
        let mut first = Peer::accept(&listener, 0, &start.endpoint);
        let _ = tx.send(format!("first: {:?}", first.read()));
        first.output(1, "hello ");
        // The next connection is the second server; meanwhile the child printed record 2 (with
        // a device attributes query that the first server never saw).
        let mut second = Peer::accept(&listener, 2, &start.endpoint);
        first.send(&HostMessage::Replaced);
        let _ = tx.send(format!("second: {:?}", second.read()));
        second.output(1, "hello ");
        second.output(2, "world\x1b[c");
        // Live output after the replay: its query is answered by the grid (no controller).
        second.output(3, "!\x1b[c");
        let _ = tx.send(format!("second: {:?}", second.read()));
        let _ = tx.send(format!("second: {:?}", second.read()));
        while second.read().is_some() {}
    });
    let (a, a_down) = start(&env.config(1, host));
    let space = a.create_space("work", &env.work()).unwrap().id;
    let id = a
        .create_terminal(&TerminalCreate {
            space_id: space.clone(),
            name: Some("agent".into()),
            command: Some(vec!["fake-shell".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    assert_eq!(said(&rx), "first: Some(Attach { since: 0 })");
    assert!(wait_for_screen(&a, &id, "hello").contains("hello"));
    assert_eq!(
        report_agent(&a, &id, 5, AgentStatus::Blocked)["accepted"],
        true
    );
    a.request(
        "terminal.report_tokens",
        json!({"terminal_id": id, "tokens": {"deliver_step": "review"}, "source": "deliver"}),
    )
    .unwrap();
    a.request(
        "terminal.report_tokens",
        json!({"terminal_id": id, "tokens": {"pr": "#7"}, "ttl_s": 3600, "source": "gh"}),
    )
    .unwrap();
    let before = a.list_terminals(None).unwrap();
    assert!(before[0].survives_restart);
    // The fake host says it is version "fake": older than the one the server ships with.
    assert_eq!(before[0].host_version.as_deref(), Some("fake"));
    assert_eq!(
        before[0].host_bundled_version.as_deref(),
        Some(env!("CARGO_PKG_VERSION"))
    );

    a.shutdown_keeping_terminals().unwrap();
    a_down.recv_timeout(WAIT).unwrap();
    // The list went to server.json with the host's endpoint and the last seq read.
    let state = env.state();
    let stored = &state["terminals"][0];
    assert_eq!(stored["id"], id.as_str());
    assert_eq!(stored["seq"], 1);
    assert_eq!(stored["agent_seq"], 5);
    assert_eq!(stored["agent"]["status"], "blocked");
    assert!(
        PathBuf::from(stored["endpoint"].as_str().unwrap())
            .ends_with(PathBuf::from("hosts").join(&id))
    );

    let (b, _) = start(&env.config(2, no_new_host()));
    assert_eq!(said(&rx), "second: Some(Attach { since: 0 })");
    let terminals = b.list_terminals(None).unwrap();
    assert_eq!(terminals.len(), 1);
    let t = &terminals[0];
    assert_eq!(
        (t.id.as_str(), t.space_id.as_str(), t.name.as_str()),
        (id.as_str(), space.as_str(), "agent")
    );
    assert_eq!(t.status, TerminalStatus::Running);
    assert_eq!((t.cols, t.rows), (90, 20));
    assert!(t.survives_restart);
    assert_eq!(
        t.host_version.as_deref(),
        Some("fake"),
        "read from hello again"
    );
    let agent = t.agent.as_ref().expect("the agent status is back");
    assert_eq!(agent.status, AgentStatus::Blocked);
    assert_eq!(agent.session_id.as_deref(), Some("sess-1"));
    assert_eq!(
        t.tokens.get("deliver_step").map(String::as_str),
        Some("review")
    );
    assert_eq!(t.tokens.get("pr").map(String::as_str), Some("#7"));
    // The tab still points to the terminal.
    let layout = b.list_layout().unwrap();
    assert_eq!(layout.tabs.len(), 1);
    assert_eq!(layout.tabs[0].root, Node::terminal(&id));

    // The grid is rebuilt from the ring: every record once, the replayed query unanswered, the
    // live one answered.
    let s = wait_for_screen(&b, &id, "!");
    assert!(s.contains("hello world!"), "{s:?}");
    assert_eq!(s.matches("hello").count(), 1, "{s:?}");
    assert_eq!(
        said(&rx),
        format!(
            "second: Some(Input {{ bytes: {:?} }})",
            encode_bytes(b"\x1b[?6c")
        )
    );
    // Input goes to the adopted host.
    b.input(&id, "ls\r").unwrap();
    assert_eq!(
        said(&rx),
        format!(
            "second: Some(Input {{ bytes: {:?} }})",
            encode_bytes(b"ls\r")
        )
    );
    // The status seq came along: a stale report stays dropped, the next one is taken.
    assert_eq!(
        report_agent(&b, &id, 5, AgentStatus::Working)["accepted"],
        false
    );
    assert_eq!(
        report_agent(&b, &id, 6, AgentStatus::Working)["accepted"],
        true
    );
    // The first server, which got `replaced`, writes nothing: the list is still the second's.
    thread::sleep(Duration::from_millis(300));
    let state = env.state();
    assert_eq!(state["terminals"][0]["id"], id.as_str());
    assert_eq!(state["terminals"][0]["agent_seq"], 6);
}

#[test]
fn a_gap_of_the_ring_is_one_marker_line_in_the_adopted_grid() {
    let env = Env::new("gap");
    let (host, rx) = fake(|listener, start, tx| {
        let mut first = Peer::accept(&listener, 0, &start.endpoint);
        first.read();
        first.output(1, "early");
        let mut second = Peer::accept(&listener, 40, &start.endpoint);
        let _ = tx.send(format!("{:?}", second.read()));
        // Records 1 to 38 left the ring while no server read it.
        second.send(&HostMessage::Gap { from: 1, to: 38 });
        second.output(39, "late\r\n");
        second.output(40, "$ ready");
        while second.read().is_some() {}
    });
    let (a, a_down) = start(&env.config(1, host));
    let space = a.create_space("work", &env.work()).unwrap().id;
    let id = a
        .create_terminal(&TerminalCreate {
            space_id: space,
            command: Some(vec!["fake-shell".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    wait_for_screen(&a, &id, "early");
    a.shutdown_keeping_terminals().unwrap();
    a_down.recv_timeout(WAIT).unwrap();

    let (b, _) = start(&env.config(2, no_new_host()));
    assert_eq!(said(&rx), "Some(Attach { since: 0 })");
    let s = wait_for_screen(&b, &id, "ready");
    let lines: Vec<&str> = s
        .split("\r\n")
        // The snapshot starts with a reset.
        .map(|l| l.trim_start_matches("\x1bc").trim_end())
        .filter(|l| l.contains("poimen:") || l.contains("late"))
        .collect();
    assert_eq!(
        lines,
        [
            "[poimen: earlier output lost, 38 records left the host's buffer]",
            "late"
        ],
        "{s:?}"
    );
}

/// A terminal list and layout as a previous server left them: space `s1` with tab `tab3`, a
/// split of `t1` (whose host answers) and `t2` (whose host is gone).
fn write_state(env: &Env, t1_endpoint: &Path, t1_seq: u64) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let terminal = |id: &str, endpoint: &Path, seq: u64| {
        json!({
            "id": id, "space_id": "s1", "name": id, "cwd": env.work(), "cols": 80, "rows": 24,
            "endpoint": endpoint, "seq": seq,
            "tokens": [
                {"name": "old", "value": "x", "source": "s", "expires_at_ms": now_ms - 1000},
                {"name": "fresh", "value": "y", "source": "s", "expires_at_ms": now_ms + 600_000},
            ],
        })
    };
    let state = json!({
        "spaces": [{"id": "s1", "name": "work", "cwd": env.work(), "tabs": ["tab3"], "active_tab": "tab3"}],
        "tabs": [{
            "tab_id": "tab3", "space_id": "s1", "name": "dev", "active_terminal": "t2",
            "root": {"kind": "split", "split_id": "split4", "direction": "right", "ratio": 0.5,
                     "children": [{"kind": "terminal", "terminal_id": "t1"},
                                  {"kind": "terminal", "terminal_id": "t2"}]},
        }],
        "next_id": 5,
        "terminals": [
            terminal("t1", t1_endpoint, t1_seq),
            terminal("t2", &env.dir.join("hosts-gone").join("t2"), 0),
        ],
    });
    std::fs::create_dir_all(env.state_file().parent().unwrap()).unwrap();
    std::fs::write(env.state_file(), state.to_string()).unwrap();
}

/// A host for the hand-written state: answers `hello` at `seq` and reports the first line.
fn standing_host(env: &Env, seq: u64) -> (PathBuf, Receiver<String>) {
    let dir = env.dir.join("hosts-up");
    std::fs::create_dir_all(&dir).unwrap();
    let endpoint = dir.join("t1");
    let listener = Listener::bind(&endpoint).unwrap();
    let (tx, rx) = mpsc::channel();
    let at = endpoint.clone();
    thread::spawn(move || {
        let mut peer = Peer::accept(&listener, seq, &at);
        let _ = tx.send(format!("{:?}", peer.read()));
        while peer.read().is_some() {}
    });
    (endpoint, rx)
}

#[test]
fn a_host_that_does_not_answer_is_dropped_and_its_leaf_leaves_the_tree() {
    let env = Env::new("dropped");
    let (endpoint, rx) = standing_host(&env, 3);
    write_state(&env, &endpoint, 3);
    let (b, _) = start(&env.config(2, no_new_host()));
    assert_eq!(said(&rx), "Some(Attach { since: 0 })");
    let terminals = b.list_terminals(None).unwrap();
    let ids: Vec<&str> = terminals.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["t1"]);
    // A token whose TTL ran out while no server was there is gone.
    assert_eq!(
        terminals[0].tokens,
        [("fresh".to_string(), "y".to_string())].into()
    );
    // The split collapsed into the leaf of the terminal that came back (#510).
    let layout = b.list_layout().unwrap();
    assert_eq!(layout.tabs[0].root, Node::terminal("t1"));
    assert_eq!(layout.tabs[0].active_terminal, "t1");
    // And the list in server.json no longer has it.
    let state = env.state();
    assert_eq!(state["terminals"].as_array().unwrap().len(), 1);
    // New ids do not collide with the adopted ones.
    let space = b.create_space("new", &env.work()).unwrap();
    assert_eq!(space.id, "s5");
}

#[test]
fn a_host_behind_the_seq_read_from_it_is_another_one_and_is_not_adopted() {
    let env = Env::new("behind");
    let (endpoint, _rx) = standing_host(&env, 3);
    // The previous server read up to record 10 from t1: a host at 3 is not that host.
    write_state(&env, &endpoint, 10);
    let (b, _) = start(&env.config(2, no_new_host()));
    assert!(b.list_terminals(None).unwrap().is_empty());
    assert!(b.list_layout().unwrap().tabs.is_empty());
}

#[test]
fn a_shutdown_without_keep_terminals_still_ends_them_and_leaves_no_list() {
    let env = Env::new("ends");
    let (host, rx) = fake(|listener, start, tx| {
        let mut first = Peer::accept(&listener, 0, &start.endpoint);
        first.read();
        // `kill` comes on a connection of its own.
        let mut killer = Peer::accept(&listener, 0, &start.endpoint);
        let _ = tx.send(format!("{:?} {:?}", killer.read(), killer.read()));
    });
    let (a, a_down) = start(&env.config(1, host));
    let space = a.create_space("work", &env.work()).unwrap().id;
    a.create_terminal(&TerminalCreate {
        space_id: space,
        command: Some(vec!["fake-shell".into()]),
        ..Default::default()
    })
    .unwrap();
    a.shutdown().unwrap();
    assert_eq!(said(&rx), "Some(Attach { since: 0 }) Some(Kill)");
    a_down.recv_timeout(WAIT).unwrap();
    assert!(env.state().get("terminals").is_none());
}

#[cfg(unix)]
#[test]
fn a_real_shell_keeps_running_across_a_server_restart() {
    let env = Env::new("real");
    let host = || PtyHost::launch(poimen_pty_host::spawn_in_thread);
    let (a, a_down) = start(&env.config(1, host()));
    let space = a.create_space("work", &env.work()).unwrap().id;
    let id = a
        .create_terminal(&TerminalCreate {
            space_id: space,
            command: Some(vec!["sh".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    a.input(&id, "echo before-$((40+2))\n").unwrap();
    assert!(wait_for_screen(&a, &id, "before-42").contains("before-42"));
    report_agent(&a, &id, 1, AgentStatus::Working);
    a.shutdown_keeping_terminals().unwrap();
    a_down.recv_timeout(WAIT).unwrap();

    let (b, _) = start(&env.config(2, host()));
    let terminals = b.list_terminals(None).unwrap();
    assert_eq!(terminals.len(), 1);
    assert_eq!(terminals[0].id, id);
    assert_eq!(
        terminals[0].agent.as_ref().map(|a| a.status),
        Some(AgentStatus::Working)
    );
    assert!(wait_for_screen(&b, &id, "before-42").contains("before-42"));
    // The same shell: its variable is still set.
    b.input(&id, "X=kept; echo after-$X\n").unwrap();
    b.input(&id, "echo again-$X\n").unwrap();
    assert!(wait_for_screen(&b, &id, "again-kept").contains("again-kept"));
    b.close_terminal(&id).unwrap();
}
