//! `space.create` with the layout (#511) end to end: a real server on a temp socket, a temp
//! repository with a project file and a temp home with a fake `~/.config/poimen/config.json`.
//! Never the user's server, home or config. The panes run the default shell, and the commands
//! the layout types write marker files in the temp repository.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{
    Event, Node, SpaceCreate, SpaceCreateResult, SpaceCreated, SplitDirection, TrustAnswer,
    TrustRequired,
};
use poimen_core::settings;
use poimen_server::{Config, PtyHost, Server};
use serde_json::Value;

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
        let dir = std::env::temp_dir().join(format!("poimen-create-{}-{name}", std::process::id()));
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
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
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

fn create(client: &Client, cwd: &str, trust: Option<TrustAnswer>) -> SpaceCreateResult {
    client
        .create_space_with(&SpaceCreate {
            trust,
            ..SpaceCreate::with_layout("work", cwd)
        })
        .unwrap()
}

fn asked(result: SpaceCreateResult) -> TrustRequired {
    match result {
        SpaceCreateResult::TrustRequired { trust_required } => trust_required,
        other => panic!("expected trust_required, got {other:?}"),
    }
}

fn created(result: SpaceCreateResult) -> SpaceCreated {
    match result {
        SpaceCreateResult::Created(c) => c,
        other => panic!("expected a space, got {other:?}"),
    }
}

/// A project file whose layout and setup write marker files.
fn project_file() -> String {
    format!(
        r#"{{
  "layout": {{"panes": [
    {{"name": "dev", "command": "{dev}"}},
    {{"name": "logs", "split": {{"from": "dev", "direction": "right", "ratio": 0.6}}}},
    {{"name": "sh", "split": {{"from": "logs", "direction": "down"}}}}
  ]}},
  "setup": "{setup}"
}}"#,
        dev = marker("dev-ran"),
        setup = marker("setup-ran"),
    )
}

#[test]
fn an_untrusted_project_asks_first_and_the_trusted_answer_builds_tree_and_setup() {
    let env = Env::new("trust");
    env.project("repo", &project_file());
    env.write_config("{\n  \"theme\": \"dark\"\n}\n");
    let client = env.start();
    let rx = events(&client);
    let repo = env.repo("repo");

    // Not trusted yet: the question, and nothing exists.
    let question = asked(create(&client, &repo, None));
    assert_eq!(question.root, repo);
    let commands: Vec<_> = question.commands.iter().map(|c| c.what.as_str()).collect();
    assert_eq!(commands, ["layout: pane \"dev\"", "setup"]);
    let panes: Vec<_> = question
        .layout
        .as_ref()
        .unwrap()
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert_eq!(panes, ["dev", "logs", "sh"]);
    assert!(client.list_spaces().unwrap().is_empty());
    assert!(client.list_terminals(None).unwrap().is_empty());
    assert_eq!(drain(&rx), Vec::<String>::new());
    assert_eq!(env.config_text(), "{\n  \"theme\": \"dark\"\n}\n");

    // The user trusted it: the server stores the hash and builds the layout.
    let space = created(create(
        &client,
        &repo,
        Some(TrustAnswer::Trust {
            hash: question.hash.clone(),
        }),
    ));
    assert!(space.setup_started);
    assert!(space.warnings.is_empty(), "{:?}", space.warnings);
    let names: Vec<_> = space.tabs.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["dev", "setup"]);
    assert_eq!(
        space.space.active_tab.as_deref(),
        Some(space.tabs[0].tab_id.as_str())
    );

    let terminals = client.list_terminals(Some(&space.space.id)).unwrap();
    let id_of = |name: &str| {
        terminals
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("no terminal {name}"))
            .id
            .clone()
    };
    let Node::Split {
        direction,
        ratio,
        children,
        ..
    } = &space.tabs[0].root
    else {
        panic!("{:?}", space.tabs[0].root)
    };
    assert_eq!((*direction, *ratio), (SplitDirection::Right, 0.6));
    assert_eq!(children[0], Node::terminal(id_of("dev")));
    let Node::Split {
        direction,
        ratio,
        children,
        ..
    } = &children[1]
    else {
        panic!("{:?}", children[1])
    };
    assert_eq!((*direction, *ratio), (SplitDirection::Down, 0.5));
    assert_eq!(
        children.as_ref(),
        &[Node::terminal(id_of("logs")), Node::terminal(id_of("sh"))]
    );
    assert_eq!(space.tabs[1].root, Node::terminal(id_of("setup")));
    assert_eq!(space.tabs[0].active_terminal, id_of("dev"));

    // The layout matches what `space.list` reports.
    let listed = client.list_layout().unwrap();
    assert_eq!(listed.spaces, std::slice::from_ref(&space.space));
    assert_eq!(listed.tabs, space.tabs);
    let order = drain(&rx);
    assert_eq!(order.first().map(String::as_str), Some("space.created"));
    let first_tab = order.iter().position(|e| e == "tab.created").unwrap();
    let first_terminal = order.iter().position(|e| e == "terminal.created").unwrap();
    assert!(first_terminal < first_tab, "{order:?}");

    // The commands ran in their panes.
    wait_for_file(&env.dir.join("repo/dev-ran"));
    wait_for_file(&env.dir.join("repo/setup-ran"));

    // The hash is stored, the rest of the config is kept, and the next space does not ask.
    let text = env.config_text();
    assert!(text.contains("\"theme\": \"dark\""), "{text}");
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["trustedProjects"][&repo], question.hash.as_str());
    let again = created(create(&client, &repo, None));
    assert!(again.setup_started);
    for s in client.list_spaces().unwrap() {
        client.close_space(&s.id).unwrap();
    }
}

