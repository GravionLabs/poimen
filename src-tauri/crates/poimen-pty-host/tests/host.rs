//! `poimen-pty-host` end to end: the binary is started with a scripted child (never a user
//! session), and a test client speaks the host protocol on its endpoint.
//!
//! The scripted children are `sh -c` on Unix; Windows runs the platform-free tests with `cmd.exe`
//! and `ping`, among them the ConPTY check of ADR 0009.

#![allow(clippy::disallowed_methods)] // Test processes of a test run: no GUI app, no window to hide.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::transport::Conn;

use poimen_protocol::host::{
    ClientMessage, HOST_VERSION, HostMessage, HostStart, Size, SpawnSpec, parse_host_line, to_line,
};

const WAIT: Duration = Duration::from_secs(20);

struct Options {
    orphan_timeout_ms: u64,
    ring_bytes: usize,
    exit_linger_ms: u64,
    env: BTreeMap<String, String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            orphan_timeout_ms: 60_000,
            ring_bytes: 1024 * 1024,
            exit_linger_ms: 10_000,
            env: BTreeMap::new(),
        }
    }
}

/// A running host process in a temp directory; killed and cleaned up on drop.
struct Fixture {
    dir: PathBuf,
    endpoint: PathBuf,
    process: Child,
    hello: HostMessage,
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn temp_dir(name: &str) -> PathBuf {
    // Short: a Unix socket path has a limit of about 100 bytes.
    let dir = std::env::temp_dir().join(format!("ph-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn args(argv: &[&str]) -> Vec<String> {
    argv.iter().map(|a| (*a).to_string()).collect()
}

/// Starts the binary with `start` on stdin; returns it with what it printed on stdout.
fn launch(start: &HostStart) -> (Child, Result<HostMessage, String>) {
    let mut process = Command::new(env!("CARGO_BIN_EXE_poimen-pty-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = process.stdin.take().unwrap();
    stdin.write_all(&to_line(start)).unwrap();
    drop(stdin);
    let stdout = process.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut line = String::new();
        let _ = BufReader::new(stdout).read_line(&mut line);
        let _ = tx.send(line);
    });
    let line = rx.recv_timeout(WAIT).expect("the host printed nothing");
    let hello = parse_host_line(&line).map_err(|e| format!("{e}: {line:?}"));
    (process, hello)
}

fn start_with(name: &str, argv: &[&str], options: Options) -> Fixture {
    let dir = temp_dir(name);
    let endpoint = dir.join("h.sock");
    let start = HostStart {
        endpoint: endpoint.clone(),
        spawn: SpawnSpec {
            argv: args(argv),
            cwd: dir.display().to_string(),
            env: options.env,
            cols: 80,
            rows: 24,
        },
        orphan_timeout_ms: options.orphan_timeout_ms,
        ring_bytes: options.ring_bytes,
        exit_linger_ms: options.exit_linger_ms,
    };
    let (process, hello) = launch(&start);
    Fixture {
        dir,
        endpoint,
        process,
        hello: hello.unwrap(),
    }
}

fn start(name: &str, argv: &[&str]) -> Fixture {
    start_with(name, argv, Options::default())
}

/// A child that prints a line now and then and runs for minutes.
fn ticking() -> Vec<&'static str> {
    if cfg!(windows) {
        vec!["ping", "-n", "600", "127.0.0.1"]
    } else {
        vec![
            "sh",
            "-c",
            "i=0; while :; do i=$((i+1)); echo tick$i; sleep 0.3; done",
        ]
    }
}

impl Fixture {
    fn connect(&self) -> TestClient {
        TestClient::connect(&self.endpoint)
    }

    fn child_pid(&self) -> u32 {
        match &self.hello {
            HostMessage::Hello { child_pid, .. } => child_pid.expect("child pid"),
            other => panic!("{other:?}"),
        }
    }

    /// Waits for the host process to end; its exit code.
    fn wait_end(&mut self, within: Duration) -> Option<i32> {
        let deadline = Instant::now() + within;
        while Instant::now() < deadline {
            if let Some(status) = self.process.try_wait().unwrap() {
                return Some(status.code().unwrap_or(-1));
            }
            thread::sleep(Duration::from_millis(50));
        }
        None
    }
}

/// A client on the host's endpoint. Reads happen on a helper thread that hands the reader back,
/// so no thread holds the connection between reads (on Windows a pipe closes only when every
/// handle is gone) and a test never hangs on a read.
struct TestClient {
    conn: Conn,
    reader: Option<BufReader<Conn>>,
}

impl TestClient {
    fn connect(endpoint: &Path) -> Self {
        let conn = Conn::connect(endpoint).unwrap();
        Self {
            reader: Some(BufReader::new(conn.clone())),
            conn,
        }
    }

    fn send(&self, message: &ClientMessage) {
        (&self.conn).write_all(&to_line(message)).unwrap();
    }

    fn input(&self, text: &str) {
        self.send(&ClientMessage::Input {
            bytes: poimen_protocol::encode_bytes(text.as_bytes()),
        });
    }

    /// Reads messages until `done` holds for one (included). Panics on end of stream or timeout.
    fn read_until(
        &mut self,
        mut done: impl FnMut(&HostMessage, &[HostMessage]) -> bool + Send + 'static,
    ) -> Vec<HostMessage> {
        let mut reader = self.reader.take().expect("a read is running");
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut seen = Vec::new();
            let mut line = String::new();
            let mut matched = false;
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let message = parse_host_line(&line).unwrap();
                seen.push(message.clone());
                if done(&message, &seen) {
                    matched = true;
                    break;
                }
            }
            let _ = tx.send((reader, seen, matched));
        });
        let (reader, seen, matched) = rx.recv_timeout(WAIT).expect("no matching message in time");
        assert!(matched, "stream ended before the match: {seen:?}");
        self.reader = Some(reader);
        seen
    }

    /// Reads until the decoded output contains `text`.
    #[cfg(unix)]
    fn output_until(&mut self, text: &'static str) -> Vec<HostMessage> {
        self.read_until(move |_, seen| output_text(seen).contains(text))
    }

    /// Reads to the end of the stream; what came before it.
    fn read_to_end(mut self) -> Vec<HostMessage> {
        let mut reader = self.reader.take().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut seen = Vec::new();
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 0) {
                seen.push(parse_host_line(&line).unwrap());
                line.clear();
            }
            let _ = tx.send(seen);
        });
        rx.recv_timeout(WAIT).expect("the stream did not end")
    }
}

