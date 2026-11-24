//! `poimen session run` (#1257) as the real binary against an in-process poimen-server on a
//! temporary socket and a fake harness (a shell script, never a real agent): the record
//! sequence, each exit code, the signals, `--close-on-end` and a prompt that must arrive as one
//! argument. Unix-only (signals, a FIFO, `sh`).
#![cfg(unix)]

use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::Duration;

use poimen_client::Client;
use poimen_client::protocol::{AgentStatus, TerminalCreate};
use poimen_server::{Config, PtyHost, Server};
use serde_json::{Value, json};

const WAIT: Duration = Duration::from_secs(30);

/// A prompt with spaces, quotes, `$()`, `;` and a newline.
const NASTY_PROMPT: &str = "fix it's \"bug\" $(touch pwned); echo `x`\nsecond line; done";

struct Fixture {
    dir: PathBuf,
    client: Client,
    space: String,
    other_space: String,
    caller: String,
    fifo: PathBuf,
    started: PathBuf,
    exited: PathBuf,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn report(client: &Client, terminal: &str, seq: u64, status: AgentStatus) {
    client
        .request(
            "terminal.report_agent",
            json!({"terminal_id": terminal, "seq": seq, "status": status, "agent": "claude"}),
        )
        .unwrap();
}

fn fixture(name: &str) -> Fixture {
    let dir = std::env::temp_dir().join(format!("poimen-srun-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let work = dir.join("work");
    std::fs::create_dir_all(&work).unwrap();
    let config = Config {
        socket: dir.join("run").join("server.sock"),
        state_file: dir.join("state").join("server.json"),
        history_dir: dir.join("state").join("history"),
        scrollback: 1000,
        home: None,
        pty_host: PtyHost::launch(poimen_pty_host::spawn_in_thread),
    };
    let server = Server::bind(config.clone()).unwrap();
    thread::spawn(move || server.serve());
    let client = Client::new(&config.socket);
    let cwd = work.display().to_string();
    let space = client.create_space("work", &cwd).unwrap().id;
    let other_space = client.create_space("other", &cwd).unwrap().id;
    let caller = client
        .create_terminal(&TerminalCreate {
            space_id: space.clone(),
            name: Some("caller".into()),
            command: Some(vec!["sleep".into(), "120".into()]),
            ..Default::default()
        })
        .unwrap()
        .id;
    let fifo = dir.join("go.fifo");
    let ok = poimen_core::gui_path::command("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success();
    assert!(ok, "mkfifo");
    let (started, exited) = (dir.join("started.txt"), dir.join("exit-input.txt"));
    // The fake harness: records its argument and environment, tells the test it runs (the FIFO),
    // then reads one line (the `exitInput`) and records it.
    let script = format!(
        "printf '%s|%s|%s' \"$1\" \"$AGENT_EVENTS_RUN\" \"$FOO\" > {started}; \
         echo go > {fifo}; read line; printf '%s' \"$line\" > {exited}; sleep 120",
        started = started.display(),
        fifo = fifo.display(),
        exited = exited.display(),
    );
    let home = dir.join("home");
    let cfg = poimen_core::settings::config_path(&home);
    std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
    std::fs::write(
        cfg,
        json!({"harnesses": {
            "claude": {"command": ["sh", "-c", script, "sh"], "promptArgs": ["{prompt}"],
                       "exitInput": "bye"},
            "other": {"command": ["true"]},
        }})
        .to_string(),
    )
    .unwrap();
    Fixture {
        dir,
        client,
        space,
        other_space,
        caller,
        fifo,
        started,
        exited,
    }
}

impl Fixture {
    /// Reports the agent as idle (its `SessionStart`) once the harness runs in a new terminal.
    fn start_reporter(&self) {
        let (client, fifo, caller) = (self.client.clone(), self.fifo.clone(), self.caller.clone());
        thread::spawn(move || {
            // Blocks until the harness writes: the command line was typed and runs.
            let mut sink = String::new();
            let _ = std::fs::File::open(&fifo).and_then(|mut f| f.read_to_string(&mut sink));
            let new = client
                .list_terminals(None)
                .unwrap()
                .into_iter()
                .find(|t| t.id != caller)
                .unwrap();
            report(&client, &new.id, 1, AgentStatus::Idle);
        });
    }

    fn poimen(&self, socket: &Path, extra: &[&str]) -> Proc {
        let mut args = vec!["session", "run", "--harness", "claude"];
        args.extend(extra);
        let mut child = poimen_core::gui_path::command(env!("CARGO_BIN_EXE_poimen"))
            .args(&args)
            .env("POIMEN_SOCKET", socket)
            .env("POIMEN_TERMINAL_ID", &self.caller)
            .env("HOME", self.dir.join("home"))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    return;
                }
            }
        });
        Proc { child, lines }
    }

