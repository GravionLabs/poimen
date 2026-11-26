//! The TUI and ended terminals in a tab (#1305): a scheduled run keeps its output and exit code
//! in its pane, and an ended terminal that is resumed shows its new shell. Against an in-process
//! server. Unix-only: the commands are POSIX shell scripts.
#![cfg(unix)]

mod common;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossterm::event::{Event as InputEvent, KeyCode, KeyEvent, KeyModifiers};
use poimen_client::Client;
use poimen_client::protocol::{AttachMode, TerminalCreate, decode_bytes};
use poimen_console::session::Error;
use poimen_console::tui::config::Config;
use poimen_console::tui::{self, Options, Outcome};
use poimen_server::{Config as ServerConfig, PtyHost, Server};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use serde_json::json;

/// 2026-01-10 08:59 UTC, a minute before the due time of `0 9 * * *`.
const BEFORE: u64 = 1_768_003_200_000 + (9 * 60 - 1) * 60_000;

fn wait_until(what: &str, mut pred: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !pred() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        thread::yield_now();
    }
}

fn screen_of(client: &Client, id: &str) -> String {
    let attachment = client.attach(id, AttachMode::Observe, false).unwrap();
    let bytes = decode_bytes(&attachment.info.snapshot).unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

struct Running {
    input: Sender<InputEvent>,
    screen: Arc<Mutex<Buffer>>,
    handle: JoinHandle<Result<Outcome, Error>>,
}

fn start(client: &Client) -> Running {
    let (input, rx) = mpsc::channel();
    let screen = Arc::new(Mutex::new(Buffer::empty(Default::default())));
    let client = Client::new(client.socket());
    let opts = Options {
        config: Config::default(),
        home: None,
        own_terminal: None,
        mirror: Some(screen.clone()),
        issues: None,
    };
    let handle = thread::spawn(move || {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let stop = AtomicBool::new(false);
        tui::run(&client, &opts, rx, &mut terminal, &Arc::new(stop))
    });
    Running {
        input,
        screen,
        handle,
    }
}

impl Running {
    fn text(&self) -> String {
        common::screen_text(&self.screen)
    }

    fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.text().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "`{needle}` did not show:\n{}",
                self.text()
            );
            thread::yield_now();
        }
    }

    /// The pane shows the output and the status line the exit code, without a notice.
    fn shows_the_ended_run(&self) {
        self.wait_for("exited with 3");
        self.wait_for("job-done");
        assert!(!self.text().contains("attachment ended"), "{}", self.text());
    }

    fn detach(self) {
        for (code, mods) in [('b', KeyModifiers::CONTROL), ('d', KeyModifiers::NONE)] {
            let key = KeyEvent::new(KeyCode::Char(code), mods);
            self.input.send(InputEvent::Key(key)).unwrap();
        }
        let outcome = common::join_within(self.handle, &self.screen).unwrap();
        assert_eq!(outcome, Outcome::Detached);
    }
}

