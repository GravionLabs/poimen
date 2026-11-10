//! `space.apply_layout` (#645) end to end: a real server on a temp socket, a temp repository with
//! a project file and a temp home with a fake `~/.config/poimen/config.json`. Never the user's
//! server, home or config. The panes run the default shell, and the commands the layout types
//! write marker files in the temp repository.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::protocol::{
    ErrorCode, Event, Node, SpaceApplyLayout, SpaceApplyLayoutResult, SpaceCreated, SplitDirection,
    TabCreate, TrustAnswer, TrustRequired,
};
use poimen_client::{Client, ClientError};
use poimen_core::settings;
use poimen_server::{Config, Server};

/// Writes a marker file in the shell's directory; the same line works in sh, zsh, bash,
/// PowerShell and cmd.
fn marker(name: &str) -> String {
    format!("echo ok > {name}")
}

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-apply-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["repo/.poimen", "other/.poimen"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let env = Self { dir };
        std::fs::create_dir_all(settings::config_dir(&env.home())).unwrap();
        env
    }

    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    fn repo(&self, name: &str) -> String {
        self.dir.join(name).display().to_string()
    }

    fn project(&self, repo: &str, text: &str) {
        std::fs::write(self.dir.join(repo).join(".poimen/config.json"), text).unwrap();
    }

    fn write_config(&self, text: &str) {
        std::fs::write(settings::config_path(&self.home()), text).unwrap();
    }

    fn config_text(&self) -> String {
        std::fs::read_to_string(settings::config_path(&self.home())).unwrap_or_default()
    }

    fn start(&self) -> Client {
        let config = Config {
            socket: self.dir.join("run").join("server.sock"),
            state_file: self.dir.join("state").join("server.json"),
            scrollback: 1000,
            home: Some(self.home()),
        };
        let server = Server::bind(config.clone()).unwrap();
        thread::spawn(move || server.serve());
        Client::new(&config.socket)
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn events(client: &Client) -> Receiver<Event> {
    let stream = client.subscribe().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for event in stream.flatten() {
            if tx.send(event).is_err() {
                return;
            }
        }
    });
    rx
}

/// The names of the events that arrive within a short while.
fn drain(rx: &Receiver<Event>) -> Vec<String> {
    let mut names = Vec::new();
    while let Ok(event) = rx.recv_timeout(Duration::from_millis(300)) {
        let value = serde_json::to_value(&event).unwrap();
        names.push(value["event"].as_str().unwrap().to_string());
    }
    names
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "{} did not appear",
            path.display()
        );
        thread::sleep(Duration::from_millis(50));
    }
}

fn apply(client: &Client, params: SpaceApplyLayout) -> SpaceApplyLayoutResult {
    client.apply_layout(&params).unwrap()
}

fn with_trust(params: SpaceApplyLayout, trust: TrustAnswer) -> SpaceApplyLayout {
    SpaceApplyLayout {
        trust: Some(trust),
        ..params
    }
}

fn asked(result: SpaceApplyLayoutResult) -> TrustRequired {
    match result {
        SpaceApplyLayoutResult::TrustRequired { trust_required } => trust_required,
        other => panic!("expected trust_required, got {other:?}"),
    }
}

fn applied(result: SpaceApplyLayoutResult) -> SpaceCreated {
    match result {
        SpaceApplyLayoutResult::Applied(a) => a,
        other => panic!("expected the applied layout, got {other:?}"),
    }
}

/// A project file whose layout, setup and preset write marker files.
fn project_file() -> String {
    format!(
        r#"{{
  "layout": {{"panes": [
    {{"name": "dev", "command": "{dev}"}},
    {{"name": "logs", "split": {{"from": "dev", "direction": "right", "ratio": 0.6}}}}
  ]}},
  "setup": "{setup}",
  "presets": [{{"name": "duo", "panes": [
    {{"name": "left", "command": "{left}"}},
    {{"name": "right", "split": {{"direction": "down", "ratio": 0.3}}}}
  ]}}]
}}"#,
        dev = marker("dev-ran"),
        setup = marker("setup-ran"),
        left = marker("left-ran"),
    )
}

fn server_error(err: ClientError) -> ErrorCode {
    match err {
        ClientError::Server { code, .. } => code,
        other => panic!("expected a server error, got {other:?}"),
    }
}

#[test]
fn rebuilding_an_empty_space_asks_first_then_builds_layout_and_setup_into_it() {
    let env = Env::new("rebuild");
    env.project("repo", &project_file());
    let client = env.start();
    let repo = env.repo("repo");
    let space = client.create_space("work", &repo).unwrap();
    let rx = events(&client);

    // Not trusted yet: the question, and nothing is created.
    let question = asked(apply(&client, SpaceApplyLayout::resolved(&space.id)));
    assert_eq!(question.root, repo);
    let panes: Vec<_> = question
        .layout
        .as_ref()
        .unwrap()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(panes, ["dev", "logs"]);
    assert!(client.list_terminals(None).unwrap().is_empty());
    assert_eq!(drain(&rx), Vec::<String>::new());
    assert_eq!(env.config_text(), "");

    let done = applied(apply(
        &client,
        with_trust(
            SpaceApplyLayout::resolved(&space.id),
            TrustAnswer::Trust {
                hash: question.hash.clone(),
            },
        ),
    ));
    assert_eq!(done.space.id, space.id);
    assert!(done.setup_started);
    assert!(done.warnings.is_empty(), "{:?}", done.warnings);
    let names: Vec<_> = done.tabs.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["dev", "setup"]);
    let Node::Split {
        direction,
        ratio,
        children,
        ..
    } = &done.tabs[0].root
    else {
        panic!("{:?}", done.tabs[0].root)
    };
    assert_eq!((*direction, *ratio), (SplitDirection::Right, 0.6));
    assert_eq!(children.len(), 2);
    assert_eq!(client.list_layout().unwrap().tabs, done.tabs);

    let order = drain(&rx);
    assert!(order.contains(&"tab.created".to_string()), "{order:?}");
    assert!(order.contains(&"tab.changed".to_string()), "{order:?}");
    assert!(!order.contains(&"space.created".to_string()), "{order:?}");
    wait_for_file(&env.dir.join("repo/dev-ran"));
    wait_for_file(&env.dir.join("repo/setup-ran"));
    let stored = env.config_text();
    assert!(stored.contains(&question.hash), "{stored}");
    client.close_space(&space.id).unwrap();
}