    fn run(&self, extra: &[&str]) -> Proc {
        self.poimen(self.client.socket(), extra)
    }
}

struct Proc {
    child: Child,
    lines: Receiver<String>,
}

impl Drop for Proc {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Proc {
    fn record(&self) -> Value {
        let line = self.lines.recv_timeout(WAIT).expect("a record on stdout");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON ({e}): {line}"))
    }

    /// The lines still to come until stdout closes, the exit code and stderr.
    fn finish(mut self) -> (Vec<Value>, i32, String) {
        let mut rest = Vec::new();
        while let Ok(line) = self.lines.recv_timeout(WAIT) {
            rest.push(serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}")));
        }
        let code = self.child.wait().unwrap();
        let mut err = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut err)
            .unwrap();
        use std::os::unix::process::ExitStatusExt;
        (
            rest,
            code.code().or(code.signal().map(|s| 128 + s)).unwrap(),
            err,
        )
    }

    fn signal(&self, name: &str) {
        let ok = poimen_core::gui_path::command("kill")
            .arg(format!("-{name}"))
            .arg(self.child.id().to_string())
            .status()
            .unwrap()
            .success();
        assert!(ok, "kill -{name}");
    }
}

fn terminal_ids(f: &Fixture) -> Vec<String> {
    f.client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .map(|t| t.id)
        .collect()
}

/// Waits for the server to drop the terminal (closed), by its event stream.
fn wait_closed(f: &Fixture, terminal: &str) {
    let stream = f.client.subscribe().unwrap();
    if !terminal_ids(f).iter().any(|t| t == terminal) {
        return;
    }
    for event in stream {
        if matches!(event.unwrap(), poimen_client::protocol::Event::TerminalClosed { terminal_id, .. } if terminal_id == terminal)
        {
            return;
        }
    }
}

#[test]
fn a_session_prints_started_each_status_and_ended_then_exits_0() {
    let f = fixture("done");
    f.start_reporter();
    let p = f.run(&[
        "--prompt",
        NASTY_PROMPT,
        "--run",
        "run-7",
        "--task",
        "task-9",
        "--env",
        "FOO=bar baz",
        "--name",
        "my tab",
    ]);
    let started = p.record();
    assert_eq!(started["type"], "started");
    assert_eq!(started["space"], f.space.as_str());
    let terminal = started["terminal"].as_str().unwrap().to_owned();

    // The prompt arrived as one argument, with the run and the env in the environment.
    assert_eq!(
        std::fs::read_to_string(&f.started).unwrap(),
        format!("{NASTY_PROMPT}|run-7|bar baz")
    );
    assert!(!f.dir.join("work").join("pwned").exists());
    let t = f
        .client
        .list_terminals(None)
        .unwrap()
        .into_iter()
        .find(|t| t.id == terminal)
        .unwrap();
    assert_eq!(t.run.as_deref(), Some("run-7"));
    assert_eq!(t.task.as_deref(), Some("task-9"));
    assert_eq!(t.name, "my tab");

    report(&f.client, &terminal, 2, AgentStatus::Working);
    assert_eq!(p.record(), json!({"type": "status", "status": "working"}));
    report(&f.client, &terminal, 3, AgentStatus::Blocked);
    assert_eq!(p.record(), json!({"type": "status", "status": "blocked"}));
    report(&f.client, &terminal, 4, AgentStatus::Working);
    assert_eq!(p.record(), json!({"type": "status", "status": "working"}));
    report(&f.client, &terminal, 5, AgentStatus::Done);
    let (rest, code, err) = p.finish();
    assert_eq!(
        rest,
        vec![
            json!({"type": "status", "status": "done"}),
            json!({"type": "ended", "status": "done", "terminal": terminal}),
        ]
    );
    assert_eq!(code, 0, "{err}");
    // The terminal stays for the user.
    assert!(terminal_ids(&f).contains(&terminal));
    assert!(!f.exited.exists());
}

