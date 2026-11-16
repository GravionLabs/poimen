//! Tabs with a parent (#655, protocol 7) end to end: `space.create` builds every tab of a named
//! layout depth first with the parent link, closing a parent makes its children top level,
//! `tab.move` keeps the link and it all survives a restart. A real server on a temp socket, a temp
//! home with a fake `config.json`; never the user's server or config.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use poimen_client::Client;
use poimen_client::protocol::{Event, SpaceCreate, SpaceCreateResult, SpaceCreated};
use poimen_core::settings;
use poimen_server::{Config, PtyHost, Server};

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("poimen-parents-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let env = Self { dir };
        std::fs::create_dir_all(settings::config_dir(&env.home())).unwrap();
        env
    }

    fn home(&self) -> PathBuf {
        self.dir.join("home")
    }

    fn work(&self) -> String {
        self.dir.join("work").display().to_string()
    }

    fn config(&self, run: &str) -> Config {
        Config {
            socket: self.dir.join(run).join("server.sock"),
            state_file: self.dir.join("state").join("server.json"),
            scrollback: 1000,
            home: Some(self.home()),
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        }
    }

    /// A layout with the #608 example: claude left, Terminal right top, Logs right bottom, and a
    /// tab whose `of` names nothing.
    fn write_placed(&self) {
        let dir = self.work();
        std::fs::write(
            settings::config_path(&self.home()),
            format!(
                r#"{{"layouts": [{{"name": "placed", "tabs": [
                  {{"name": "claude", "children": [
                    {{"name": "Terminal", "place": {{"direction": "right", "ratio": 0.6}}}},
                    {{"name": "Logs", "place": {{"direction": "down", "of": "Terminal"}}}},
                    {{"name": "Lost", "place": {{"direction": "down", "of": "nowhere"}}}}]}}]}}],
                 "spaceTemplates": [{{"name": "t", "agent": "claude", "layout": "placed",
                   "match": {{"dir": {dir:?}}}}}]}}"#
            ),
        )
        .unwrap();
    }

    /// A template for `work` that names a layout with a parent tab, two children (the second has a
    /// child of its own) and a second top-level tab, one of them split.
    fn write_layout(&self) {
        let dir = self.work();
        std::fs::write(
            settings::config_path(&self.home()),
            format!(
                r#"{{"layouts": [{{"name": "nest", "tabs": [
                  {{"name": "claude", "children": [
                    {{"name": "Terminal"}},
                    {{"name": "Logs", "root": {{"kind": "split", "direction": "right", "children": [
                      {{"kind": "terminal", "name": "Logs"}}, {{"kind": "terminal", "name": "b"}}]}},
                     "children": [{{"name": "Deep"}}]}}]}},
                  {{"name": "other"}}]}}],
                 "spaceTemplates": [{{"name": "t", "agent": "claude", "layout": "nest",
                   "match": {{"dir": {dir:?}}}}}]}}"#
            ),
        )
        .unwrap();
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(config: &Config) -> Client {
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    Client::new(&config.socket)
}

fn create(client: &Client, cwd: &str) -> SpaceCreated {
    match client
        .create_space_with(&SpaceCreate::with_layout("work", cwd))
        .unwrap()
    {
        SpaceCreateResult::Created(c) => c,
        other => panic!("expected a space, got {other:?}"),
    }
}

/// `(name, parent name)` of the tabs in space order.
fn shape(client: &Client) -> Vec<(String, Option<String>)> {
    let layout = client.list_layout().unwrap();
    let space = &layout.spaces[0];
    space
        .tabs
        .iter()
        .map(|id| {
            let tab = layout.tabs.iter().find(|t| &t.tab_id == id).unwrap();
            let parent = tab.parent.as_ref().map(|p| {
                layout
                    .tabs
                    .iter()
                    .find(|t| &t.tab_id == p)
                    .unwrap()
                    .name
                    .clone()
            });
            (tab.name.clone(), parent)
        })
        .collect()
}

fn expected() -> Vec<(String, Option<String>)> {
    [
        ("claude", None),
        ("Terminal", Some("claude")),
        ("Logs", Some("claude")),
        ("Deep", Some("Logs")),
        ("other", None),
    ]
    .into_iter()
    .map(|(n, p)| (n.to_string(), p.map(str::to_string)))
    .collect()
}

