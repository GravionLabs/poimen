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
use poimen_client::protocol::{
    AttachMode, Event, Node, SplitDirection, Tab, TabSplit, TerminalCreate, TerminalStatus,
};
use poimen_client::{Client, ClientError};
use poimen_console::session::Error;
use poimen_console::tui::config::Config;
use poimen_console::tui::sidebar::{Model, RowKind};
use poimen_console::tui::{self, Options, Outcome};
use poimen_core::settings::SidebarRows;
use poimen_server::{Config as ServerConfig, PtyHost, Server};
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

/// The TUI on a 100x30 test screen: the pane is 68x28 right of the sidebar.
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
        issues: None,
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
    assert!(text(0).starts_with("▲ work blocked "), "{}", text(0));
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
    // The PTY follows the pane: 100 - 32 sidebar columns, 30 - 1 status line - 1 tab bar.
    wait_until("pty sized to the pane", || f.size_of(&id) == (68, 28));

    t.typed("hello");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    wait_until("echo", || t.pane_text().matches("hello").count() >= 2);

    // The prefix twice sends it: cat shows ^B.
    t.prefix(KeyCode::Char('z'));
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
    wait_until("pty sized to the pane", || f.size_of(&id) == (68, 28));
    t.prefix(KeyCode::Char('b'));
    wait_until("full width", || f.size_of(&id) == (100, 28));
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

impl Fixture {
    fn tabs(&self) -> Vec<Tab> {
        let layout = self.client.list_layout().unwrap();
        let space = layout.spaces.iter().find(|s| s.id == self.space).unwrap();
        space
            .tabs
            .iter()
            .map(|id| {
                layout
                    .tabs
                    .iter()
                    .find(|t| &t.tab_id == id)
                    .unwrap()
                    .clone()
            })
            .collect()
    }

    fn active_tab(&self) -> String {
        let layout = self.client.list_layout().unwrap();
        let space = layout.spaces.iter().find(|s| s.id == self.space).unwrap();
        space.active_tab.clone().unwrap()
    }

    fn tab_of(&self, terminal: &str) -> Tab {
        self.tabs()
            .into_iter()
            .find(|t| t.root.contains_terminal(terminal))
            .unwrap()
    }

    /// Splits `terminal` from outside the TUI (as the app does) with a `cat` terminal.
    fn split_cat(&self, terminal: &str, name: &str) -> String {
        let (_, new) = self
            .client
            .split(&TabSplit {
                terminal_id: terminal.into(),
                direction: SplitDirection::Right,
                ratio: None,
                cwd: None,
                name: Some(name.into()),
                command: Some(vec![
                    "sh".into(),
                    "-c".into(),
                    format!("printf first-{name}; exec cat"),
                ]),
            })
            .unwrap();
        new.id
    }
}

fn is_control_held(f: &Fixture, id: &str) -> bool {
    matches!(
        f.client.attach(id, AttachMode::Control, false),
        Err(ClientError::Server {
            code: poimen_client::protocol::ErrorCode::Conflict,
            ..
        })
    )
}

#[test]
fn tab_bar_lists_the_tabs_and_prefix_keys_create_and_switch_them() {
    let f = fixture("tabs");
    let one = f.cat_terminal("one");
    let two = f.cat_terminal("two");
    let t = start(&f, None);
    t.wait_for("first-one");
    wait_until("tab bar", || {
        t.text()
            .lines()
            .next()
            .is_some_and(|l| l.contains("1:one") && l.contains("2:two"))
    });

    // Next tab shows the other terminal and the server's focus follows.
    t.prefix(KeyCode::Char('n'));
    wait_until("second tab", || t.pane_text().contains("first-two"));
    wait_until("server focus", || f.tab_of(&two).tab_id == f.active_tab());
    t.prefix(KeyCode::Char('p'));
    wait_until("first tab", || f.tab_of(&one).tab_id == f.active_tab());

    // A new tab gets a terminal and the focus, here and on the server.
    t.prefix(KeyCode::Char('c'));
    wait_until("third tab", || f.tabs().len() == 3);
    wait_until("tab bar shows it", || {
        t.text().lines().next().is_some_and(|l| l.contains("3:"))
    });
    wait_until("new tab focused", || {
        f.tab_of_active_terminal_is_new(&[&one, &two])
    });
    assert_eq!(t.detach(), Outcome::Detached);
}

impl Fixture {
    fn tab_of_active_terminal_is_new(&self, old: &[&str]) -> bool {
        let active = self.active_tab();
        let tabs = self.tabs();
        tabs.iter().find(|t| t.tab_id == active).is_some_and(|t| {
            t.root.terminal_ids().iter().all(|id| !old.contains(id)) && tabs.len() == 3
        })
    }
}