#[test]
fn without_builds_the_panes_but_types_and_stores_nothing() {
    let env = Env::new("without");
    env.project("repo", &project_file());
    let client = env.start();
    let repo = env.repo("repo");

    let space = created(create(&client, &repo, Some(TrustAnswer::Without)));
    assert!(!space.setup_started);
    let names: Vec<_> = space.tabs.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(names, ["dev"]);
    assert_eq!(space.tabs[0].root.terminal_ids().len(), 3);
    thread::sleep(Duration::from_millis(1500));
    assert!(!env.dir.join("repo/dev-ran").exists());
    assert!(!env.dir.join("repo/setup-ran").exists());
    assert_eq!(env.config_text(), "");
    // Still not trusted: a new space asks again.
    asked(create(&client, &repo, None));
    client.close_space(&space.space.id).unwrap();
}

#[test]
fn a_stale_hash_asks_again_and_creates_nothing() {
    let env = Env::new("stale");
    env.project("repo", &project_file());
    let client = env.start();
    let repo = env.repo("repo");
    let first = asked(create(&client, &repo, None));
    // The file changes between the question and the answer.
    env.project("repo", &project_file().replace("setup-ran", "other-ran"));
    let second = asked(create(
        &client,
        &repo,
        Some(TrustAnswer::Trust {
            hash: first.hash.clone(),
        }),
    ));
    assert_ne!(second.hash, first.hash);
    assert!(client.list_spaces().unwrap().is_empty());
    assert!(client.list_terminals(None).unwrap().is_empty());
    assert_eq!(env.config_text(), "");
}

#[test]
fn without_a_project_file_the_user_layout_is_built_without_asking() {
    let env = Env::new("user");
    env.write_config(&format!(
        r#"{{"spaces": {{"layout": {{"panes": [{{"name": "solo", "command": "{}"}}]}}}}}}"#,
        marker("solo-ran")
    ));
    let client = env.start();
    let dir = env.repo("other");
    let space = created(create(&client, &dir, None));
    assert_eq!(space.tabs.len(), 1);
    assert_eq!(space.tabs[0].name, "solo");
    assert!(!space.setup_started);
    wait_for_file(&env.dir.join("other/solo-ran"));
    client.close_space(&space.space.id).unwrap();
}