#[test]
fn an_error_ends_with_4_and_gone_with_4() {
    let f = fixture("error");
    f.start_reporter();
    let p = f.run(&["--prompt", "go"]);
    let terminal = p.record()["terminal"].as_str().unwrap().to_owned();
    report(&f.client, &terminal, 2, AgentStatus::Error);
    let (rest, code, _) = p.finish();
    assert_eq!(
        rest.last().unwrap(),
        &json!({"type": "ended", "status": "error", "terminal": terminal})
    );
    assert_eq!(code, 4);

    let f = fixture("gone");
    f.start_reporter();
    let p = f.run(&["--prompt", "go"]);
    let terminal = p.record()["terminal"].as_str().unwrap().to_owned();
    // Once a status arrives the wait is subscribed, so the close is seen as an event.
    report(&f.client, &terminal, 2, AgentStatus::Working);
    assert_eq!(p.record()["status"], "working");
    f.client.close_terminal(&terminal).unwrap();
    let (rest, code, _) = p.finish();
    assert_eq!(
        rest.last().unwrap(),
        &json!({"type": "ended", "status": "gone", "terminal": terminal})
    );
    assert_eq!(code, 4);
}

#[test]
fn start_failures_exit_3_with_the_reason_on_stderr_and_nothing_on_stdout() {
    let f = fixture("start");
    for (extra, needle) in [
        (
            &["--harness", "nope", "--prompt", "go"][..],
            "unknown harness",
        ),
        (&["--harness", "other", "--prompt", "go"][..], "no hooks"),
        (
            &["--prompt", "go", "--space", "no-such-space"][..],
            "not found",
        ),
    ] {
        // The later `--harness` of the case replaces the fixture's default one.
        let p = f.run(extra);
        let (rest, code, err) = p.finish();
        assert!(rest.is_empty(), "{rest:?}");
        assert_eq!(code, 3, "{err}");
        assert!(err.contains(needle), "{err}");
    }
}

#[test]
fn a_space_other_than_the_callers_is_refused_with_6_without_the_app() {
    let f = fixture("refused");
    let p = f.run(&["--prompt", "go", "--space", &f.other_space]);
    let (rest, code, err) = p.finish();
    assert!(rest.is_empty(), "{rest:?}");
    assert_eq!(code, 6, "{err}");
    assert!(err.contains("refused"), "{err}");
    // Nothing was started.
    assert_eq!(terminal_ids(&f), vec![f.caller.clone()]);
}

#[test]
fn a_lost_server_exits_5() {
    let f = fixture("lost");
    let p = f.poimen(&f.dir.join("missing.sock"), &["--prompt", "go"]);
    let (rest, code, err) = p.finish();
    assert!(rest.is_empty(), "{rest:?}");
    assert_eq!(code, 5, "{err}");
}

#[test]
fn bad_arguments_exit_2_with_the_usage() {
    let f = fixture("usage");
    for args in [&["--prompt", "go", "--bogus"][..], &[]] {
        let p = f.run(args);
        let (rest, code, err) = p.finish();
        assert!(rest.is_empty());
        assert_eq!(code, 2, "{err}");
        assert!(err.contains("usage: poimen"), "{err}");
    }
}

#[test]
fn sigint_and_sigterm_end_the_wait_and_leave_the_terminal_running() {
    for (signal, expected) in [("INT", 130), ("TERM", 143)] {
        let f = fixture(&format!("sig{signal}"));
        f.start_reporter();
        let p = f.run(&["--prompt", "go"]);
        let terminal = p.record()["terminal"].as_str().unwrap().to_owned();
        p.signal(signal);
        let (rest, code, _) = p.finish();
        assert!(rest.is_empty(), "no ended record: {rest:?}");
        assert_eq!(code, expected);
        assert!(terminal_ids(&f).contains(&terminal));
        assert!(!f.exited.exists(), "the agent was not told to exit");
    }
}

#[test]
fn close_on_end_exits_the_agent_and_closes_the_terminal_on_a_signal_and_at_the_end() {
    let f = fixture("closesig");
    f.start_reporter();
    let p = f.run(&["--prompt", "go", "--close-on-end"]);
    let terminal = p.record()["terminal"].as_str().unwrap().to_owned();
    p.signal("TERM");
    let (_, code, _) = p.finish();
    assert_eq!(code, 143);
    wait_closed(&f, &terminal);
    assert_eq!(std::fs::read_to_string(&f.exited).unwrap(), "bye");

    let f = fixture("closeend");
    f.start_reporter();
    let p = f.run(&["--prompt", "go", "--close-on-end"]);
    let terminal = p.record()["terminal"].as_str().unwrap().to_owned();
    report(&f.client, &terminal, 2, AgentStatus::Done);
    let (rest, code, _) = p.finish();
    assert_eq!(code, 0);
    assert_eq!(rest.last().unwrap()["type"], "ended");
    wait_closed(&f, &terminal);
    assert_eq!(std::fs::read_to_string(&f.exited).unwrap(), "bye");
}
