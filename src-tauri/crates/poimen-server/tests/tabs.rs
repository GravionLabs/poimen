//! Tabs and split trees (`tab.*`, protocol 4) end to end: a real server on a temp socket with
//! real shells in a temp dir, the real client. Never the user's server.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::protocol::{
    ErrorCode, Event, Node, Space, SplitDirection, Tab, TabCreate, TabSplit, Terminal,
    TerminalCreate,
};
use poimen_client::{Client, ClientError};
use poimen_server::{Config, PtyHost, Server};
use serde_json::Value;

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-tabs-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        Self { dir }
    }

    fn config(&self, run: &str) -> Config {
        Config {
            socket: self.dir.join(run).join("server.sock"),
            state_file: self.dir.join("state").join("server.json"),
            scrollback: 1000,
            home: None,
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        }
    }

    fn work(&self) -> String {
        self.dir.join("work").display().to_string()
    }

    fn state(&self) -> Value {
        let text = std::fs::read_to_string(self.dir.join("state").join("server.json")).unwrap();
        serde_json::from_str(&text).unwrap()
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

fn wait_for_event(rx: &Receiver<Event>, pred: impl Fn(&Event) -> bool) -> Event {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(left).expect("event did not arrive");
        if pred(&event) {
            return event;
        }
    }
}

/// The names of the events that arrive within a short while (the ones a request caused).
fn drain(rx: &Receiver<Event>) -> Vec<String> {
    let mut names = Vec::new();
    while let Ok(event) = rx.recv_timeout(Duration::from_millis(300)) {
        let value = serde_json::to_value(&event).unwrap();
        names.push(value["event"].as_str().unwrap().to_string());
    }
    names
}

const SHELL: &str = if cfg!(windows) { "cmd.exe" } else { "sh" };
const ENTER: &str = if cfg!(windows) { "\r" } else { "\n" };

fn sh(space_id: &str) -> TerminalCreate {
    TerminalCreate {
        space_id: space_id.into(),
        command: Some(vec![SHELL.into()]),
        ..Default::default()
    }
}

fn split_of(terminal_id: &str, direction: SplitDirection, ratio: Option<f64>) -> TabSplit {
    TabSplit {
        terminal_id: terminal_id.into(),
        direction,
        ratio,
        cwd: None,
        name: None,
        command: Some(vec![SHELL.into()]),
    }
}

fn code<T: std::fmt::Debug>(r: Result<T, ClientError>) -> ErrorCode {
    match r {
        Err(ClientError::Server { code, .. }) => code,
        other => panic!("expected a server error, got {other:?}"),
    }
}

fn space_of(client: &Client, id: &str) -> Space {
    client
        .list_layout()
        .unwrap()
        .spaces
        .into_iter()
        .find(|s| s.id == id)
        .unwrap()
}

fn tab_of(client: &Client, id: &str) -> Option<Tab> {
    client
        .list_layout()
        .unwrap()
        .tabs
        .into_iter()
        .find(|t| t.tab_id == id)
}

fn leaf(id: &str) -> Node {
    Node::terminal(id)
}

fn split(id: &str, direction: SplitDirection, ratio: f64, a: Node, b: Node) -> Node {
    Node::Split {
        split_id: id.into(),
        direction,
        ratio,
        children: Box::new([a, b]),
    }
}

fn split_id(node: &Node) -> String {
    match node {
        Node::Split { split_id, .. } => split_id.clone(),
        other => panic!("not a split: {other:?}"),
    }
}

/// A space with one terminal; returns the space, the terminal and its tab.
fn space_with_terminal(client: &Client, env: &Env) -> (Space, Terminal, Tab) {
    let space = client.create_space("work", &env.work()).unwrap();
    let terminal = client.create_terminal(&sh(&space.id)).unwrap();
    let layout = client.list_layout().unwrap();
    let tab = layout
        .tabs
        .into_iter()
        .find(|t| t.root.contains_terminal(&terminal.id))
        .unwrap();
    (space, terminal, tab)
}