/// Compact form of a split tree: `name` of a leaf (by terminal name), `right[a, b]`, `down[a, b]`.
fn shape(node: &Node, names: &std::collections::HashMap<String, String>) -> String {
    match node {
        Node::Terminal { terminal_id } => names[terminal_id].clone(),
        Node::Split {
            direction,
            children,
            ..
        } => {
            let d = match direction {
                SplitDirection::Right => "right",
                SplitDirection::Down => "down",
            };
            format!(
                "{d}[{}, {}]",
                shape(&children[0], names),
                shape(&children[1], names)
            )
        }
    }
}

#[test]
fn the_user_layout_keeps_each_split_direction_and_from() {
    // #664: copilot right of claude, shell below claude.
    let env = Env::new("directions");
    env.write_config(
        r#"{"spaces": {"layout": {"panes": [
            {"name": "claude"},
            {"name": "copilot", "split": {"from": "claude", "direction": "right", "ratio": 0.5}},
            {"name": "shell", "split": {"from": "claude", "direction": "down", "ratio": 0.7}}]}}}"#,
    );
    let client = env.start();
    let space = created(create(&client, &env.repo("other"), None));
    let names: std::collections::HashMap<String, String> = client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .map(|t| (t.id, t.name))
        .collect();
    assert_eq!(space.tabs.len(), 1);
    assert_eq!(
        shape(&space.tabs[0].root, &names),
        "right[down[claude, shell], copilot]"
    );
    let Node::Split {
        ratio, children, ..
    } = &space.tabs[0].root
    else {
        panic!("root is a split")
    };
    assert_eq!(*ratio, 0.5);
    let Node::Split { ratio, .. } = &children[0] else {
        panic!("first child is a split")
    };
    assert_eq!(*ratio, 0.7);
    client.close_space(&space.space.id).unwrap();
}

#[test]
fn layout_false_creates_an_empty_space() {
    let env = Env::new("empty");
    env.project("repo", &project_file());
    let client = env.start();
    let space = client.create_space("plain", &env.repo("repo")).unwrap();
    assert!(space.tabs.is_empty());
    assert!(client.list_terminals(None).unwrap().is_empty());
}

#[test]
fn server_and_app_writes_of_config_json_in_turn_keep_both_changes() {
    let env = Env::new("writers");
    env.project("repo", &project_file());
    env.project("other", &project_file().replace("dev-ran", "other-dev-ran"));
    env.write_config("{\n  // my settings\n  \"theme\": \"light\"\n}\n");
    let client = env.start();

    let trust = |repo: &str| {
        let question = asked(create(&client, repo, None));
        let space = created(create(
            &client,
            repo,
            Some(TrustAnswer::Trust {
                hash: question.hash.clone(),
            }),
        ));
        client.close_space(&space.space.id).unwrap();
        question.hash
    };
    // Server, app (its own key, through the same writer), server, app.
    let first = trust(&env.repo("repo"));
    settings::save_with(&env.home(), |t| {
        settings::set_key_text(t, "theme", "\"dark\"")
    })
    .unwrap();
    let second = trust(&env.repo("other"));
    settings::save_with(&env.home(), |t| {
        settings::set_path_text(t, &["sidebar", "layout"], "\"split\"")
    })
    .unwrap();

    let text = env.config_text();
    assert!(text.contains("// my settings"), "{text}");
    let parsed: Value = serde_json::from_str(&text.replace("  // my settings\n", "")).unwrap();
    assert_eq!(parsed["theme"], "dark");
    assert_eq!(parsed["sidebar"]["layout"], "split");
    assert_eq!(parsed["trustedProjects"][env.repo("repo")], first.as_str());
    assert_eq!(
        parsed["trustedProjects"][env.repo("other")],
        second.as_str()
    );
}