#[cfg(unix)]
fn output_text(messages: &[HostMessage]) -> String {
    let mut bytes = Vec::new();
    for m in messages {
        if let HostMessage::Output { bytes: b, .. } = m {
            bytes.extend(poimen_protocol::decode_bytes(b).unwrap());
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

fn output_seqs(messages: &[HostMessage]) -> Vec<u64> {
    messages
        .iter()
        .filter_map(|m| match m {
            HostMessage::Output { seq, .. } => Some(*seq),
            _ => None,
        })
        .collect()
}

fn hello_of(messages: &[HostMessage]) -> (u64, Size, Option<u32>) {
    match messages.first() {
        Some(HostMessage::Hello {
            seq,
            size,
            child_pid,
            ..
        }) => (*seq, *size, *child_pid),
        other => panic!("first message is not hello: {other:?}"),
    }
}

fn is_hello(m: &HostMessage) -> bool {
    matches!(m, HostMessage::Hello { .. })
}

fn is_exit(m: &HostMessage) -> bool {
    matches!(m, HostMessage::Exit { .. })
}

/// Whether a process with this pid still runs.
fn alive(pid: u32) -> bool {
    if cfg!(windows) {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
    } else {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    }
}

fn gone_within(pid: u32, within: Duration) -> Option<Duration> {
    let started = Instant::now();
    while started.elapsed() < within {
        if !alive(pid) {
            return Some(started.elapsed());
        }
        thread::sleep(Duration::from_millis(100));
    }
    None
}

#[test]
fn hello_is_printed_on_stdout_and_sent_first_on_every_connection() {
    let f = start("hello", &ticking());
    match &f.hello {
        HostMessage::Hello {
            host_version,
            version,
            pid,
            child_pid,
            size,
            endpoint,
            ..
        } => {
            assert_eq!(*host_version, HOST_VERSION);
            assert_eq!(version, env!("CARGO_PKG_VERSION"));
            assert_eq!(*pid, f.process.id());
            assert!(child_pid.is_some_and(|c| c != f.process.id()));
            assert_eq!(*size, Size { cols: 80, rows: 24 });
            assert_eq!(endpoint, &f.endpoint);
        }
        other => panic!("{other:?}"),
    }
    let mut client = f.connect();
    let first = client.read_until(|m, _| is_hello(m));
    let (_, size, child_pid) = hello_of(&first);
    assert_eq!(size, Size { cols: 80, rows: 24 });
    assert_eq!(child_pid, Some(f.child_pid()));
}

#[test]
fn version_and_usage_errors() {
    let bin = env!("CARGO_BIN_EXE_poimen-pty-host");
    let out = Command::new(bin).arg("--version").output().unwrap();
    assert!(out.status.success());
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        format!(
            "poimen-pty-host {} (host protocol 1)",
            env!("CARGO_PKG_VERSION")
        )
    );
    let out = Command::new(bin).arg("--nope").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn a_bad_start_is_an_error_line_and_exit_code_1() {
    let dir = temp_dir("bad");
    let spec = |argv: &[&str], endpoint: PathBuf| HostStart {
        endpoint,
        spawn: SpawnSpec {
            argv: args(argv),
            cwd: dir.display().to_string(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        },
        orphan_timeout_ms: 1000,
        ring_bytes: 1024,
        exit_linger_ms: 0,
    };
    let expect_error = |start: &HostStart, needle: &str| {
        let (mut process, line) = launch(start);
        match line {
            Ok(HostMessage::Error { message }) => {
                assert!(message.contains(needle), "{message}");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(process.wait().unwrap().code(), Some(1));
    };

    expect_error(&spec(&[], dir.join("a.sock")), "argv is empty");
    // A program that does not exist: the endpoint is removed again.
    let endpoint = dir.join("b.sock");
    expect_error(
        &spec(&["poimen-no-such-program-602"], endpoint.clone()),
        "could not start",
    );
    assert!(!endpoint.exists());
    // A directory that does not exist cannot hold the endpoint.
    expect_error(
        &spec(&ticking(), dir.join("missing").join("c.sock")),
        "could not bind",
    );

    // Not a start line at all.
    let mut process = Command::new(env!("CARGO_BIN_EXE_poimen-pty-host"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    process.stdin.take().unwrap().write_all(b"{}\n").unwrap();
    let out = process.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        matches!(
            parse_host_line(&String::from_utf8_lossy(&out.stdout)),
            Ok(HostMessage::Error { .. })
        ),
        "{out:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_endpoint_in_use_is_refused() {
    let f = start("inuse", &ticking());
    let start = HostStart {
        endpoint: f.endpoint.clone(),
        spawn: SpawnSpec {
            argv: args(&ticking()),
            cwd: f.dir.display().to_string(),
            env: BTreeMap::new(),
            cols: 80,
            rows: 24,
        },
        orphan_timeout_ms: 1000,
        ring_bytes: 1024,
        exit_linger_ms: 0,
    };
    let (mut process, line) = launch(&start);
    assert!(
        matches!(&line, Ok(HostMessage::Error { message }) if message.contains("could not bind")),
        "{line:?}"
    );
    assert_eq!(process.wait().unwrap().code(), Some(1));
    // The first host still answers.
    let mut client = f.connect();
    client.read_until(|m, _| is_hello(m));
}

#[test]
fn the_exit_code_is_reported_and_the_host_ends_with_its_child() {
    let argv: &[&str] = if cfg!(windows) {
        &["cmd.exe", "/d", "/c", "exit", "7"]
    } else {
        &["sh", "-c", "read x; exit 7"]
    };
    let mut f = start("exit", argv);
    let mut client = f.connect();
    client.read_until(|m, _| is_hello(m));
    client.send(&ClientMessage::Attach { since: 0 });
    if cfg!(unix) {
        client.input("\r");
    }
    let seen = client.read_until(|m, _| is_exit(m));
    assert_eq!(seen.last(), Some(&HostMessage::Exit { code: Some(7) }));
    // Nothing follows `exit`: the connection ends with the host.
    assert_eq!(client.read_to_end(), vec![]);
    assert_eq!(f.wait_end(WAIT), Some(0));
    assert!(!f.endpoint.exists(), "the endpoint stays after the end");
}

#[cfg(unix)]
#[test]
fn input_reaches_the_child_in_its_directory_and_environment() {
    let mut f = start_with(
        "echo",
        &[
            "sh",
            "-c",
            "read line; echo \"got:$line v=$POIMEN_X\"; pwd; exit 3",
        ],
        Options {
            env: BTreeMap::from([("POIMEN_X".into(), "42".into())]),
            ..Options::default()
        },
    );
    let dir = f.dir.canonicalize().unwrap();
    let mut client = f.connect();
    client.read_until(|m, _| is_hello(m));
    client.send(&ClientMessage::Attach { since: 0 });
    client.input("hello\r");
    let seen = client.read_until(|m, _| is_exit(m));
    let text = output_text(&seen);
    assert!(text.contains("got:hello v=42"), "{text:?}");
    assert!(text.contains(&dir.display().to_string()), "{text:?}");
    assert_eq!(seen.last(), Some(&HostMessage::Exit { code: Some(3) }));
    // Every record is one more than the one before.
    let seqs = output_seqs(&seen);
    assert!(!seqs.is_empty());
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
    assert_eq!(f.wait_end(WAIT), Some(0));
}

#[cfg(unix)]
#[test]
fn resize_reaches_the_pty_and_the_next_hello() {
    let f = start("resize", &["sh", "-c", "read x; stty size; sleep 30"]);
    let mut client = f.connect();
    client.read_until(|m, _| is_hello(m));
    client.send(&ClientMessage::Attach { since: 0 });
    client.send(&ClientMessage::Resize {
        cols: 100,
        rows: 30,
    });
    // `stty size` runs after the input, so after the resize.
    thread::sleep(Duration::from_millis(200));
    client.input("\r");
    client.output_until("30 100");
    let mut second = f.connect();
    let hello = second.read_until(|m, _| is_hello(m));
    assert_eq!(
        hello_of(&hello).1,
        Size {
            cols: 100,
            rows: 30
        }
    );
}

#[test]
fn bad_lines_get_an_error_and_the_connection_stays() {
    let f = start("bad-lines", &ticking());
    let mut client = f.connect();
    client.read_until(|m, _| is_hello(m));
    (&client.conn).write_all(b"not json\n").unwrap();
    let seen = client.read_until(|m, _| matches!(m, HostMessage::Error { .. }));
    assert!(
        matches!(seen.last(), Some(HostMessage::Error { message }) if message.contains("invalid line"))
    );
    (&client.conn).write_all(b"{\"type\":\"pause\"}\n").unwrap();
    let seen = client.read_until(|m, _| matches!(m, HostMessage::Error { .. }));
    assert!(
        matches!(seen.last(), Some(HostMessage::Error { message }) if message.contains("unknown message type"))
    );
    client.send(&ClientMessage::Input {
        bytes: "***".into(),
    });
    client.read_until(|m, _| matches!(m, HostMessage::Error { .. }));
    client.send(&ClientMessage::Resize { cols: 0, rows: 5 });
    client.read_until(|m, _| matches!(m, HostMessage::Error { .. }));
    // Still served: an attach starts the stream, a second one is refused.
    client.send(&ClientMessage::Attach { since: 0 });
    client.send(&ClientMessage::Attach { since: 0 });
    let seen = client.read_until(|m, _| matches!(m, HostMessage::Error { .. }));
    assert!(
        matches!(seen.last(), Some(HostMessage::Error { message }) if message == "already attached")
    );
}

#[cfg(unix)]
#[test]
fn a_since_ahead_of_the_host_is_refused() {
    let f = start("ahead", &["sh", "-c", "sleep 30"]);
    let mut client = f.connect();
    client.read_until(|m, _| is_hello(m));
    client.send(&ClientMessage::Attach { since: 99 });
    let seen = client.read_until(|m, _| matches!(m, HostMessage::Error { .. }));
    assert!(
        matches!(seen.last(), Some(HostMessage::Error { message }) if message.contains("ahead")),
        "{seen:?}"
    );
}

#[test]
fn a_second_connection_replaces_the_first() {
    let f = start("replace", &ticking());
    let mut first = f.connect();
    first.read_until(|m, _| is_hello(m));
    first.send(&ClientMessage::Attach { since: 0 });
    let mut second = f.connect();
    second.read_until(|m, _| is_hello(m));
    first.read_until(|m, _| *m == HostMessage::Replaced);
    // The new client gets the stream.
    second.send(&ClientMessage::Attach { since: 0 });
    second.read_until(|m, _| matches!(m, HostMessage::Output { .. }));
}

#[cfg(unix)]
#[test]
fn a_reconnecting_client_gets_the_rest_of_the_ring_after_since() {
    let f = start(
        "since",
        &["sh", "-c", "echo one; read x; echo two; sleep 30"],
    );
    let mut first = f.connect();
    first.read_until(|m, _| is_hello(m));
    first.send(&ClientMessage::Attach { since: 0 });
    let seen = first.output_until("one");
    let last = *output_seqs(&seen).last().unwrap();
    // Output while no client is attached goes to the ring.
    first.input("\r");
    drop(first);
    thread::sleep(Duration::from_millis(500));

    let mut second = f.connect();
    let hello = second.read_until(|m, _| is_hello(m));
    assert!(hello_of(&hello).0 > last, "{hello:?}");
    second.send(&ClientMessage::Attach { since: last });
    let seen = second.output_until("two");
    assert!(!seen.iter().any(|m| matches!(m, HostMessage::Gap { .. })));
    assert!(!output_text(&seen).contains("one"), "{seen:?}");
    let seqs = output_seqs(&seen);
    assert_eq!(seqs[0], last + 1);
    assert!(seqs.windows(2).all(|w| w[1] == w[0] + 1), "{seqs:?}");
}

#[cfg(unix)]
#[test]
fn a_client_behind_the_ring_gets_a_gap_marker_first() {
    let f = start_with(
        "gap",
        &[
            "sh",
            "-c",
            "printf aaaaaaaa; sleep 0.2; printf bbbbbbbb; sleep 0.2; printf cccccccc; sleep 30",
        ],
        Options {
            ring_bytes: 8,
            ..Options::default()
        },
    );
    thread::sleep(Duration::from_millis(1200));
    let mut client = f.connect();
    let hello = client.read_until(|m, _| is_hello(m));
    let last = hello_of(&hello).0;
    assert!(last >= 3, "{hello:?}");
    client.send(&ClientMessage::Attach { since: 0 });
    let seen =
        client.read_until(move |m, _| matches!(m, HostMessage::Output { seq, .. } if *seq == last));
    assert_eq!(
        seen[0],
        HostMessage::Gap {
            from: 1,
            to: last - 1
        }
    );
    assert_eq!(output_seqs(&seen), vec![last]);
    assert_eq!(output_text(&seen), "cccccccc");
}

#[cfg(unix)]
#[test]
fn an_exit_without_a_client_waits_for_one_to_collect_it() {
    let mut f = start_with(
        "linger",
        &["sh", "-c", "echo bye; exit 4"],
        Options {
            exit_linger_ms: 10_000,
            ..Options::default()
        },
    );
    thread::sleep(Duration::from_millis(1500));
    assert_eq!(
        f.process.try_wait().unwrap(),
        None,
        "ended before the linger"
    );
    let mut client = f.connect();
    client.read_until(|m, _| is_hello(m));
    client.send(&ClientMessage::Attach { since: 0 });
    let seen = client.read_until(|m, _| is_exit(m));
    assert!(output_text(&seen).contains("bye"));
    assert_eq!(seen.last(), Some(&HostMessage::Exit { code: Some(4) }));
    assert_eq!(f.wait_end(WAIT), Some(0));
}

#[cfg(unix)]
#[test]
fn an_exit_nobody_collects_ends_the_host_after_the_linger() {
    let mut f = start_with(
        "linger-end",
        &["sh", "-c", "exit 0"],
        Options {
            exit_linger_ms: 200,
            ..Options::default()
        },
    );
    assert_eq!(f.wait_end(Duration::from_secs(10)), Some(0));
    assert!(!f.endpoint.exists());
}

#[test]
fn kill_ends_the_child_and_the_host() {
    let mut f = start("kill", &ticking());
    let child = f.child_pid();
    let mut client = f.connect();
    client.read_until(|m, _| is_hello(m));
    client.send(&ClientMessage::Attach { since: 0 });
    client.send(&ClientMessage::Kill);
    client.read_until(|m, _| is_exit(m));
    assert_eq!(f.wait_end(WAIT), Some(0));
    assert!(
        gone_within(child, WAIT).is_some(),
        "child {child} still runs"
    );
    assert!(!f.endpoint.exists());
}

#[test]
fn without_any_client_the_host_ends_after_the_orphan_timeout() {
    let mut f = start_with(
        "orphan",
        &ticking(),
        Options {
            orphan_timeout_ms: 300,
            exit_linger_ms: 60_000,
            ..Options::default()
        },
    );
    let child = f.child_pid();
    assert_eq!(f.wait_end(WAIT), Some(0));
    assert!(
        gone_within(child, WAIT).is_some(),
        "child {child} still runs"
    );
}

#[test]
fn a_connected_client_holds_off_the_orphan_timeout() {
    let mut f = start_with(
        "orphan-held",
        &ticking(),
        Options {
            orphan_timeout_ms: 1000,
            ..Options::default()
        },
    );
    let mut client = f.connect();
    client.read_until(|m, _| is_hello(m));
    thread::sleep(Duration::from_millis(2500));
    assert_eq!(f.process.try_wait().unwrap(), None, "ended with a client");
    drop(client);
    assert_eq!(f.wait_end(WAIT), Some(0));
}

/// ADR 0009, open point (Windows: ConPTY): the child of a host keeps running while its client
/// disconnects and reconnects, and ends when the host itself ends (killed, without a chance to
/// close the pseudoconsole). Runs on every platform; the Windows result comes from the CI
/// `windows` job and is recorded in ADR 0009.
#[test]
fn the_child_outlives_its_clients_but_not_its_host() {
    let mut f = start("conpty", &ticking());
    let child = f.child_pid();

    let mut first = f.connect();
    first.read_until(|m, _| is_hello(m));
    first.send(&ClientMessage::Attach { since: 0 });
    let seen = first.read_until(|m, _| matches!(m, HostMessage::Output { .. }));
    let last = *output_seqs(&seen).last().unwrap();
    drop(first);

    // No client for a while: the child keeps running and printing into the ring.
    thread::sleep(Duration::from_secs(3));
    assert!(alive(child), "child {child} ended without a client");
    let mut second = f.connect();
    let hello = second.read_until(|m, _| is_hello(m));
    let (seq, _, child_pid) = hello_of(&hello);
    assert_eq!(child_pid, Some(child));
    assert!(seq > last, "no output while disconnected: {seq} <= {last}");
    second.send(&ClientMessage::Attach { since: last });
    let seen = second.read_until(|m, _| matches!(m, HostMessage::Output { .. }));
    assert!(!seen.iter().any(|m| matches!(m, HostMessage::Gap { .. })));
    assert_eq!(output_seqs(&seen)[0], last + 1);
    drop(second);

    // The host itself ends abruptly (TerminateProcess on Windows, SIGKILL on Unix).
    f.process.kill().unwrap();
    f.process.wait().unwrap();
    let ended = gone_within(child, Duration::from_secs(30));
    eprintln!("conpty check: child of a killed host ended after {ended:?}");
    assert!(
        ended.is_some(),
        "child {child} still runs 30 s after its host was killed"
    );
}