/// Every running terminal is in exactly one leaf, and every leaf is a running terminal.
fn assert_consistent(client: &Client) {
    let layout = client.list_layout().unwrap();
    let mut leaves: Vec<String> = layout
        .tabs
        .iter()
        .flat_map(|t| t.root.terminal_ids())
        .map(str::to_string)
        .collect();
    leaves.sort();
    let mut running: Vec<String> = client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .filter(|t| t.status == poimen_client::protocol::TerminalStatus::Running)
        .map(|t| t.id)
        .collect();
    running.sort();
    assert_eq!(leaves, running);
    for space in &layout.spaces {
        let own: Vec<&str> = layout
            .tabs
            .iter()
            .filter(|t| t.space_id == space.id)
            .map(|t| t.tab_id.as_str())
            .collect();
        let mut listed: Vec<&str> = space.tabs.iter().map(String::as_str).collect();
        listed.sort_unstable();
        let mut own_sorted = own.clone();
        own_sorted.sort_unstable();
        assert_eq!(listed, own_sorted, "tabs of {}", space.id);
    }
}

#[test]
fn a_terminal_created_without_a_tab_gets_one_and_the_space_lists_it() {
    let env = Env::new("create");
    let client = start(&env.config("run"));
    let space = client.create_space("work", &env.work()).unwrap();
    assert!(space.tabs.is_empty());
    let rx = events(&client);

    let terminal = client.create_terminal(&sh(&space.id)).unwrap();

    assert_eq!(
        drain(&rx),
        ["terminal.created", "tab.created", "space.changed"]
    );
    let layout = client.list_layout().unwrap();
    assert_eq!(layout.tabs.len(), 1);
    let tab = &layout.tabs[0];
    assert_eq!(tab.root, leaf(&terminal.id));
    assert_eq!(tab.name, terminal.name);
    assert_eq!(tab.space_id, space.id);
    assert_eq!(tab.active_terminal, terminal.id);
    let space = &layout.spaces[0];
    assert_eq!(space.tabs, std::slice::from_ref(&tab.tab_id));
    assert_eq!(space.active_tab.as_ref(), Some(&tab.tab_id));
    // The protocol 3 view still works.
    assert_eq!(client.list_spaces().unwrap()[0].tabs, space.tabs);
}

#[test]
fn split_resize_by_split_id_and_errors() {
    let env = Env::new("split");
    let client = start(&env.config("run"));
    let (_, first, tab) = space_with_terminal(&client, &env);
    let rx = events(&client);

    let (after, second) = client
        .split(&split_of(&first.id, SplitDirection::Right, Some(0.3)))
        .unwrap();
    assert_eq!(drain(&rx), ["terminal.created", "tab.changed"]);
    let outer = split_id(&after.root);
    assert_eq!(
        after.root,
        split(
            &outer,
            SplitDirection::Right,
            0.3,
            leaf(&first.id),
            leaf(&second.id)
        )
    );
    assert_eq!(after.active_terminal, first.id);
    assert_eq!(second.cwd, first.cwd);

    // Another client splits the first leaf; the outer split keeps its id though its path moved.
    let (after, third) = client
        .split(&split_of(&first.id, SplitDirection::Down, None))
        .unwrap();
    let inner = match &after.root {
        Node::Split { children, .. } => split_id(&children[0]),
        other => panic!("{other:?}"),
    };
    assert_ne!(inner, outer);
    let resized = client.resize_split(&tab.tab_id, &outer, 0.75).unwrap();
    assert_eq!(
        resized.root,
        split(
            &outer,
            SplitDirection::Right,
            0.75,
            split(
                &inner,
                SplitDirection::Down,
                0.5,
                leaf(&first.id),
                leaf(&third.id)
            ),
            leaf(&second.id)
        )
    );
    assert_eq!(tab_of(&client, &tab.tab_id).unwrap(), resized);

    assert_eq!(
        code(client.resize_split(&tab.tab_id, "split999", 0.5)),
        ErrorCode::NotFound
    );
    assert_eq!(
        code(client.resize_split(&tab.tab_id, &outer, 1.0)),
        ErrorCode::InvalidParams
    );
    assert_eq!(
        code(client.split(&split_of(&first.id, SplitDirection::Down, Some(0.0)))),
        ErrorCode::InvalidParams
    );
    assert_eq!(
        code(client.split(&split_of("t999", SplitDirection::Down, None))),
        ErrorCode::NotFound
    );
    assert_consistent(&client);
}