#[test]
fn a_preset_is_one_new_tab_after_the_existing_ones_without_setup() {
    let env = Env::new("preset");
    env.project("repo", &project_file());
    let client = env.start();
    let repo = env.repo("repo");
    let space = client.create_space("work", &repo).unwrap();
    let (first, _) = client
        .create_tab(&TabCreate {
            space_id: space.id.clone(),
            name: Some("shell".into()),
            terminal_id: None,
        })
        .unwrap();
    let rx = events(&client);

    let question = asked(apply(&client, SpaceApplyLayout::preset(&space.id, "duo")));
    assert_eq!(question.layout.as_ref().unwrap()[0].name, "left");
    let done = applied(apply(
        &client,
        with_trust(
            SpaceApplyLayout::preset(&space.id, "duo"),
            TrustAnswer::Trust {
                hash: question.hash,
            },
        ),
    ));
    assert!(!done.setup_started);
    assert_eq!(done.tabs.len(), 1);
    assert_eq!(done.tabs[0].name, "left");
    assert_eq!(done.tabs[0].root.terminal_ids().len(), 2);
    assert_eq!(
        done.space.tabs,
        [first.tab_id.clone(), done.tabs[0].tab_id.clone()]
    );
    // The existing tab stays the active one.
    assert_eq!(
        done.space.active_tab.as_deref(),
        Some(first.tab_id.as_str())
    );
    let order = drain(&rx);
    assert!(order.contains(&"tab.changed".to_string()), "{order:?}");
    wait_for_file(&env.dir.join("repo/left-ran"));
    assert!(!env.dir.join("repo/setup-ran").exists());
    client.close_space(&space.id).unwrap();
}

#[test]
fn without_builds_the_preset_panes_but_types_and_stores_nothing() {
    let env = Env::new("without");
    env.project("repo", &project_file());
    let client = env.start();
    let space = client.create_space("work", &env.repo("repo")).unwrap();
    let done = applied(apply(
        &client,
        with_trust(
            SpaceApplyLayout::preset(&space.id, "duo"),
            TrustAnswer::Without,
        ),
    ));
    assert_eq!(done.tabs[0].root.terminal_ids().len(), 2);
    thread::sleep(Duration::from_millis(1500));
    assert!(!env.dir.join("repo/left-ran").exists());
    assert_eq!(env.config_text(), "");
    client.close_space(&space.id).unwrap();
}

#[test]
fn without_a_project_file_the_user_layout_is_rebuilt_without_asking() {
    let env = Env::new("user");
    env.write_config(&format!(
        r#"{{"spaces": {{"layout": {{"panes": [{{"name": "solo", "command": "{}"}}]}}}}}}"#,
        marker("solo-ran")
    ));
    let client = env.start();
    let space = client.create_space("work", &env.repo("other")).unwrap();
    let done = applied(apply(&client, SpaceApplyLayout::resolved(&space.id)));
    assert_eq!(done.tabs.len(), 1);
    assert_eq!(done.tabs[0].name, "solo");
    wait_for_file(&env.dir.join("other/solo-ran"));
    client.close_space(&space.id).unwrap();
}

#[test]
fn an_unknown_space_or_preset_is_not_found_and_creates_nothing() {
    let env = Env::new("missing");
    env.project("repo", &project_file());
    let client = env.start();
    let err = client
        .apply_layout(&SpaceApplyLayout::resolved("s404"))
        .unwrap_err();
    assert_eq!(server_error(err), ErrorCode::NotFound);
    let space = client.create_space("work", &env.repo("repo")).unwrap();
    let err = client
        .apply_layout(&SpaceApplyLayout::preset(&space.id, "nope"))
        .unwrap_err();
    assert_eq!(server_error(err), ErrorCode::NotFound);
    assert!(client.list_terminals(None).unwrap().is_empty());
    assert!(client.list_layout().unwrap().tabs.is_empty());
}

#[test]
fn a_missing_directory_is_invalid_params() {
    let env = Env::new("gone");
    let client = env.start();
    let gone = env.dir.join("gone").display().to_string();
    let space = client.create_space("work", &gone).unwrap();
    let err = client
        .apply_layout(&SpaceApplyLayout::resolved(&space.id))
        .unwrap_err();
    assert_eq!(server_error(err), ErrorCode::InvalidParams);
}
