//! Grants survive a `poimen-server` restart within their duration (ADR 0018 amendment, #1453):
//! a lead gets a grant, the server stops keeping its terminals, a second server on the same state
//! file adopts the lead's host and restores the grant with its id and end. A grant that ran out,
//! was revoked, or whose lead did not come back is not restored.
//!
//! The lead runs in the scripted fake host of `tests/adopt.rs` (an in-process endpoint speaking
//! the host protocol). Every server lives in a temp directory: no test touches the user's server.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use poimen_client::Client;
use poimen_client::protocol::host::{HostMessage, HostStart, Size, to_line};
use poimen_client::protocol::{
    AgentGrantCheck, AgentGrantRequest, AgentGrantRevoke, AgentWriteAction, ClientInfo, ClientKind,
    Event, EventsSubscribe, GrantVerdict, TerminalCreate,
};
use poimen_client::transport::{Conn, Listener};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(10);

struct Env {
    dir: PathBuf,
}

impl Env {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("poimen-grants-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for d in ["work", "api"] {
            std::fs::create_dir_all(dir.join(d)).unwrap();
        }
        Self { dir }
    }

    /// Server number `n`: a socket of its own, the one state file all of them share.
    fn config(&self, n: u32, pty_host: PtyHost) -> Config {
        Config {
            socket: self.dir.join(format!("run{n}")).join("server.sock"),
            state_file: self.state_file(),
            history_dir: self.state_file().with_file_name("history"),
            scrollback: 100,
            home: None,
            pty_host,
        }
    }

    fn state_file(&self) -> PathBuf {
        self.dir.join("state").join("server.json")
    }

    fn state(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.state_file()).unwrap()).unwrap()
    }

    fn path(&self, name: &str) -> String {
        let p = self.dir.join(name);
        if cfg!(windows) {
            p.display().to_string()
        } else {
            p.canonicalize().unwrap().display().to_string()
        }
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn start(config: &Config) -> (Client, Receiver<()>) {
    let server = Server::bind(config.clone()).unwrap();
    let (tx, rx) = mpsc::channel();
    server.on_shutdown(move || {
        let _ = tx.send(());
    });
    thread::spawn(move || server.serve());
    (Client::new(&config.socket), rx)
}

/// One connection of the fake host: says `hello` and reads until the server goes.
fn serve_peer(conn: Conn, endpoint: &Path) {
    let hello = HostMessage::Hello {
        host_version: 1,
        version: "fake".into(),
        pid: 1,
        child_pid: Some(2),
        size: Size { cols: 80, rows: 24 },
        endpoint: endpoint.to_path_buf(),
        seq: 0,
    };
    let _ = (&conn).write_all(&to_line(&hello));
    let mut lines = BufReader::new(conn);
    let mut line = String::new();
    while matches!(lines.read_line(&mut line), Ok(n) if n > 0) {
        line.clear();
    }
}

/// A fake host per terminal. With `adoptable` it also answers the next server; without, it stops
/// listening once the first server is connected, so the next one cannot adopt it.
fn fake(adoptable: bool) -> PtyHost {
    PtyHost::launch(move |start: &HostStart| {
        let listener = Listener::bind(&start.endpoint).map_err(|e| e.to_string())?;
        let endpoint = start.endpoint.clone();
        thread::spawn(move || {
            let first = listener.accept().unwrap();
            if !adoptable {
                drop(listener);
                serve_peer(first, &endpoint);
                return;
            }
            let reader = {
                let endpoint = endpoint.clone();
                thread::spawn(move || serve_peer(first, &endpoint))
            };
            if let Ok(second) = listener.accept() {
                serve_peer(second, &endpoint);
            }
            let _ = reader.join();
        });
        Ok(())
    })
}

/// A host the second server must not start: adopted terminals already have theirs.
fn no_new_host() -> PtyHost {
    PtyHost::launch(|_: &HostStart| Err("no host is started on adoption".into()))
}

struct Lead {
    lead: String,
    work: String,
    api: String,
}

fn lead(client: &Client, env: &Env) -> Lead {
    let work = client.create_space("work", &env.path("work")).unwrap().id;
    let api = client.create_space("api", &env.path("api")).unwrap().id;
    let lead = client
        .create_terminal(&TerminalCreate {
            space_id: work.clone(),
            name: Some("lead".into()),
            command: Some(vec!["fake-agent".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    Lead { lead, work, api }
}

/// The lead's grant over both spaces, confirmed by a stand-in for the app; returns its id.
fn grant(client: &Client, l: &Lead) -> String {
    let events = client
        .subscribe_with(EventsSubscribe {
            client: Some(ClientInfo {
                kind: ClientKind::App,
                pid: std::process::id(),
            }),
            confirms: true,
            ..Default::default()
        })
        .unwrap();
    let app = client.clone();
    let answered = thread::spawn(move || {
        for event in events.flatten() {
            if let Event::AgentGrantRequested { id, .. } = event {
                app.request("agent.grant_answer", json!({"id": id, "approve": true}))
                    .unwrap();
                return;
            }
        }
    });
    let request: AgentGrantRequest = serde_json::from_value(json!({
        "caller_terminal_id": l.lead,
        "plan": "ship the milestone overnight",
        "spaces": [l.work, l.api],
        "tools": ["start_agent", "prompt_agent"],
        "max_agents": 2,
        "worktrees": true,
        "duration_s": 3600,
    }))
    .unwrap();
    let result = client.request_agent_grant(&request, WAIT).unwrap();
    answered.join().unwrap();
    assert!(result.granted, "{result:?}");
    result.grant.unwrap().id
}

fn check(client: &Client, lead: &str, space: &str) -> GrantVerdict {
    client
        .check_agent_grant(&AgentGrantCheck {
            caller_terminal_id: lead.into(),
            action: AgentWriteAction::PromptAgent,
            space_id: space.into(),
        })
        .unwrap()
}

fn stop(client: &Client, down: &Receiver<()>) {
    client.shutdown_keeping_terminals().unwrap();
    down.recv_timeout(WAIT).unwrap();
}

#[test]
fn a_grant_comes_back_with_its_id_and_end_after_a_restart() {
    let env = Env::new("restored");
    let (a, a_down) = start(&env.config(1, fake(true)));
    let l = lead(&a, &env);
    let id = grant(&a, &l);
    // On disk before the request answered, in the private state file.
    let state = env.state();
    assert_eq!(state["grants"][0]["id"], id.as_str(), "{state}");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(env.state_file())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let before = a.agent_grants().unwrap().grants;
    stop(&a, &a_down);

    let (b, _) = start(&env.config(2, no_new_host()));
    let after = b.agent_grants().unwrap().grants;
    assert_eq!(after.len(), 1, "{after:?}");
    assert_eq!(after[0].id, id);
    assert_eq!(after[0].expires_at, before[0].expires_at);
    assert_eq!(after[0].lead_terminal_id, l.lead);
    assert_eq!(after[0].max_agents, 2);
    assert!(after[0].worktrees, "the worktree line comes back too");
    assert_eq!(after[0].spaces.len(), 2);
    assert_eq!(
        check(&b, &l.lead, &l.api),
        GrantVerdict::Covered { grant: id }
    );
    assert_eq!(
        check(&b, "t-other", &l.api),
        GrantVerdict::NeedsConfirmation
    );
}

#[test]
fn a_revoked_grant_stays_revoked_after_a_restart() {
    let env = Env::new("revoked");
    let (a, a_down) = start(&env.config(1, fake(true)));
    let l = lead(&a, &env);
    let id = grant(&a, &l);
    let revoke = AgentGrantRevoke {
        id: Some(id),
        caller_terminal_id: None,
    };
    assert!(a.revoke_agent_grant(&revoke).unwrap());
    // Written before the revoke answered.
    assert_eq!(env.state().get("grants"), None);
    stop(&a, &a_down);

    let (b, _) = start(&env.config(2, no_new_host()));
    assert!(b.agent_grants().unwrap().grants.is_empty());
    assert_eq!(check(&b, &l.lead, &l.work), GrantVerdict::NeedsConfirmation);
}

#[test]
fn a_grant_that_ran_out_during_the_downtime_is_not_restored() {
    let env = Env::new("expired");
    let (a, a_down) = start(&env.config(1, fake(true)));
    let l = lead(&a, &env);
    grant(&a, &l);
    stop(&a, &a_down);
    // The downtime outlasts the grant: its end is now in the past by the wall clock.
    let mut state = env.state();
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    state["grants"][0]["expires_at"] = json!(now_ms - 1000);
    std::fs::write(env.state_file(), state.to_string()).unwrap();

    let (b, _) = start(&env.config(2, no_new_host()));
    assert_eq!(b.list_terminals(None).unwrap().len(), 1, "the lead is back");
    assert!(b.agent_grants().unwrap().grants.is_empty());
    assert_eq!(check(&b, &l.lead, &l.work), GrantVerdict::NeedsConfirmation);
    assert_eq!(env.state().get("grants"), None, "the file drops it too");
}

#[test]
fn a_grant_whose_lead_was_not_adopted_is_not_restored() {
    let env = Env::new("lead-gone");
    let (a, a_down) = start(&env.config(1, fake(false)));
    let l = lead(&a, &env);
    grant(&a, &l);
    stop(&a, &a_down);

    let (b, _) = start(&env.config(2, no_new_host()));
    assert!(b.agent_grants().unwrap().grants.is_empty());
    assert_eq!(check(&b, &l.lead, &l.work), GrantVerdict::NeedsConfirmation);
}

#[test]
fn a_restored_grant_loses_the_spaces_that_are_gone() {
    let env = Env::new("space-gone");
    let (a, a_down) = start(&env.config(1, fake(true)));
    let l = lead(&a, &env);
    let id = grant(&a, &l);
    stop(&a, &a_down);
    // The api space went while no server ran.
    let mut state = env.state();
    let spaces = state["spaces"].as_array_mut().unwrap();
    spaces.retain(|s| s["id"] != l.api.as_str());
    std::fs::write(env.state_file(), state.to_string()).unwrap();

    let (b, _) = start(&env.config(2, no_new_host()));
    let after = b.agent_grants().unwrap().grants;
    assert_eq!(after.len(), 1);
    let ids: Vec<&str> = after[0].spaces.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, [l.work.as_str()]);
    assert_eq!(
        check(&b, &l.lead, &l.work),
        GrantVerdict::Covered { grant: id }
    );
    assert_eq!(check(&b, &l.lead, &l.api), GrantVerdict::NeedsConfirmation);
}

/// A change of a stored grant at a time (ms since the epoch).
type Edit = fn(&mut Value, u64);

#[test]
fn a_stored_grant_no_request_could_get_is_not_restored() {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let edits: [(&str, Edit); 3] = [
        ("longer", |g, now| {
            g["expires_at"] = json!(now + 25 * 3600 * 1000)
        }),
        ("write", |g, _| g["tools"] = json!(["write_terminal"])),
        ("zero", |g, _| g["max_agents"] = json!(0)),
    ];
    for (name, edit) in edits {
        let env = Env::new(name);
        let (a, a_down) = start(&env.config(1, fake(true)));
        let l = lead(&a, &env);
        grant(&a, &l);
        stop(&a, &a_down);
        let mut state = env.state();
        edit(&mut state["grants"][0], now_ms);
        std::fs::write(env.state_file(), state.to_string()).unwrap();

        let (b, _) = start(&env.config(2, no_new_host()));
        assert!(b.agent_grants().unwrap().grants.is_empty(), "{name}");
        assert_eq!(
            check(&b, &l.lead, &l.work),
            GrantVerdict::NeedsConfirmation,
            "{name}"
        );
    }
}