#[test]
fn closing_a_terminal_collapses_its_split_and_the_last_one_closes_the_tab() {
    let env = Env::new("close");
    let client = start(&env.config("run"));
    let (space, first, tab) = space_with_terminal(&client, &env);
    let (_, second) = client
        .split(&split_of(&first.id, SplitDirection::Right, None))
        .unwrap();
    client.focus_tab(&tab.tab_id, Some(&first.id)).unwrap();
    let rx = events(&client);

    client.close_terminal(&first.id).unwrap();

    assert_eq!(drain(&rx), ["terminal.closed", "tab.changed"]);
    let left = tab_of(&client, &tab.tab_id).unwrap();
    assert_eq!(left.root, leaf(&second.id));
    assert_eq!(left.active_terminal, second.id);

    client.close_terminal(&second.id).unwrap();
    assert_eq!(
        drain(&rx),
        ["terminal.closed", "tab.closed", "space.changed"]
    );
    assert!(tab_of(&client, &tab.tab_id).is_none());
    let space = space_of(&client, &space.id);
    assert!(space.tabs.is_empty());
    assert_eq!(space.active_tab, None);
}

#[test]
fn an_exited_terminal_leaves_the_tree_but_stays_listed() {
    let env = Env::new("exit");
    let client = start(&env.config("run"));
    let (_, first, tab) = space_with_terminal(&client, &env);
    let (_, second) = client
        .split(&split_of(&first.id, SplitDirection::Down, None))
        .unwrap();
    let rx = events(&client);

    client.input(&second.id, &format!("exit 0{ENTER}")).unwrap();

    wait_for_event(
        &rx,
        |e| matches!(e, Event::TerminalExited { terminal_id, .. } if *terminal_id == second.id),
    );
    let changed = wait_for_event(&rx, |e| matches!(e, Event::TabChanged { .. }));
    let Event::TabChanged { tab: changed } = changed else {
        unreachable!()
    };
    assert_eq!(changed.root, leaf(&first.id));
    assert_eq!(tab_of(&client, &tab.tab_id).unwrap().root, leaf(&first.id));
    assert_eq!(client.list_terminals(None).unwrap().len(), 2);
    assert_consistent(&client);
    // An exited terminal can be neither split nor given a tab.
    assert_eq!(
        code(client.split(&split_of(&second.id, SplitDirection::Down, None))),
        ErrorCode::Exited
    );

    client.input(&first.id, &format!("exit 0{ENTER}")).unwrap();
    wait_for_event(
        &rx,
        |e| matches!(e, Event::TabClosed { tab_id, .. } if *tab_id == tab.tab_id),
    );
    assert!(client.list_layout().unwrap().tabs.is_empty());
}

