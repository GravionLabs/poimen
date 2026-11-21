//! The silent mark of an agent that is `working` without a report (#898).

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::Client;
use poimen_client::protocol::{AgentInfo, Event, TerminalCreate};
use poimen_server::{Config, PtyHost, Server};
use serde_json::json;

const MIN: u64 = 60_000;
const WORKING: &str = r"\033]7501;state=working:app=loop\007";

struct Env {
    dir: PathBuf,
    config: Config,
    client: Client,
    terminal: String,
    sweep: Box<dyn Fn(u64, u64) + Send + Sync>,
    down: Receiver<()>,
}

type Sweep = Box<dyn Fn(u64, u64) + Send + Sync>;

fn serve(config: &Config) -> (Client, Sweep, Receiver<()>) {
    let deadline = Instant::now() + Duration::from_secs(10);
    let server = loop {
        match Server::bind(config.clone()) {
            Ok(s) => break s,
            Err(e) => assert!(Instant::now() < deadline, "bind: {e}"),
        }
        thread::sleep(Duration::from_millis(20));
    };
    let sweep = Box::new(server.silent_sweep());
    let (tx, rx) = mpsc::channel();
    server.on_shutdown(move || {
        let _ = tx.send(());
    });
    thread::spawn(move || server.serve());
    (Client::new(&config.socket), sweep, rx)
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-silent-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("work")).unwrap();
        let config = Config {
            socket: dir.join("run").join("server.sock"),
            state_file: dir.join("state").join("server.json"),
            scrollback: 1000,
            home: None,
            pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
        };
        let (client, sweep, down) = serve(&config);
        let work = dir.join("work").canonicalize().unwrap();
        let space = client
            .create_space("api", &work.display().to_string())
            .unwrap();
        let terminal = client
            .create_terminal(&TerminalCreate {
                space_id: space.id,
                command: Some(vec!["sh".into()]),
                ..Default::default()
            })
            .unwrap()
            .id;
        Self {
            dir,
            config,
            client,
            terminal,
            sweep,
            down,
        }
    }

    fn report(&self, seq: u64, status: &str) {
        let r = self
            .client
            .request(
                "terminal.report_agent",
                json!({"terminal_id": self.terminal, "seq": seq, "status": status}),
            )
            .unwrap();
        assert!(r["accepted"].as_bool().unwrap());
    }

    fn agent(&self) -> AgentInfo {
        agent_of(&self.client)
    }

    /// Runs a shell command that prints an OSC 7501 report.
    fn print(&self, escape: &str) {
        self.client
            .input(&self.terminal, &format!("printf '{escape}'\n"))
            .unwrap();
    }

    /// Waits until the agent exists and `reported_at` is not `after` anymore.
    fn reported_after(&self, after: Option<u64>) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let at = self
                .client
                .list_terminals(None)
                .unwrap()
                .remove(0)
                .agent
                .and_then(|a| a.reported_at);
            if let Some(at) = at.filter(|at| Some(*at) != after) {
                return at;
            }
            assert!(Instant::now() < deadline, "no report arrived");
            thread::yield_now();
        }
    }

    /// Repeats the report until `reported_at` moves past `after` (the clock has milliseconds).
    fn repeat_after(&self, after: u64) -> u64 {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.print(WORKING);
            let at = self.reported_after(Some(after));
            if at > after {
                return at;
            }
            assert!(Instant::now() < deadline, "no repeat arrived");
        }
    }

    fn wait_silent_cleared(&self) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.agent().silent_since.is_some() {
            assert!(Instant::now() < deadline, "mark stays");
            thread::yield_now();
        }
    }
}

