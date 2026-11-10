//! The TUI against an in-process poimen-server on a temporary socket (never the user's server),
//! drawn on ratatui's test backend. Unix-only: the terminals run POSIX shell scripts.
#![cfg(unix)]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossterm::event::{Event as InputEvent, KeyCode, KeyEvent, KeyModifiers};
use poimen_client::protocol::{AttachMode, Event, TerminalCreate, TerminalStatus};
use poimen_client::{Client, ClientError};
use poimen_console::session::Error;
use poimen_console::tui::config::Config;
use poimen_console::tui::sidebar::{Model, RowKind};
use poimen_console::tui::{self, Options, Outcome};
use poimen_core::settings::SidebarRows;
use poimen_server::{Config as ServerConfig, Server};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use serde_json::json;

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
    let dir = std::env::temp_dir().join(format!("poimen-tui-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let config = ServerConfig {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        scrollback: 1000,
        home: None,
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
    /// A terminal named `name` that prints `first-<name>`, then echoes what it is typed (`cat`).
    fn cat_terminal(&self, name: &str) -> String {
        self.client
            .create_terminal(&TerminalCreate {
                space_id: self.space.clone(),
                name: Some(name.into()),
                command: Some(vec![
                    "sh".into(),
                    "-c".into(),
                    format!("printf first-{name}; exec cat"),
                ]),
                ..Default::default()
            })
            .unwrap()
            .id
    }

    fn status(&self, id: &str) -> TerminalStatus {
        let t = self.client.list_terminals(None).unwrap();
        t.iter().find(|t| t.id == id).unwrap().status
    }

    fn size_of(&self, id: &str) -> (u16, u16) {
        let t = self.client.list_terminals(None).unwrap();
        let t = t.iter().find(|t| t.id == id).unwrap();
        (t.cols, t.rows)
    }
}

fn wait_until(what: &str, mut pred: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !pred() {
        assert!(Instant::now() < deadline, "timed out: {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

struct Running {
    input: Sender<InputEvent>,
    screen: Arc<Mutex<Buffer>>,
    stop: Arc<AtomicBool>,
    handle: JoinHandle<Result<Outcome, Error>>,
}

/// The TUI on a 100x30 test screen: the pane is 68x29 right of the sidebar.
fn start(f: &Fixture, own_terminal: Option<String>) -> Running {
    let (input, rx) = mpsc::channel();
    let screen = Arc::new(Mutex::new(Buffer::empty(Default::default())));
    let stop = Arc::new(AtomicBool::new(false));
    let client = Client::new(f.client.socket());
    let opts = Options {
        config: Config::default(),
        home: None,
        own_terminal,
        mirror: Some(screen.clone()),
    };
    let s = stop.clone();
    let handle = thread::spawn(move || {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        tui::run(&client, &opts, rx, &mut terminal, &s)
    });
    Running {
        input,
        screen,
        stop,
        handle,
    }
}

impl Running {
    fn text(&self) -> String {
        let buf = self.screen.lock().unwrap();
        let a = buf.area;
        (a.top()..a.bottom())
            .map(|y| {
                (a.left()..a.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The pane's part of the screen (right of the sidebar, above the status line).
    fn pane_text(&self) -> String {
        let buf = self.screen.lock().unwrap();
        let a = buf.area;
        (a.top()..a.bottom().saturating_sub(1))
            .map(|y| {
                (32..a.right())
                    .map(|x| buf[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.text().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "`{needle}` did not show:\n{}",
                self.text()
            );
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn key(&self, code: KeyCode, mods: KeyModifiers) {
        self.input
            .send(InputEvent::Key(KeyEvent::new(code, mods)))
            .unwrap();
    }

    fn typed(&self, text: &str) {
        for c in text.chars() {
            self.key(KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    fn prefix(&self, code: KeyCode) {
        self.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
        self.key(code, KeyModifiers::NONE);
    }

    fn detach(self) -> Outcome {
        self.prefix(KeyCode::Char('d'));
        self.handle.join().unwrap().unwrap()
    }
}

#[test]
fn sidebar_model_comes_from_the_server_with_agent_status_and_tokens() {
    let f = fixture("model");
    let shell = f.cat_terminal("shell");
    let agent = f.cat_terminal("claude");
    f.client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": agent, "seq": 1, "status": "blocked", "agent": "claude"}),
        )
        .unwrap();
    f.client
        .request(
            "terminal.report_tokens",
            json!({"terminal_id": shell, "tokens": {"cost": "$0.40"}, "source": "test"}),
        )
        .unwrap();
    let config = SidebarRows {
        terminal: Some(vec![
            ["state_icon", "name", "$cost"]
                .iter()
                .map(|t| poimen_core::settings::RowToken {
                    token: (*t).into(),
                    bold: false,
                    dim: false,
                })
                .collect(),
        ]),
        ..SidebarRows::default()
    };
    let model = Model::load(&f.client).unwrap();
    let rows = model.rows(&config, None);
    let text = |i: usize| {
        rows[i].lines[0]
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };
    assert_eq!(rows.len(), 3, "{rows:#?}");
    assert_eq!(rows[0].kind, RowKind::Space);
    assert_eq!(text(0), "▲ work blocked");
    assert_eq!(rows[1].terminal_id.as_deref(), Some(shell.as_str()));
    assert_eq!(text(1), "· shell $0.40");
    assert_eq!(text(2), "▲ claude blocked");
    // The first tab stays the active one.
    assert_eq!(model.initial(None).as_deref(), Some(shell.as_str()));
}

#[test]
fn tui_attaches_as_controller_types_and_detaches_leaving_the_terminals_running() {
    let f = fixture("control");
    let id = f.cat_terminal("shell");
    let t = start(&f, None);
    t.wait_for("first-shell");
    t.wait_for("shell · control");
    // The PTY follows the pane: 100 - 32 sidebar columns, 30 - 1 status line.
    wait_until("pty sized to the pane", || f.size_of(&id) == (68, 29));

    t.typed("hello");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    wait_until("echo", || t.pane_text().matches("hello").count() >= 2);

    // The prefix twice sends it: cat shows ^B.
    t.prefix(KeyCode::Char('x'));
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    wait_until("prefix passed through", || t.pane_text().contains("^B"));

    assert_eq!(t.detach(), Outcome::Detached);
    assert_eq!(f.status(&id), TerminalStatus::Running);
    // Detaching gave the control back: another client gets it without a conflict.
    wait_until("control released", || {
        f.client.attach(&id, AttachMode::Control, false).is_ok()
    });
}

#[test]
fn moving_the_selection_attaches_to_that_terminal() {
    let f = fixture("select");
    let a = f.cat_terminal("alpha");
    let b = f.cat_terminal("beta");
    let t = start(&f, None);
    t.wait_for("first-alpha");
    assert!(t.text().contains("beta"), "sidebar lists both");

    t.prefix(KeyCode::Down);
    wait_until("beta in the pane", || t.pane_text().contains("first-beta"));
    t.wait_for("beta · control");
    t.typed("to-beta");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    wait_until("input reaches beta", || {
        t.pane_text().matches("to-beta").count() >= 2
    });

    t.prefix(KeyCode::Up);
    wait_until("alpha back", || t.pane_text().contains("first-alpha"));
    assert!(!t.pane_text().contains("to-beta"));

    assert_eq!(t.detach(), Outcome::Detached);
    assert_eq!(f.status(&a), TerminalStatus::Running);
    assert_eq!(f.status(&b), TerminalStatus::Running);
}

#[test]
fn an_existing_controller_is_taken_over_only_after_asking() {
    let f = fixture("takeover");
    let id = f.cat_terminal("shell");
    let mut other = f.client.attach(&id, AttachMode::Control, false).unwrap();
    let t = start(&f, None);
    t.wait_for("take over? [y/N]");
    t.wait_for("shell · observing");

    // As observer nothing typed reaches the terminal.
    t.typed("zzz");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    thread::sleep(Duration::from_millis(300));
    assert!(!t.pane_text().contains("zzz"));
    assert!(t.text().contains("take over? [y/N]"));

    t.key(KeyCode::Char('y'), KeyModifiers::NONE);
    t.wait_for("shell · control");
    loop {
        match other.next_record() {
            Ok(Event::TerminalDetached { .. }) => break,
            Ok(_) => {}
            Err(ClientError::Io(e)) => panic!("{e}"),
            Err(e) => panic!("{e}"),
        }
    }
    t.typed("mine");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    wait_until("input after take-over", || {
        t.pane_text().matches("mine").count() >= 2
    });
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn declining_the_take_over_keeps_observing() {
    let f = fixture("decline");
    let id = f.cat_terminal("shell");
    let _other = f.client.attach(&id, AttachMode::Control, false).unwrap();
    let t = start(&f, None);
    t.wait_for("take over? [y/N]");
    t.key(KeyCode::Char('n'), KeyModifiers::NONE);
    wait_until("question gone", || !t.text().contains("take over?"));
    assert!(t.text().contains("shell · observing"));
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn server_events_update_the_sidebar() {
    let f = fixture("events");
    let first = f.cat_terminal("first");
    let t = start(&f, None);
    t.wait_for("first-first");

    f.cat_terminal("newcomer");
    t.wait_for("newcomer");
    f.client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": first, "seq": 1, "status": "working", "agent": "claude"}),
        )
        .unwrap();
    t.wait_for("working");

    // Closing the attached terminal (from elsewhere) moves the pane to another one.
    f.client.close_terminal(&first).unwrap();
    wait_until("pane moved", || t.pane_text().contains("first-newcomer"));
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn own_terminal_is_never_attached() {
    let f = fixture("own");
    let own = f.cat_terminal("self");
    f.cat_terminal("other");
    let t = start(&f, Some(own.clone()));
    wait_until("other attached", || t.pane_text().contains("first-other"));
    t.prefix(KeyCode::Down);
    thread::sleep(Duration::from_millis(300));
    assert!(!t.pane_text().contains("first-self"));
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn sidebar_toggles_and_the_pane_takes_the_width() {
    let f = fixture("toggle");
    let id = f.cat_terminal("shell");
    let t = start(&f, None);
    wait_until("pty sized to the pane", || f.size_of(&id) == (68, 29));
    t.prefix(KeyCode::Char('b'));
    wait_until("full width", || f.size_of(&id) == (100, 29));
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn stop_flag_interrupts_and_keeps_the_terminals() {
    let f = fixture("stop");
    let id = f.cat_terminal("shell");
    let t = start(&f, None);
    t.wait_for("first-shell");
    t.stop.store(true, Ordering::SeqCst);
    assert_eq!(t.handle.join().unwrap().unwrap(), Outcome::Interrupted);
    assert_eq!(f.status(&id), TerminalStatus::Running);
}

#[test]
fn empty_server_shows_a_hint() {
    let f = fixture("empty");
    let t = start(&f, None);
    t.wait_for("No terminals");
    assert_eq!(t.detach(), Outcome::Detached);
}