#[test]
fn tab_create_with_a_new_or_an_existing_terminal_rename_and_close() {
    let env = Env::new("tabs");
    let client = start(&env.config("run"));
    let (space, first, first_tab) = space_with_terminal(&client, &env);
    let (_, second) = client
        .split(&split_of(&first.id, SplitDirection::Right, None))
        .unwrap();
    let rx = events(&client);

    // A new terminal in a new tab, named as asked.
    let (tab, fresh) = client
        .create_tab(&TabCreate {
            space_id: space.id.clone(),
            name: Some("logs".into()),
            terminal_id: None,
        })
        .unwrap();
    assert_eq!(
        drain(&rx),
        ["terminal.created", "tab.created", "space.changed"]
    );
    assert_eq!(
        (tab.name.as_str(), tab.root.clone()),
        ("logs", leaf(&fresh.id))
    );

    // An existing terminal moves out of its split into a tab of its own.
    let (moved, terminal) = client
        .create_tab(&TabCreate {
            space_id: space.id.clone(),
            name: None,
            terminal_id: Some(second.id.clone()),
        })
        .unwrap();
    assert_eq!(drain(&rx), ["tab.changed", "tab.created", "space.changed"]);
    assert_eq!(terminal.id, second.id);
    assert_eq!(moved.root, leaf(&second.id));
    assert_eq!(moved.name, second.name);
    assert_eq!(
        tab_of(&client, &first_tab.tab_id).unwrap().root,
        leaf(&first.id)
    );
    assert_eq!(
        space_of(&client, &space.id).tabs,
        [
            first_tab.tab_id.clone(),
            tab.tab_id.clone(),
            moved.tab_id.clone()
        ]
    );

    let renamed = client.rename_tab(&tab.tab_id, "build").unwrap();
    assert_eq!(renamed.name, "build");
    assert_eq!(drain(&rx), ["tab.changed"]);
    assert_eq!(
        code(client.rename_tab(&tab.tab_id, "  ")),
        ErrorCode::InvalidParams
    );

    // Closing a tab closes its terminals.
    client.close_tab(&first_tab.tab_id).unwrap();
    assert_eq!(
        drain(&rx),
        ["terminal.closed", "tab.closed", "space.changed"]
    );
    let ids: Vec<String> = client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, [second.id.clone(), fresh.id.clone()]);
    // The active tab was closed: the one that took its place is active now.
    assert_eq!(
        space_of(&client, &space.id).active_tab.as_ref(),
        Some(&tab.tab_id)
    );

    // A terminal of another space, an unknown space, tab or terminal.
    let other = client.create_space("other", &env.work()).unwrap();
    let foreign = TabCreate {
        space_id: other.id.clone(),
        name: None,
        terminal_id: Some(fresh.id.clone()),
    };
    assert_eq!(code(client.create_tab(&foreign)), ErrorCode::InvalidParams);
    let unknown = TabCreate {
        space_id: "s999".into(),
        ..Default::default()
    };
    assert_eq!(code(client.create_tab(&unknown)), ErrorCode::NotFound);
    assert_eq!(code(client.close_tab("tab999")), ErrorCode::NotFound);
    let gone = TabCreate {
        space_id: space.id.clone(),
        name: None,
        terminal_id: Some("t999".into()),
    };
    assert_eq!(code(client.create_tab(&gone)), ErrorCode::NotFound);
    assert_consistent(&client);
}

#[test]
fn move_and_focus_are_shared_and_reported() {
    let env = Env::new("move");
    let client = start(&env.config("run"));
    let (space, first, a) = space_with_terminal(&client, &env);
    let b = client
        .create_tab(&TabCreate {
            space_id: space.id.clone(),
            ..Default::default()
        })
        .unwrap()
        .0;
    let c = client
        .create_tab(&TabCreate {
            space_id: space.id.clone(),
            ..Default::default()
        })
        .unwrap()
        .0;
    let rx = events(&client);

    let moved = client.move_tab(&c.tab_id, 0).unwrap();
    assert_eq!(
        moved.tabs,
        [c.tab_id.clone(), a.tab_id.clone(), b.tab_id.clone()]
    );
    assert_eq!(drain(&rx), ["tab.changed", "space.changed"]);
    let moved = client.move_tab(&c.tab_id, 50).unwrap();
    assert_eq!(
        moved.tabs,
        [a.tab_id.clone(), b.tab_id.clone(), c.tab_id.clone()]
    );
    assert_eq!(code(client.move_tab("tab999", 0)), ErrorCode::NotFound);

    let (_, second) = client
        .split(&split_of(&first.id, SplitDirection::Down, None))
        .unwrap();
    drain(&rx);
    // Two clients focus one after the other: the last one wins.
    let other = Client::new(client.socket());
    client.focus_tab(&b.tab_id, None).unwrap();
    let focused = other.focus_tab(&a.tab_id, Some(&second.id)).unwrap();
    assert_eq!(focused.active_terminal, second.id);
    assert_eq!(
        drain(&rx),
        [
            "tab.changed",
            "space.changed",
            "tab.changed",
            "space.changed"
        ]
    );
    let space = space_of(&client, &space.id);
    assert_eq!(space.active_tab.as_ref(), Some(&a.tab_id));
    assert_eq!(
        tab_of(&client, &a.tab_id).unwrap().active_terminal,
        second.id
    );
    // A terminal that is not a leaf of the tab is refused.
    assert_eq!(
        code(client.focus_tab(&b.tab_id, Some(&first.id))),
        ErrorCode::NotFound
    );
}

