//! Compatibility with the last stable release in both directions (#969, ADR 0009):
//!
//! 1. the server of this tree adopts a `poimen-pty-host` built from the last release;
//! 2. the client of this tree pings a `poimen-server` of the last release and attaches to its
//!    terminal.
//!
//! The CI job `previous-release` builds the last stable tag in a second checkout and sets
//! `POIMEN_PREVIOUS_RELEASE_DIR` (the `target/debug` of that build) and `POIMEN_PREVIOUS_TAG`;
//! `POIMEN_REQUIRE_PREVIOUS=1` makes a missing one a failure. Without them (a local `cargo test`)
//! the tests say so and pass. Every server and host lives in a temp directory.

#![cfg(unix)]

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use poimen_client::Client;
use poimen_client::protocol::host::HostStart;
use poimen_client::protocol::{AttachMode, Event, PROTOCOL_VERSION, TerminalCreate, decode_bytes};
use poimen_server::{Config, PtyHost, Server};

const WAIT: Duration = Duration::from_secs(15);

/// The last release: its tag and the directory with its binaries.
struct Previous {
    tag: String,
    bin: PathBuf,
}

impl Previous {
    fn from_env() -> Option<Self> {
        let dir = std::env::var_os("POIMEN_PREVIOUS_RELEASE_DIR").filter(|v| !v.is_empty());
        let tag = std::env::var("POIMEN_PREVIOUS_TAG")
            .ok()
            .filter(|v| !v.is_empty());
        match (dir, tag) {
            (Some(dir), Some(tag)) => Some(Self {
                tag,
                bin: PathBuf::from(dir),
            }),
            _ => {
                assert!(
                    std::env::var_os("POIMEN_REQUIRE_PREVIOUS").is_none(),
                    "POIMEN_PREVIOUS_RELEASE_DIR and POIMEN_PREVIOUS_TAG are required here"
                );
                eprintln!("skipped: no previous release build (POIMEN_PREVIOUS_RELEASE_DIR)");
                None
            }
        }
    }

    fn binary(&self, name: &str) -> PathBuf {
        let path = self.bin.join(name);
        assert!(
            path.is_file(),
            "the build of {} has no {}",
            self.tag,
            path.display()
        );
        path
    }

    /// Names the two versions in a failure.
    fn versions(&self) -> String {
        format!(
            "this tree poimen-server {} (protocol {PROTOCOL_VERSION}) and the last release {}",
            env!("CARGO_PKG_VERSION"),
            self.tag
        )
    }
}

struct Dir(PathBuf);