#[test]
fn a_layout_builds_every_tab_depth_first_with_the_parent_link() {
    let env = Env::new("create");
    env.write_layout();
    let client = start(&env.config("run"));
    let space = create(&client, &env.work());
    assert_eq!(shape(&client), expected());
    // The first tab is the active one; the answer carries the same links.
    assert_eq!(space.space.active_tab.as_ref(), Some(&space.space.tabs[0]));
    assert_eq!(space.tabs.len(), 5);
    assert_eq!(space.tabs[0].parent, None);
    assert_eq!(
        space.tabs[1].parent.as_deref(),
        Some(space.tabs[0].tab_id.as_str())
    );
    // A tab with a split tree builds it.
    assert_eq!(
        client.list_terminals(Some(&space.space.id)).unwrap().len(),
        6
    );
}

#[test]
fn closing_a_parent_keeps_its_children_as_top_level_tabs_and_move_keeps_the_link() {
    let env = Env::new("close");
    env.write_layout();
    let client = start(&env.config("run"));
    let space = create(&client, &env.work());
    // Moving a tab keeps its parent.
    let logs = space.tabs[2].tab_id.clone();
    client.move_tab(&logs, 0).unwrap();
    let moved = client.list_layout().unwrap();
    assert_eq!(
        moved
            .tabs
            .iter()
            .find(|t| t.tab_id == logs)
            .unwrap()
            .parent
            .as_deref(),
        Some(space.tabs[0].tab_id.as_str())
    );

    let stream = client.subscribe().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for event in stream.flatten() {
            if tx.send(event).is_err() {
                return;
            }
        }
    });

    // Closing `claude` makes `Terminal` and `Logs` top level (`tab.changed` each); `Deep` keeps
    // `Logs`.
    client.close_tab(&space.tabs[0].tab_id).unwrap();
    let mut changed = Vec::new();
    while let Ok(event) = rx.recv_timeout(Duration::from_millis(500)) {
        if let Event::TabChanged { tab } = event {
            changed.push(tab.name);
        }
    }
    changed.sort();
    assert_eq!(changed, ["Logs", "Terminal"]);
    let names: Vec<_> = shape(&client);
    assert!(names.contains(&("Terminal".to_string(), None)));
    assert!(names.contains(&("Logs".to_string(), None)));
    assert!(names.contains(&("Deep".to_string(), Some("Logs".to_string()))));
    assert!(!names.iter().any(|(n, _)| n == "claude"));
}

#[test]
fn the_parent_link_survives_a_restart() {
    let env = Env::new("persist");
    env.write_layout();
    let config = env.config("run");
    let server = Server::bind(config.clone()).unwrap();
    let (down_tx, down_rx) = mpsc::channel();
    server.on_shutdown(move || down_tx.send(()).unwrap());
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    create(&client, &env.work());
    let state = std::fs::read_to_string(env.dir.join("state").join("server.json")).unwrap();
    assert!(state.contains("\"parent\""), "{state}");

    client.shutdown_keeping_terminals().unwrap();
    down_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let client = start(&env.config("run2"));
    assert_eq!(shape(&client), expected());
}

#[test]
fn a_tab_place_is_stored_with_of_as_a_tab_id() {
    let env = Env::new("place");
    env.write_placed();
    let client = start(&env.config("run"));
    create(&client, &env.work());
    let layout = client.list_layout().unwrap();
    let id = |name: &str| {
        layout
            .tabs
            .iter()
            .find(|t| t.name == name)
            .unwrap()
            .tab_id
            .clone()
    };
    let place = |name: &str| {
        layout
            .tabs
            .iter()
            .find(|t| t.name == name)
            .unwrap()
            .place
            .clone()
    };
    let terminal = place("Terminal").unwrap();
    assert_eq!((terminal.of, terminal.ratio), (id("claude"), 0.6));
    let logs = place("Logs").unwrap();
    assert_eq!((logs.of, logs.ratio), (id("Terminal"), 0.5));
    // An unknown `of` is dropped by the parser: the tab opens without placement.
    assert_eq!(place("Lost"), None);
    assert_eq!(place("claude"), None);
}