#[test]
fn closing_a_space_closes_its_tabs() {
    let env = Env::new("space");
    let client = start(&env.config("run"));
    let (space, _, tab) = space_with_terminal(&client, &env);
    let rx = events(&client);
    client.close_space(&space.id).unwrap();
    assert_eq!(
        drain(&rx),
        ["terminal.closed", "tab.closed", "space.closed"]
    );
    assert!(client.list_layout().unwrap().tabs.is_empty());
    assert!(tab_of(&client, &tab.tab_id).is_none());
}

#[test]
fn tabs_and_trees_are_persisted_and_kept_with_their_adopted_terminals_after_a_restart() {
    let env = Env::new("persist");
    let config = env.config("run");
    let server = Server::bind(config.clone()).unwrap();
    let (down_tx, down_rx) = mpsc::channel();
    server.on_shutdown(move || down_tx.send(()).unwrap());
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let (space, first, tab) = space_with_terminal(&client, &env);
    let (after, second) = client
        .split(&split_of(&first.id, SplitDirection::Right, Some(0.4)))
        .unwrap();
    let old_split = split_id(&after.root);
    client.rename_tab(&tab.tab_id, "dev").unwrap();

    // The file carries the spaces with their tab order, the tabs with their trees.
    let state = env.state();
    assert_eq!(state["spaces"][0]["tabs"][0], tab.tab_id.as_str());
    assert_eq!(state["spaces"][0]["active_tab"], tab.tab_id.as_str());
    assert_eq!(state["tabs"][0]["name"], "dev");
    assert_eq!(state["tabs"][0]["root"]["split_id"], old_split.as_str());
    assert_eq!(state["tabs"][0]["root"]["ratio"], 0.4);
    assert!(state["next_id"].as_u64().is_some());

    // A restart that keeps the terminals: the new server (same state file) adopts them, so the
    // tab keeps its tree with the same split and terminal ids (the leaves of terminals that do
    // not come back are dropped, see tests/adopt.rs).
    client.shutdown_keeping_terminals().unwrap();
    down_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    let client = start(&env.config("run2"));
    let layout = client.list_layout().unwrap();
    assert_eq!(layout.spaces.len(), 1);
    assert_eq!(layout.spaces[0].id, space.id);
    assert_eq!(layout.spaces[0].tabs, std::slice::from_ref(&tab.tab_id));
    assert_eq!(layout.tabs.len(), 1);
    assert_eq!(layout.tabs[0].root, after.root);
    assert_eq!(layout.tabs[0].name, "dev");
    let ids: Vec<String> = client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect();
    assert_eq!(ids, [first.id.clone(), second.id.clone()]);

    // Ids are never given twice: a new split and tab get fresh ones.
    let fresh = client.create_terminal(&sh(&space.id)).unwrap();
    let (after, _) = client
        .split(&split_of(&fresh.id, SplitDirection::Down, None))
        .unwrap();
    assert_ne!(split_id(&after.root), old_split);
    assert_ne!(after.tab_id, tab.tab_id);
    assert!(![first.id, second.id].contains(&fresh.id));
}

#[test]
fn a_protocol_3_state_file_loads_and_new_terminals_get_tabs() {
    let env = Env::new("upgrade");
    std::fs::create_dir_all(env.dir.join("state")).unwrap();
    std::fs::write(
        env.dir.join("state").join("server.json"),
        format!(
            r#"{{"spaces":[{{"id":"s7","name":"api","cwd":{:?}}}]}}"#,
            env.work()
        ),
    )
    .unwrap();
    let client = start(&env.config("run"));
    let layout = client.list_layout().unwrap();
    assert_eq!(layout.spaces[0].id, "s7");
    assert!(layout.spaces[0].tabs.is_empty());
    let terminal = client.create_terminal(&sh("s7")).unwrap();
    let space = space_of(&client, "s7");
    assert_eq!(space.tabs.len(), 1);
    assert_eq!(
        tab_of(&client, &space.tabs[0]).unwrap().root,
        leaf(&terminal.id)
    );
    // Ids continue after the persisted space.
    assert!(client.create_space("x", &env.work()).unwrap().id != "s7");
}
