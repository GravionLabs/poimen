//! The TUI reloads `config.json` while it runs (#1175): against an in-process poimen-server on a
//! temporary socket and a temporary home (never the user's server or config). Unix-only.
#![cfg(unix)]

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossterm::event::{Event as InputEvent, KeyCode, KeyEvent, KeyModifiers};
use poimen_client::Client;
use poimen_console::session::Error;
use poimen_console::tui::{self, Options, Outcome};
use poimen_server::{Config as ServerConfig, PtyHost, Server};
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

struct Fixture {
    dir: PathBuf,
    home: PathBuf,
    client: Client,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn fixture(name: &str, config: &str) -> Fixture {
    let dir = PathBuf::from("/tmp").join(format!("pr-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let home = dir.join("home");
    let file = poimen_core::settings::config_path(&home);
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, config).unwrap();
    let server_config = ServerConfig {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        history_dir: dir.join("state").join("history"),
        scrollback: 1000,
        home: Some(home.clone()),
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
    };
    let server = Server::bind(server_config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&server_config.socket);
    client
        .create_space("work", &dir.join("work").display().to_string())
        .unwrap();
    Fixture { dir, home, client }
}

impl Fixture {
    /// Replaces the config the way an editor does: a temp file renamed over it.
    fn write_config(&self, text: &str) {
        let file = poimen_core::settings::config_path(&self.home);
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, text).unwrap();
        std::fs::rename(&tmp, &file).unwrap();
    }
}

struct Running {
    input: Sender<InputEvent>,
    screen: Arc<Mutex<Buffer>>,
    handle: JoinHandle<Result<Outcome, Error>>,
}

fn start(f: &Fixture) -> Running {
    let (input, rx) = mpsc::channel();
    let screen = Arc::new(Mutex::new(Buffer::empty(Default::default())));
    let client = Client::new(f.client.socket());
    let opts = Options {
        config: tui::config::load(Some(&f.home)),
        home: Some(f.home.display().to_string()),
        own_terminal: None,
        mirror: Some(screen.clone()),
        issues: None,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let handle = thread::spawn(move || {
        let mut terminal = Terminal::new(TestBackend::new(200, 30)).unwrap();
        tui::run(&client, &opts, rx, &mut terminal, &stop)
    });
    Running {
        input,
        screen,
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

    fn press(&self, code: KeyCode, mods: KeyModifiers) {
        self.input
            .send(InputEvent::Key(KeyEvent::new(code, mods)))
            .unwrap();
    }

    fn ctrl(&self, c: char) {
        self.press(KeyCode::Char(c), KeyModifiers::CONTROL);
    }

    fn plain(&self, c: char) {
        self.press(KeyCode::Char(c), KeyModifiers::NONE);
    }

    fn outcome(self) -> Outcome {
        self.handle.join().unwrap().unwrap()
    }
}

#[test]
fn a_changed_prefix_and_key_route_without_a_restart() {
    let f = fixture("keys", r#"{"console":{"prefix":"C-a"}}"#);
    let r = start(&f);
    r.wait_for("work");
    f.write_config(r#"{"console":{"prefix":"C-t","keys":{"detach":"q"}}}"#);
    r.wait_for("config reloaded");
    // The old prefix is a plain key now; the new one and its detach key end the TUI.
    r.ctrl('t');
    r.plain('q');
    assert_eq!(r.outcome(), Outcome::Detached);
}

#[test]
fn an_invalid_file_keeps_the_last_config_and_warns() {
    let f = fixture("invalid", r#"{"console":{"prefix":"C-a"}}"#);
    let r = start(&f);
    r.wait_for("work");
    f.write_config("{ not json");
    r.wait_for("not valid JSON");
    r.ctrl('a');
    r.plain('d');
    assert_eq!(r.outcome(), Outcome::Detached);
}

#[test]
fn a_prefix_pressed_before_the_reload_is_not_lost() {
    let f = fixture("pending", r#"{"console":{"prefix":"C-a"}}"#);
    let r = start(&f);
    r.wait_for("work");
    r.ctrl('a');
    // The prefix is pending; the reload changes the detach key under it.
    f.write_config(r#"{"console":{"prefix":"C-a","keys":{"detach":"q"}}}"#);
    r.wait_for("q detach");
    r.plain('q');
    assert_eq!(r.outcome(), Outcome::Detached);
}

#[test]
fn a_scheme_file_change_reloads_the_color_scheme() {
    let f = fixture("scheme", r#"{"colorSchema":"mine"}"#);
    let themes = poimen_core::schemes::themes_dir(&f.home);
    std::fs::create_dir_all(&themes).unwrap();
    let r = start(&f);
    r.wait_for("work");
    std::fs::write(
        themes.join("mine.toml"),
        "[colors]\nforeground = \"#ffffff\"\nbackground = \"#000000\"\nansi = [\"#000000\", \"#111111\", \"#222222\", \"#333333\", \"#444444\", \"#555555\", \"#666666\", \"#777777\"]\nbrights = [\"#000000\", \"#111111\", \"#222222\", \"#333333\", \"#444444\", \"#555555\", \"#666666\", \"#777777\"]\n",
    )
    .unwrap();
    r.wait_for("config reloaded");
    // The default prefix still works after the reload.
    r.ctrl('b');
    r.plain('d');
    assert_eq!(r.outcome(), Outcome::Detached);
}