fn agent_of(client: &Client) -> AgentInfo {
    client
        .list_terminals(None)
        .unwrap()
        .remove(0)
        .agent
        .expect("agent")
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

/// The `silent_since` of every status event received so far.
fn silent_events(rx: &Receiver<Event>) -> Vec<Option<u64>> {
    thread::sleep(Duration::from_millis(100));
    rx.try_iter()
        .filter_map(|e| match e {
            Event::AgentStatusChanged { silent_since, .. } => Some(silent_since),
            _ => None,
        })
        .collect()
}

#[test]
fn a_working_agent_turns_silent_once_and_a_report_clears_it() {
    let env = Env::new("once");
    let rx = events(&env.client);
    env.report(1, "working");
    let reported = env.agent().reported_at.unwrap();
    assert_eq!(silent_events(&rx), vec![None]);

    (env.sweep)(reported + 10 * MIN - 1, 10);
    assert_eq!(env.agent().silent_since, None, "not yet");
    (env.sweep)(reported + 10 * MIN, 10);
    assert_eq!(env.agent().silent_since, Some(reported + 10 * MIN));
    // Later sweeps change nothing and publish nothing more.
    (env.sweep)(reported + 11 * MIN, 10);
    assert_eq!(env.agent().silent_since, Some(reported + 10 * MIN));
    assert_eq!(silent_events(&rx), vec![Some(reported + 10 * MIN)]);

    // The next report of the same status clears the mark with one event.
    env.report(2, "working");
    assert_eq!(env.agent().silent_since, None);
    assert_eq!(silent_events(&rx), vec![None]);
}

#[test]
fn another_status_is_never_silent_and_clears_the_mark() {
    for status in ["blocked", "done", "idle"] {
        let env = Env::new(status);
        env.report(1, status);
        let reported = env.agent().reported_at.unwrap();
        (env.sweep)(reported + 60 * MIN, 10);
        assert_eq!(env.agent().silent_since, None, "{status}");
    }
    let env = Env::new("cleared");
    env.report(1, "working");
    let reported = env.agent().reported_at.unwrap();
    (env.sweep)(reported + 10 * MIN, 10);
    assert!(env.agent().silent_since.is_some());
    env.report(2, "blocked");
    let agent = env.agent();
    assert_eq!(agent.silent_since, None);
    (env.sweep)(reported + 60 * MIN, 10);
    assert_eq!(env.agent().silent_since, None);
}

#[test]
fn zero_turns_it_off() {
    let env = Env::new("off");
    env.report(1, "working");
    let reported = env.agent().reported_at.unwrap();
    (env.sweep)(reported + 600 * MIN, 0);
    assert_eq!(env.agent().silent_since, None);
}

#[test]
fn the_mark_is_in_the_server_file_and_survives_a_restart() {
    let env = Env::new("restart");
    env.report(1, "working");
    let reported = env.agent().reported_at.unwrap();
    // A restart before the time is up: the stored report time still counts.
    env.client.shutdown_keeping_terminals().unwrap();
    // Wait until the first server let go of the socket and the lock.
    env.down.recv_timeout(Duration::from_secs(10)).unwrap();
    // (another socket, same state file: Windows keeps the first pipe a while)
    let config2 = Config {
        socket: env.dir.join("run2").join("server.sock"),
        ..env.config.clone()
    };
    let (client, sweep, second) = serve(&config2);
    assert_eq!(agent_of(&client).silent_since, None);
    sweep(reported + 10 * MIN, 10);
    let marked = agent_of(&client);
    assert_eq!(marked.silent_since, Some(reported + 10 * MIN));
    let text = std::fs::read_to_string(env.dir.join("state").join("server.json")).unwrap();
    assert!(
        text.contains(&format!("\"silent_since\": {}", reported + 10 * MIN)),
        "{text}"
    );
    // And after another restart the mark is still there.
    client.shutdown_keeping_terminals().unwrap();
    second.recv_timeout(Duration::from_secs(10)).unwrap();
    let config3 = Config {
        socket: env.dir.join("run3").join("server.sock"),
        ..env.config.clone()
    };
    let (client, _, _) = serve(&config3);
    assert_eq!(agent_of(&client).silent_since, marked.silent_since);
}

#[test]
fn a_repeated_osc_report_keeps_a_working_program_alive() {
    let env = Env::new("osc-heartbeat");
    let rx = events(&env.client);
    env.print(WORKING);
    let first = env.reported_after(None);
    assert_eq!(silent_events(&rx), vec![None]);
    // The same state again moves `reported_at` and publishes nothing.
    let reported = env.repeat_after(first);
    assert_eq!(silent_events(&rx), Vec::<Option<u64>>::new());

    (env.sweep)(reported + 10 * MIN - 1, 10);
    assert_eq!(env.agent().silent_since, None);
    (env.sweep)(reported + 10 * MIN, 10);
    assert_eq!(env.agent().silent_since, Some(reported + 10 * MIN));
    assert_eq!(silent_events(&rx), vec![Some(reported + 10 * MIN)]);

    // The repeat after the mark clears it with one event.
    env.print(WORKING);
    env.wait_silent_cleared();
    assert_eq!(silent_events(&rx), vec![None]);
}

#[test]
fn output_without_a_report_does_not_keep_a_program_alive() {
    let env = Env::new("osc-output");
    env.print(WORKING);
    let reported = env.reported_after(None);
    // Plain output, then a child record: the shell runs both in order, so the record shows
    // that the output went through. A child record never moves the root `reported_at`.
    env.client
        .input(
            &env.terminal,
            "echo still-printing; printf '\\033]7501;state=working:id=probe\\007'\n",
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while env
        .client
        .list_terminals(None)
        .unwrap()
        .remove(0)
        .records
        .is_empty()
    {
        assert!(Instant::now() < deadline, "output did not arrive");
        thread::yield_now();
    }
    assert_eq!(env.agent().reported_at, Some(reported));
    (env.sweep)(reported + 10 * MIN, 10);
    assert_eq!(env.agent().silent_since, Some(reported + 10 * MIN));
}