#[test]
fn closing_a_tab_with_the_prefix_key_closes_its_terminals_and_shows_another() {
    let f = fixture("closetab");
    let one = f.cat_terminal("one");
    let two = f.cat_terminal("two");
    let t = start(&f, None);
    t.wait_for("first-one");
    t.prefix(KeyCode::Char('&'));
    wait_until("terminal closed", || {
        f.client
            .list_terminals(None)
            .unwrap()
            .iter()
            .all(|x| x.id != one)
    });
    assert_eq!(f.tabs().len(), 1);
    wait_until("other tab shown", || t.pane_text().contains("first-two"));
    assert_eq!(f.status(&two), TerminalStatus::Running);
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn split_keys_go_through_the_server_and_draw_borders() {
    let f = fixture("split");
    let one = f.cat_terminal("one");
    let t = start(&f, None);
    t.wait_for("first-one");

    t.prefix(KeyCode::Char('%'));
    wait_until("server tree split right", || {
        matches!(
            f.tab_of(&one).root,
            Node::Split {
                direction: SplitDirection::Right,
                ..
            }
        )
    });
    // 67 cells after the border: 34 | border | 33. The border is drawn in the column after the first pane.
    wait_until("border drawn", || {
        t.screen.lock().unwrap()[(32 + 34, 5)].symbol() == "│"
    });
    // The new pane has the focus here and on the server; the old one only observes.
    let tab = f.tab_of(&one);
    let new = tab
        .root
        .terminal_ids()
        .into_iter()
        .find(|id| *id != one)
        .unwrap()
        .to_owned();
    wait_until("focus on the new pane", || {
        f.tab_of(&one).active_terminal == new
    });
    wait_until("only the focused pane controls", || {
        is_control_held(&f, &new)
    });
    assert!(!is_control_held(&f, &one));
    // Both panes follow the pane sizes: the focused one is sized to its rectangle.
    wait_until("focused pty sized", || f.size_of(&new) == (33, 28));

    t.prefix(KeyCode::Char('"'));
    wait_until("nested split down", || {
        f.tab_of(&one).root.terminal_ids().len() == 3
    });
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn focus_keys_move_between_panes_and_swap_the_controller() {
    let f = fixture("focus");
    let one = f.cat_terminal("one");
    let right = f.split_cat(&one, "right");
    let t = start(&f, None);
    t.wait_for("first-one");
    t.wait_for("first-right");
    wait_until("one controls", || is_control_held(&f, &one));
    assert!(!is_control_held(&f, &right));

    t.prefix(KeyCode::Char('l'));
    wait_until("server focus on the right pane", || {
        f.tab_of(&one).active_terminal == right
    });
    wait_until("controller moved", || is_control_held(&f, &right));
    wait_until("old controller released", || !is_control_held(&f, &one));

    // Typing goes to the focused pane only.
    t.typed("abc");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    wait_until("echo in the right pane", || {
        t.pane_text().matches("abc").count() >= 2
    });
    t.prefix(KeyCode::Char('h'));
    wait_until("back on the left", || f.tab_of(&one).active_terminal == one);
    // No pane further left: nothing changes.
    t.prefix(KeyCode::Char('h'));
    thread::sleep(Duration::from_millis(200));
    assert_eq!(f.tab_of(&one).active_terminal, one);
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn resize_keys_move_the_border_on_the_server() {
    let f = fixture("resize");
    let one = f.cat_terminal("one");
    f.split_cat(&one, "right");
    let t = start(&f, None);
    t.wait_for("first-right");
    let ratio = || match f.tab_of(&one).root {
        Node::Split { ratio, .. } => ratio,
        _ => panic!("not split"),
    };
    assert!((ratio() - 0.5).abs() < 1e-9);
    // The focus is on the first pane: L moves the border right.
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    t.key(KeyCode::Char('L'), KeyModifiers::SHIFT);
    wait_until("ratio grew", || (ratio() - 0.55).abs() < 1e-9);
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    t.key(KeyCode::Char('H'), KeyModifiers::SHIFT);
    t.key(KeyCode::Char('b'), KeyModifiers::CONTROL);
    t.key(KeyCode::Char('H'), KeyModifiers::SHIFT);
    wait_until("ratio shrank", || (ratio() - 0.45).abs() < 1e-9);
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn closing_a_pane_collapses_the_split_and_focuses_the_sibling() {
    let f = fixture("closepane");
    let one = f.cat_terminal("one");
    let right = f.split_cat(&one, "right");
    let t = start(&f, None);
    t.wait_for("first-right");
    t.prefix(KeyCode::Char('l'));
    wait_until("on the right", || f.tab_of(&one).active_terminal == right);
    t.prefix(KeyCode::Char('x'));
    wait_until("pane closed", || {
        f.client
            .list_terminals(None)
            .unwrap()
            .iter()
            .all(|x| x.id != right)
    });
    assert!(matches!(f.tab_of(&one).root, Node::Terminal { .. }));
    wait_until("sibling controls", || is_control_held(&f, &one));
    t.typed("zzz");
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    wait_until("sibling echoes", || {
        t.pane_text().matches("zzz").count() >= 2
    });
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn changes_made_elsewhere_show_up_live() {
    let f = fixture("live");
    let one = f.cat_terminal("one");
    let t = start(&f, None);
    t.wait_for("first-one");

    // Another client (the app) splits: the new pane appears with its own grid.
    let right = f.split_cat(&one, "right");
    t.wait_for("first-right");
    wait_until("border", || {
        t.screen.lock().unwrap()[(32 + 34, 5)].symbol() == "│"
    });
    // The focus stays where it was, so does the controller.
    assert!(is_control_held(&f, &one));
    assert!(!is_control_held(&f, &right));

    // The app resizes the split: the border moves.
    let tab = f.tab_of(&one);
    let Node::Split { split_id, .. } = &tab.root else {
        panic!("not split")
    };
    f.client.resize_split(&tab.tab_id, split_id, 0.25).unwrap();
    // 67 cells after the border: 17 | border | 50.
    wait_until("border moved", || {
        t.screen.lock().unwrap()[(32 + 17, 5)].symbol() == "│"
    });

    // The app focuses the other pane: the TUI follows, and the controller moves.
    f.client.focus_tab(&tab.tab_id, Some(&right)).unwrap();
    wait_until("controller follows the focus", || {
        is_control_held(&f, &right)
    });

    // The app opens a new tab: it shows up in the tab bar.
    f.client
        .create_tab(&poimen_client::protocol::TabCreate {
            space_id: f.space.clone(),
            name: Some("fresh".into()),
            terminal_id: None,
        })
        .unwrap();
    wait_until("tab bar updated", || {
        t.text()
            .lines()
            .next()
            .is_some_and(|l| l.contains("2:fresh"))
    });

    // The app closes the pane in focus: the TUI collapses to the sibling.
    f.client.close_terminal(&right).unwrap();
    wait_until("sibling alone", || !t.pane_text().contains('│'));
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn the_terminal_the_tui_runs_in_is_not_attached_in_a_split() {
    let f = fixture("ownsplit");
    let own = f.cat_terminal("self");
    f.split_cat(&own, "other");
    let t = start(&f, Some(own.clone()));
    t.wait_for("first-other");
    // The pane of the own terminal stays empty; nothing attached to it.
    assert!(t.pane_text().contains("poimen runs here"));
    assert!(!t.pane_text().contains("first-self"));
    assert!(!is_control_held(&f, &own));
    assert_eq!(t.detach(), Outcome::Detached);
}

fn report(f: &Fixture, id: &str, seq: u64, status: &str, message: Option<&str>) {
    let mut params = json!({"terminal_id": id, "seq": seq, "status": status, "agent": "claude"});
    if let Some(m) = message {
        params["message"] = json!(m);
    }
    f.client.request("terminal.report_agent", params).unwrap();
}

/// A first terminal (shown) and an agent terminal `other` that works.
fn two_with_a_working_agent(name: &str) -> (Fixture, String) {
    let f = fixture(name);
    f.cat_terminal("shell");
    let other = f.cat_terminal("other");
    report(&f, &other, 1, "working", None);
    (f, other)
}

#[test]
fn a_blocked_terminal_that_is_not_viewed_rings_and_viewing_clears_it() {
    let (f, other) = two_with_a_working_agent("ring");
    let t = start(&f, None);
    t.wait_for("working");
    report(&f, &other, 2, "blocked", Some("Allow rm?"));
    wait_until("ring", || t.text().contains("1 to see"));
    assert!(
        t.text()
            .lines()
            .any(|l| l.contains("other") && l.contains('!'))
    );

    t.prefix(KeyCode::Down);
    wait_until("mark cleared", || !t.text().contains("to see"));
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn a_done_terminal_that_is_not_viewed_counts_as_unread() {
    let (f, other) = two_with_a_working_agent("unread");
    let t = start(&f, None);
    t.wait_for("working");
    report(&f, &other, 2, "done", None);
    wait_until("unread", || {
        t.text()
            .lines()
            .any(|l| l.contains("other") && l.contains('*'))
    });
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn the_notification_list_focuses_the_terminal_of_the_entry() {
    let (f, other) = two_with_a_working_agent("list");
    let t = start(&f, None);
    t.wait_for("working");
    report(&f, &other, 2, "blocked", Some("Allow rm?"));
    wait_until("ring", || t.text().contains("1 to see"));

    t.prefix(KeyCode::Char('N'));
    wait_until("list", || t.text().contains("Allow rm?"));
    assert!(t.text().contains("other (claude) · blocked"));
    t.key(KeyCode::Enter, KeyModifiers::NONE);
    wait_until("focused", || t.pane_text().contains("first-other"));
    assert!(!t.text().contains("Notifications ·"));
    assert_eq!(t.detach(), Outcome::Detached);
}

#[test]
fn an_empty_notification_list_says_so_and_escape_closes_it() {
    let f = fixture("empty-list");
    f.cat_terminal("shell");
    let t = start(&f, None);
    t.wait_for("first-shell");
    t.prefix(KeyCode::Char('N'));
    t.wait_for("no notifications");
    t.key(KeyCode::Esc, KeyModifiers::NONE);
    wait_until("closed", || !t.text().contains("no notifications"));
    assert_eq!(t.detach(), Outcome::Detached);
}
