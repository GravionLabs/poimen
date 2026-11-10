//! The command palette of the TUI against an in-process poimen-server on a temporary socket and
//! a temporary home (never the user's server or config). Unix-only.
#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossterm::event::{Event as InputEvent, KeyCode, KeyEvent, KeyModifiers};
use poimen_client::Client;
use poimen_console::session::Error;
use poimen_console::tui::config::Config;
use poimen_console::tui::sidebar::Model;
use poimen_console::tui::{self, Options, Outcome};
use poimen_core::github::OpenIssue;
use poimen_server::{Config as ServerConfig, Server};
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

/// A server with a space `work` (no terminals) and a home with `config` as user config.
fn fixture(name: &str, config: &str) -> Fixture {
    // Short: a socket path is limited to about 100 bytes.
    let dir = PathBuf::from("/tmp").join(format!("pl-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("work")).unwrap();
    let dir = dir.canonicalize().unwrap();
    let home = dir.join("home");
    let config_file = poimen_core::settings::config_path(&home);
    std::fs::create_dir_all(config_file.parent().unwrap()).unwrap();
    std::fs::write(&config_file, config).unwrap();
    let server_config = ServerConfig {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        scrollback: 1000,
        home: Some(home.clone()),
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
    fn model(&self) -> Model {
        Model::load(&self.client).unwrap()
    }

    fn space_names(&self) -> Vec<String> {
        self.model().spaces.into_iter().map(|s| s.name).collect()
    }

    fn tab_names(&self) -> Vec<String> {
        self.model().tabs.into_iter().map(|t| t.name).collect()
    }

    fn config_text(&self) -> String {
        std::fs::read_to_string(poimen_core::settings::config_path(&self.home)).unwrap()
    }

    /// A space on `dir` with one terminal, so the TUI focuses it.
    fn space_with_terminal(&self, name: &str, dir: &str) {
        let space = self.client.create_space(name, dir).unwrap();
        self.client
            .create_terminal(&poimen_client::protocol::TerminalCreate {
                space_id: space.id,
                name: Some("shell".into()),
                ..Default::default()
            })
            .unwrap();
    }

    /// A directory under the fixture with a project file.
    fn project(&self, name: &str, file: &str) -> String {
        let p = self.dir.join(name);
        std::fs::create_dir_all(p.join(".poimen")).unwrap();
        std::fs::write(p.join(".poimen/config.json"), file).unwrap();
        p.display().to_string()
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
    handle: JoinHandle<Result<Outcome, Error>>,
}

fn start(f: &Fixture, issues: Option<poimen_console::tui::actions::IssueSource>) -> Running {
    let (input, rx) = mpsc::channel();
    let screen = Arc::new(Mutex::new(Buffer::empty(Default::default())));
    let client = Client::new(f.client.socket());
    let opts = Options {
        config: Config::default(),
        home: Some(f.home.display().to_string()),
        own_terminal: None,
        mirror: Some(screen.clone()),
        issues,
        notify: None,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let handle = thread::spawn(move || {
        let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
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

    fn wait_gone(&self, needle: &str) {
        wait_until(&format!("`{needle}` off screen"), || {
            !self.text().contains(needle)
        });
    }

    fn key(&self, code: KeyCode) {
        self.input
            .send(InputEvent::Key(KeyEvent::new(code, KeyModifiers::NONE)))
            .unwrap();
    }

    fn typed(&self, text: &str) {
        for c in text.chars() {
            self.key(KeyCode::Char(c));
        }
    }

    /// Prefix, then the palette key.
    fn open_palette(&self) {
        self.input
            .send(InputEvent::Key(KeyEvent::new(
                KeyCode::Char('b'),
                KeyModifiers::CONTROL,
            )))
            .unwrap();
        self.key(KeyCode::Char(':'));
        self.wait_for("Command palette");
    }

    /// Opens the palette, filters to `entry` and starts it.
    fn pick(&self, entry: &str) {
        self.open_palette();
        self.typed(entry);
        self.wait_for(entry);
        self.key(KeyCode::Enter);
    }

    fn detach(self) {
        self.input
            .send(InputEvent::Key(KeyEvent::new(
                KeyCode::Char('b'),
                KeyModifiers::CONTROL,
            )))
            .unwrap();
        self.key(KeyCode::Char('d'));
        assert_eq!(self.handle.join().unwrap().unwrap(), Outcome::Detached);
    }
}

#[test]
fn the_palette_opens_with_the_prefix_key_filters_and_closes_with_escape() {
    let f = fixture("open", "{}");
    let r = start(&f, None);
    r.open_palette();
    let text = r.text();
    assert!(
        text.contains("Space: New") && text.contains("Space: Close"),
        "{text}"
    );
    r.typed("rena");
    r.wait_gone("Space: New");
    assert!(r.text().contains("Space: Rename"));
    r.key(KeyCode::Esc);
    r.wait_gone("Command palette");
    r.detach();
}

#[test]
fn a_new_space_is_made_from_a_typed_directory_with_completion() {
    let f = fixture("newspace", "{}");
    let target = f.dir.join("projects").join("alpha");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(f.dir.join("projects").join("beta")).unwrap();
    let r = start(&f, None);
    r.pick("Space: New");
    r.wait_for("New space: directory");
    r.typed(&format!("{}/projects/al", f.dir.display()));
    r.wait_for("projects/alpha");
    r.key(KeyCode::Tab);
    r.wait_for("projects/alpha/");
    r.key(KeyCode::Enter);
    wait_until("the space exists", || {
        f.space_names().contains(&"alpha".to_owned())
    });
    r.wait_gone("Command palette");
    // The new space got its layout and the TUI shows it.
    let space = f
        .model()
        .spaces
        .into_iter()
        .find(|s| s.name == "alpha")
        .unwrap();
    assert_eq!(space.cwd, target.display().to_string());
    assert!(!space.tabs.is_empty());
    r.wait_for("alpha");
    r.detach();
}

#[test]
fn a_missing_directory_is_reported_and_nothing_is_created() {
    let f = fixture("nodir", "{}");
    let r = start(&f, None);
    r.pick("Space: New");
    r.typed(&format!("{}/nope", f.dir.display()));
    r.key(KeyCode::Enter);
    r.wait_gone("Command palette");
    r.wait_for("cannot create the space");
    assert_eq!(f.space_names(), ["work"]);
    r.detach();
}

#[test]
fn rename_and_close_of_a_space_and_a_terminal() {
    let f = fixture("rename", "{}");
    let space = f.model().spaces[0].id.clone();
    let terminal = f
        .client
        .create_terminal(&poimen_client::protocol::TerminalCreate {
            space_id: space,
            name: Some("shell".into()),
            ..Default::default()
        })
        .unwrap()
        .id;
    let r = start(&f, None);
    r.wait_for("shell");

    r.pick("Terminal: Rename");
    r.wait_for("Rename terminal");
    for _ in 0..5 {
        r.key(KeyCode::Backspace);
    }
    r.typed("build");
    r.key(KeyCode::Enter);
    wait_until("terminal renamed", || {
        f.model()
            .terminal(&terminal)
            .is_some_and(|t| t.name == "build")
    });

    r.pick("Space: Rename");
    r.wait_for("Rename space");
    r.typed("-2");
    r.key(KeyCode::Enter);
    wait_until("space renamed", || f.space_names() == ["work-2"]);

    // Closing asks first: any other key leaves everything as it is.
    r.pick("Space: Close");
    r.wait_for("Close space \"work-2\"");
    r.key(KeyCode::Char('x'));
    assert_eq!(f.space_names(), ["work-2"]);
    r.key(KeyCode::Char('n'));
    r.wait_gone("Command palette");
    assert_eq!(f.space_names(), ["work-2"]);

    r.pick("Terminal: Close");
    r.wait_for("Close terminal \"build\"");
    r.key(KeyCode::Char('y'));
    wait_until("terminal closed", || {
        f.model().terminal(&terminal).is_none()
    });
    assert_eq!(f.space_names(), ["work-2"]);

    r.pick("Space: Close");
    r.wait_for("Close space");
    r.key(KeyCode::Char('y'));
    wait_until("space closed", || f.space_names().is_empty());
    r.detach();
}

const SETUP_PROJECT: &str = r#"{"setup": "echo setup-ran",
    "layout": {"panes": [{"name": "dev", "command": "echo dev"}]}}"#;

#[test]
fn an_untrusted_project_asks_with_hash_and_commands_then_sends_the_request_again() {
    let f = fixture("trust", "{}");
    let dir = f.project("proj", SETUP_PROJECT);
    let r = start(&f, None);
    r.pick("Space: New");
    r.typed(&dir);
    r.key(KeyCode::Enter);
    r.wait_for("It runs these commands");
    let screen = r.text();
    let hash = poimen_core::trust::sha256_hex(SETUP_PROJECT);
    assert!(screen.contains(&format!("hash {hash}")), "{screen}");
    assert!(screen.contains("setup: echo setup-ran"), "{screen}");
    assert!(screen.contains("echo dev"), "{screen}");
    // Nothing exists while the question is open, and Enter does not answer it.
    r.key(KeyCode::Enter);
    assert_eq!(f.space_names(), ["work"]);
    r.key(KeyCode::Char('t'));
    wait_until("the space exists", || {
        f.space_names().contains(&"proj".to_owned())
    });
    r.wait_gone("Command palette");
    assert!(
        f.tab_names().contains(&"setup".to_owned()),
        "{:?}",
        f.tab_names()
    );
    // The server stored the trust for exactly this content.
    assert!(f.config_text().contains(&hash), "{}", f.config_text());
    r.detach();
}

#[test]
fn declining_the_commands_builds_the_panes_without_setup_and_stores_nothing() {
    let f = fixture("without", "{}");
    let dir = f.project("proj", SETUP_PROJECT);
    let r = start(&f, None);
    r.pick("Space: New");
    r.typed(&dir);
    r.key(KeyCode::Enter);
    r.wait_for("It runs these commands");
    r.key(KeyCode::Char('w'));
    wait_until("the space exists", || {
        f.space_names().contains(&"proj".to_owned())
    });
    assert!(!f.tab_names().contains(&"setup".to_owned()));
    assert!(!f.config_text().contains("trustedProjects"));
    r.detach();
}

#[test]
fn escape_at_the_trust_question_creates_nothing() {
    let f = fixture("cancel", "{}");
    let dir = f.project("proj", SETUP_PROJECT);
    let r = start(&f, None);
    r.pick("Space: New");
    r.typed(&dir);
    r.key(KeyCode::Enter);
    r.wait_for("It runs these commands");
    r.key(KeyCode::Esc);
    r.wait_gone("Command palette");
    assert_eq!(f.space_names(), ["work"]);
    assert!(!f.config_text().contains("trustedProjects"));
    r.detach();
}

const ENTRIES_PROJECT: &str = r#"{"commands": [
        {"name": "check", "command": "echo checking", "target": "terminal"}],
    "presets": [{"name": "duo", "panes": [{"name": "left"},
        {"name": "right", "split": {"direction": "right", "from": "left"}}]}]}"#;

#[test]
fn project_entries_are_listed_and_run_behind_the_trust_gate() {
    let f = fixture("entries", "{}");
    let dir = f.project("proj", ENTRIES_PROJECT);
    // The focused space is the project's.
    f.space_with_terminal("proj", &dir);
    let r = start(&f, None);
    r.wait_for("proj");
    r.open_palette();
    r.typed("check");
    r.wait_for("Project: check");
    r.key(KeyCode::Enter);
    // Not trusted: asks, runs nothing.
    r.wait_for("It runs these commands");
    assert!(!f.tab_names().contains(&"check".to_owned()));
    r.key(KeyCode::Char('t'));
    wait_until("the tab exists", || {
        f.tab_names().contains(&"check".to_owned())
    });
    r.wait_gone("Command palette");
    assert!(
        f.config_text()
            .contains(&poimen_core::trust::sha256_hex(ENTRIES_PROJECT))
    );

    // Trusted now: the preset runs without asking and splits its tab.
    r.pick("Preset: duo");
    // The tab appears before its split does: wait for both terminals.
    wait_until("the preset tab is split", || {
        f.model()
            .tabs
            .iter()
            .any(|t| t.name == "duo" && t.root.terminal_ids().len() == 2)
    });
    r.detach();
}

#[test]
fn declining_a_project_entry_runs_nothing() {
    let f = fixture("entry-decline", "{}");
    let dir = f.project("proj", ENTRIES_PROJECT);
    f.space_with_terminal("proj", &dir);
    let r = start(&f, None);
    r.wait_for("proj");
    r.pick("Project: check");
    r.wait_for("It runs these commands");
    r.key(KeyCode::Char('w'));
    r.wait_gone("Command palette");
    r.wait_for("project commands are declined");
    assert!(!f.tab_names().contains(&"check".to_owned()));
    r.detach();
}

fn git(dir: &Path, args: &[&str]) {
    let out = poimen_core::gui_path::command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com")
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// `<f>/code/app`, a clone of a bare origin, with one commit on `main`.
fn repo(f: &Fixture) -> String {
    let code = f.dir.join("code");
    std::fs::create_dir_all(&code).unwrap();
    git(&code, &["init", "--bare", "-b", "main", "origin.git"]);
    git(&code, &["clone", "origin.git", "app"]);
    let app = code.join("app");
    std::fs::write(app.join("README.md"), "x").unwrap();
    git(&app, &["checkout", "-b", "main"]);
    git(&app, &["add", "."]);
    git(&app, &["commit", "-m", "init"]);
    git(&app, &["push", "origin", "main"]);
    git(&app, &["remote", "set-head", "origin", "main"]);
    app.display().to_string()
}

fn issues(list: Vec<OpenIssue>) -> poimen_console::tui::actions::IssueSource {
    Arc::new(move |_| Ok(list.clone()))
}

fn issue(nr: u64, title: &str) -> OpenIssue {
    OpenIssue {
        number: nr,
        title: title.into(),
        level: Some("pbi".into()),
        labels: vec!["pbi".into()],
        assignees: vec![],
        url: format!("https://github.com/o/r/issues/{nr}"),
    }
}

const TASK_CONFIG: &str = r#"{"tasks": {"modes": [
    {"name": "echo", "worktree": true, "command": ["echo", "task {nr}"]}]}}"#;

#[test]
fn start_task_picks_an_issue_makes_the_worktree_and_a_space_on_it() {
    let f = fixture("task", TASK_CONFIG);
    let app = repo(&f);
    f.space_with_terminal("app", &app);
    let r = start(
        &f,
        Some(issues(vec![
            issue(12, "Add picker"),
            issue(40, "Fix the bug"),
        ])),
    );
    r.wait_for("app");
    r.pick("Task: Start");
    r.wait_for("Start task: issue (2 open)");
    r.typed("#12");
    r.wait_gone("Fix the bug");
    r.key(KeyCode::Enter);
    // The default `agent` mode and the configured `echo` one: pick the second.
    r.wait_for("Start task #12: mode");
    r.key(KeyCode::Down);
    r.key(KeyCode::Enter);
    let worktree = f.dir.join("code").join("app-12");
    wait_until("the task space exists", || {
        f.space_names().contains(&"#12 Add picker".to_owned())
    });
    let space = f
        .model()
        .spaces
        .into_iter()
        .find(|s| s.name == "#12 Add picker")
        .unwrap();
    assert_eq!(space.cwd, worktree.display().to_string());
    assert!(worktree.join("README.md").exists());
    r.wait_gone("Command palette");
    r.detach();
}

#[test]
fn a_failing_issue_list_is_reported_in_the_status_line() {
    let f = fixture("task-fail", TASK_CONFIG);
    let source: poimen_console::tui::actions::IssueSource =
        Arc::new(|_| Err("gh is not logged in (run `gh auth login`)".into()));
    let r = start(&f, Some(source));
    r.pick("Task: Start");
    r.wait_for("cannot read the issues");
    r.wait_gone("Command palette");
    assert_eq!(f.space_names(), ["work"]);
    r.detach();
}

#[test]
fn a_task_in_a_trust_needing_repository_asks_then_starts_with_the_same_command() {
    let f = fixture("task-trust", TASK_CONFIG);
    let app = repo(&f);
    std::fs::create_dir_all(Path::new(&app).join(".poimen")).unwrap();
    std::fs::write(
        Path::new(&app).join(".poimen/config.json"),
        r#"{"setup": "echo hi"}"#,
    )
    .unwrap();
    git(Path::new(&app), &["add", "."]);
    git(Path::new(&app), &["commit", "-m", "project file"]);
    git(Path::new(&app), &["push", "origin", "main"]);
    f.space_with_terminal("app", &app);
    let r = start(&f, Some(issues(vec![issue(12, "Add picker")])));
    r.wait_for("app");
    r.pick("Task: Start");
    r.wait_for("Start task: issue");
    r.key(KeyCode::Enter);
    r.wait_for("Start task #12: mode");
    r.key(KeyCode::Down);
    r.key(KeyCode::Enter);
    // The worktree has the project file too, so the space asks before it is created.
    r.wait_for("It runs these commands");
    assert!(!f.space_names().contains(&"#12 Add picker".to_owned()));
    r.key(KeyCode::Char('w'));
    wait_until("the task space exists", || {
        f.space_names().contains(&"#12 Add picker".to_owned())
    });
    r.detach();
}