impl Dir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-prev-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self(dir)
    }

    fn work(&self) -> String {
        self.0
            .join("work")
            .canonicalize()
            .unwrap()
            .display()
            .to_string()
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Reads records of the attachment until the screen (snapshot plus output) has `needle`.
fn read_until(att: &mut poimen_client::Attachment, needle: &str, context: &str) {
    att.set_read_timeout(Some(WAIT)).unwrap();
    let mut screen =
        String::from_utf8_lossy(&decode_bytes(&att.info.snapshot).unwrap()).into_owned();
    while !screen.contains(needle) {
        match att.next_record() {
            Ok(Event::TerminalOutput { data, .. }) => {
                screen.push_str(&String::from_utf8_lossy(&decode_bytes(&data).unwrap()));
            }
            Ok(_) => {}
            Err(e) => {
                panic!("{context}: waiting for {needle:?} failed with {e}; screen {screen:?}")
            }
        }
    }
}

fn terminal(client: &Client, space: &str, script: &str) -> String {
    client
        .create_terminal(&TerminalCreate {
            space_id: space.into(),
            command: Some(vec!["sh".into(), "-c".into(), script.into()]),
            ..Default::default()
        })
        .unwrap()
        .id
}

fn config(dir: &Dir, n: u32, pty_host: PtyHost) -> Config {
    Config {
        socket: dir.0.join(format!("run{n}")).join("server.sock"),
        state_file: dir.0.join("state").join("server.json"),
        history_dir: dir
            .0
            .join("state")
            .join("server.json")
            .with_file_name("history"),
        scrollback: 100,
        home: None,
        pty_host,
    }
}

fn start(config: &Config) -> (Client, mpsc::Receiver<()>) {
    let server = Server::bind(config.clone()).unwrap();
    let (tx, rx) = mpsc::channel();
    server.on_shutdown(move || {
        let _ = tx.send(());
    });
    thread::spawn(move || server.serve());
    (Client::new(&config.socket), rx)
}

#[test]
fn the_server_of_this_tree_adopts_a_pty_host_of_the_last_release() {
    let Some(prev) = Previous::from_env() else {
        return;
    };
    let ctx = format!(
        "adopting a poimen-pty-host of the last release: {}",
        prev.versions()
    );
    let dir = Dir::new("adopt");
    let host = PtyHost::Binary(prev.binary("poimen-pty-host"));
    let (a, a_down) = start(&config(&dir, 1, host));
    let space = a.create_space("work", &dir.work()).unwrap().id;
    // The shell keeps a variable: the same process must come back, not a new one.
    let id = terminal(&a, &space, "printf 'before-%s\\n' 42; exec cat");
    let mut att = a.attach(&id, AttachMode::Observe, false).unwrap();
    read_until(&mut att, "before-42", &ctx);
    drop(att);
    a.shutdown_keeping_terminals().unwrap();
    a_down.recv_timeout(WAIT).unwrap();

    let no_new_host = PtyHost::launch(|_: &HostStart| Err("no host starts on adoption".into()));
    let (b, _) = start(&config(&dir, 2, no_new_host));
    let terminals = b.list_terminals(None).unwrap();
    assert!(
        terminals.len() == 1 && terminals[0].id == id,
        "{ctx}: the terminal was not adopted, the server lists {terminals:?}"
    );
    let mut att = b.attach(&id, AttachMode::Control, false).unwrap();
    read_until(&mut att, "before-42", &ctx);
    // Input reaches the child through the old host, and its answer comes back.
    att.input("after-adoption\r").unwrap();
    read_until(&mut att, "after-adoption", &ctx);
    drop(att);
    b.close_terminal(&id).unwrap();
}

/// The server of the last release as a process on a socket of its own.
struct OldServer {
    child: Child,
    dir: Dir,
}

impl Drop for OldServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// Unix only (the file is `#![cfg(unix)]`): no console window to hide.
#[allow(clippy::disallowed_methods)]
fn old_server(prev: &Previous, dir: Dir) -> (OldServer, Client) {
    let socket: &Path = &dir.0.join("run").join("server.sock");
    let mut child = Command::new(prev.binary("poimen-server"))
        .env("POIMEN_SOCKET", socket)
        .env("POIMEN_STATE_DIR", dir.0.join("state"))
        .env("HOME", &dir.0)
        .env_remove("XDG_STATE_HOME")
        .env_remove("XDG_RUNTIME_DIR")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("cannot start the server of {}: {e}", prev.tag));
    // The server says where it listens once the socket is bound.
    let mut lines = BufReader::new(child.stderr.take().unwrap());
    let mut seen = String::new();
    loop {
        let mut line = String::new();
        let n = lines.read_line(&mut line).unwrap_or(0);
        seen.push_str(&line);
        if line.contains("listening on") {
            break;
        }
        assert!(n > 0, "the server of {} ended at start: {seen}", prev.tag);
    }
    // Keep reading so the server never blocks on a full pipe.
    thread::spawn(move || for _ in lines.lines() {});
    let client = Client::new(socket);
    (OldServer { child, dir }, client)
}

#[test]
fn the_client_of_this_tree_talks_to_the_server_of_the_last_release() {
    let Some(prev) = Previous::from_env() else {
        return;
    };
    let ctx = format!(
        "the client of this tree against the server of the last release: {}",
        prev.versions()
    );
    let (old, client) = old_server(&prev, Dir::new("client"));

    // The classification of the app (`poimen_server::classify`) keys on the protocol version and
    // then on the version: another protocol is `Incompatible`, a lower version `Outdated`.
    let ping = client
        .ping()
        .unwrap_or_else(|e| panic!("{ctx}: ping failed with {e}"));
    // Protocols 9 (#1451), 10 (#1452, worktree starts in a grant) and 11 (#1454, `grant_lost`)
    // are a step the app handles through `Incompatible`: the last release (0.40.1) speaks 8. Set
    // this to `PROTOCOL_VERSION` again once a release with protocol 11 is the last one.
    assert_eq!(
        ping.protocol,
        PROTOCOL_VERSION - 3,
        "{ctx}: the server speaks protocol {} (version {}), this tree {PROTOCOL_VERSION}: the \
         expected step is three protocol versions (docs/agents/backend-and-server.md, \"Compatibility \
         with the last release\")",
        ping.protocol,
        ping.version
    );

    let space = client.create_space("work", &old.dir.work()).unwrap().id;
    let id = terminal(&client, &space, "printf 'ready-%s\\n' 7; exec cat");
    let mut att = client
        .attach(&id, AttachMode::Control, false)
        .unwrap_or_else(|e| panic!("{ctx}: terminal.attach failed with {e}"));
    read_until(&mut att, "ready-7", &ctx);
    att.input("typed-in\r").unwrap();
    read_until(&mut att, "typed-in", &ctx);
    drop(att);
    client.shutdown().unwrap();
}