#[test]
fn an_ended_scheduled_run_keeps_its_output_and_exit_code_in_the_pane() {
    let guard = tempfile::tempdir().unwrap();
    let dir = guard.path().canonicalize().unwrap();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("hold"), "").unwrap();
    let home = dir.join("home");
    let config = poimen_core::settings::config_path(&home);
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    let job = "while [ -e hold ]; do sleep 0.05; done; echo job-done; exit 3";
    let user = json!({
        "tasks": {"modes": [{"name": "job", "command": ["sh", "-c", job], "worktree": false}]},
        "schedules": [{"cron": "0 9 * * *", "timezone": "UTC",
                       "dir": work.display().to_string(), "mode": "job"}],
    });
    std::fs::write(config, user.to_string()).unwrap();

    let socket = dir.join("run").join("server.sock");
    let server = Server::bind(ServerConfig {
        socket: socket.clone(),
        state_file: dir.join("state").join("server.json"),
        history_dir: dir.join("state").join("history"),
        scrollback: 1000,
        home: Some(home),
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
    })
    .unwrap();
    let clock = Arc::new(AtomicU64::new(BEFORE));
    let now = Arc::clone(&clock);
    server.set_clock(move || now.load(Ordering::SeqCst));
    let sweep = server.schedule_sweeper();
    thread::spawn(move || server.serve());
    let client = Client::new(&socket);
    // The run's tab is the only one: it lands in this space, which has no terminal yet.
    client
        .create_space("work", &work.display().to_string())
        .unwrap();
    // The first sweep registers the schedule; one a minute later starts the run.
    sweep();
    clock.store(BEFORE + 60_000, Ordering::SeqCst);
    sweep();
    wait_until("the run starts", || {
        !client.list_terminals(None).unwrap().is_empty()
    });

    // Watched while it ends.
    let t = start(&client);
    t.wait_for("schedule job · control");
    std::fs::remove_file(work.join("hold")).unwrap();
    t.shows_the_ended_run();
    // Another server event makes the TUI read the model again; the pane stays.
    let space = client.list_terminals(None).unwrap()[0].space_id.clone();
    client
        .create_terminal(&TerminalCreate {
            space_id: space,
            name: Some("later".into()),
            command: Some(vec!["cat".into()]),
            ..Default::default()
        })
        .unwrap();
    t.wait_for("later");
    t.shows_the_ended_run();
    t.detach();

    // Opened after it ended.
    let t = start(&client);
    t.shows_the_ended_run();
    t.detach();
}

#[test]
fn a_resumed_terminal_in_an_observed_pane_shows_its_new_shell() {
    let guard = tempfile::tempdir().unwrap();
    let dir = guard.path().canonicalize().unwrap();
    let state_file = dir.join("state").join("server.json");
    let config = |run: &str| ServerConfig {
        socket: dir.join(run).join("server.sock"),
        history_dir: dir.join("state").join("history"),
        state_file: state_file.clone(),
        scrollback: 1000,
        home: None,
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
    };
    let cat = || {
        ["sh", "-c", "echo earlier-work; exec cat"]
            .map(String::from)
            .to_vec()
    };

    // A split tab; the second terminal reports an agent session.
    let first = config("run1");
    let server = Server::bind(first.clone()).unwrap();
    let (down, stopped) = mpsc::channel();
    server.on_shutdown(move || {
        let _ = down.send(());
    });
    thread::spawn(move || server.serve());
    let client = Client::new(&first.socket);
    let space = client
        .create_space("work", &dir.display().to_string())
        .unwrap()
        .id;
    let running = client
        .create_terminal(&TerminalCreate {
            space_id: space,
            command: Some(vec!["cat".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    let split = client
        .request(
            "tab.split",
            json!({"terminal_id": running, "direction": "right", "command": cat()}),
        )
        .unwrap();
    let ended = split["terminal"]["id"].as_str().unwrap().to_owned();
    wait_until("the output arrives", || {
        screen_of(&client, &ended).contains("earlier-work")
    });
    client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": ended, "seq": 1, "status": "idle", "agent": "claude",
                   "session_id": "session-1"}),
        )
        .unwrap();
    client
        .request("server.shutdown", json!({"keep_terminals": true}))
        .unwrap();
    stopped.recv_timeout(Duration::from_secs(10)).unwrap();

    // Its host is gone for the next server: it adopts the terminal from the history.
    let text = std::fs::read_to_string(&state_file).unwrap();
    let state: serde_json::Value = serde_json::from_str(&text).unwrap();
    let endpoint = state["terminals"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"] == ended.as_str())
        .and_then(|t| t["endpoint"].as_str())
        .unwrap();
    std::fs::remove_file(endpoint).unwrap();
    let second = config("run2");
    let server = Server::bind(second.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&second.socket);

    let t = start(&client);
    t.wait_for("earlier-work");
    // Another client takes the control, which resumes the session in a new shell.
    let _controller = client.attach(&ended, AttachMode::Control, false).unwrap();
    t.wait_for("session-1");
    t.detach();
}
