//! The process tree of a terminal ends with it (#968): every process of the child's session on
//! Unix, the job of the child on Windows. Runs the real binary.

#![allow(clippy::disallowed_methods)] // Test processes of a test run: no GUI app, no window to hide.
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use poimen_client::transport::Conn;
use poimen_protocol::host::{
    ClientMessage, HostMessage, HostStart, SpawnSpec, TreeEnd, parse_host_line, to_line,
};

const WAIT: Duration = Duration::from_secs(25);

/// A host with a client attached; killed and cleaned up on drop.
struct Terminal {
    dir: PathBuf,
    process: Child,
    conn: Conn,
    messages: Receiver<HostMessage>,
    output: String,
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Terminal {
    fn start(name: &str, argv: &[&str]) -> Terminal {
        let dir = std::env::temp_dir().join(format!("pt-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let endpoint = dir.join("h.sock");
        let start = HostStart {
            endpoint: endpoint.clone(),
            spawn: SpawnSpec {
                argv: argv.iter().map(|a| (*a).to_string()).collect(),
                cwd: dir.display().to_string(),
                env: Default::default(),
                cols: 80,
                rows: 24,
            },
            orphan_timeout_ms: 120_000,
            ring_bytes: 1 << 20,
            exit_linger_ms: 30_000,
        };
        let mut process = Command::new(env!("CARGO_BIN_EXE_poimen-pty-host"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut stdin = process.stdin.take().unwrap();
        stdin.write_all(&to_line(&start)).unwrap();
        drop(stdin);
        let mut hello = String::new();
        BufReader::new(process.stdout.take().unwrap())
            .read_line(&mut hello)
            .unwrap();
        parse_host_line(&hello).expect("hello");
        let conn = connect(&endpoint);
        let (tx, messages) = mpsc::channel();
        let reader = BufReader::new(conn.clone());
        thread::spawn(move || {
            for line in reader.lines().map_while(Result::ok) {
                if let Ok(message) = parse_host_line(&line)
                    && tx.send(message).is_err()
                {
                    break;
                }
            }
        });
        let terminal = Terminal {
            dir,
            process,
            conn,
            messages,
            output: String::new(),
        };
        terminal.send(&ClientMessage::Attach { since: 0 });
        terminal
    }

    fn send(&self, message: &ClientMessage) {
        (&self.conn).write_all(&to_line(message)).unwrap();
    }

    /// Reads until the output holds `text`.
    fn output_until(&mut self, text: &str) {
        let deadline = Instant::now() + WAIT;
        while !self.output.contains(text) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.messages.recv_timeout(left) {
                Ok(HostMessage::Output { bytes, .. }) => {
                    let data = poimen_protocol::decode_bytes(&bytes).unwrap();
                    self.output.push_str(&String::from_utf8_lossy(&data));
                }
                Ok(_) => {}
                Err(e) => panic!("no {text:?} in the output ({e}): {:?}", self.output),
            }
        }
    }

    /// The `tree` of the `exit` record.
    fn exit_tree(&mut self) -> Option<TreeEnd> {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.messages.recv_timeout(left) {
                Ok(HostMessage::Exit { tree, .. }) => return tree,
                Ok(_) => {}
                Err(e) => panic!("no exit record: {e}"),
            }
        }
    }

    /// The number after `label=` in the output.
    fn pid(&self, label: &str) -> u32 {
        let start = self.output.find(&format!("{label}=")).unwrap() + label.len() + 1;
        let digits: String = self.output[start..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits.parse().unwrap()
    }
}

/// The host binds its endpoint before it answers `hello`, so one connect is enough.
fn connect(endpoint: &Path) -> Conn {
    Conn::connect(endpoint).unwrap_or_else(|e| panic!("cannot connect: {e}"))
}

fn alive(pid: u32) -> bool {
    if cfg!(windows) {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).contains(&format!("\"{pid}\""))
    } else {
        // A zombie is gone for this purpose (its parent may be slow to reap it).
        let state = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok();
        match state {
            Some(s) => !s
                .rsplit_once(')')
                .is_some_and(|(_, r)| r.trim_start().starts_with('Z')),
            None => Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success(),
        }
    }
}

/// `Ended` is the host's own confirmation that the tree is empty, so one look is enough.
fn gone(pid: u32) -> bool {
    !alive(pid)
}

#[cfg(unix)]
mod unix {
    use super::*;

    /// A shell with job control, so the background job gets a process group of its own.
    const JOB: &str = "set -m; sleep 300 & echo gc=$!; echo sh=$$; echo ready; wait";

    #[test]
    fn kill_ends_a_background_job_in_its_own_group() {
        let mut t = Terminal::start("job", &["sh", "-c", JOB]);
        t.output_until("ready");
        let (shell, job) = (t.pid("sh"), t.pid("gc"));
        t.send(&ClientMessage::Kill);
        assert_eq!(t.exit_tree(), Some(TreeEnd::Ended));
        assert!(gone(job), "job {job} runs");
        assert!(gone(shell), "shell {shell} runs");
    }

    #[test]
    fn the_child_ending_by_itself_takes_its_background_jobs_with_it() {
        let mut t = Terminal::start(
            "exit",
            &[
                "sh",
                "-c",
                "sleep 300 & echo gc=$!; echo ready; sleep 1; exit 0",
            ],
        );
        t.output_until("ready");
        let job = t.pid("gc");
        assert_eq!(t.exit_tree(), Some(TreeEnd::Ended));
        assert!(gone(job), "job {job} runs");
    }

    #[test]
    fn a_job_that_ignores_the_hangup_is_killed_after_the_grace_time() {
        let mut t = Terminal::start(
            "ignore",
            &[
                "sh",
                "-c",
                "trap '' HUP; sleep 300 & echo gc=$!; echo ready; wait",
            ],
        );
        t.output_until("ready");
        let job = t.pid("gc");
        t.send(&ClientMessage::Kill);
        assert_eq!(t.exit_tree(), Some(TreeEnd::Ended));
        assert!(gone(job), "job {job} runs");
    }

    #[test]
    fn a_process_in_another_session_is_left_alone() {
        if Command::new("setsid").arg("--version").output().is_err() {
            eprintln!("skipped: no setsid");
            return;
        }
        let mut t = Terminal::start(
            "setsid",
            &[
                "sh",
                "-c",
                "setsid sleep 300 & echo gc=$!; sleep 1; echo ready; wait",
            ],
        );
        t.output_until("ready");
        let daemon = t.pid("gc");
        t.send(&ClientMessage::Kill);
        assert_eq!(t.exit_tree(), Some(TreeEnd::Ended));
        let survived = alive(daemon);
        // End it ourselves before asserting.
        let _ = Command::new("kill").arg(daemon.to_string()).status();
        assert!(survived, "the process in its own session was signalled");
    }
}

#[cfg(windows)]
mod windows {
    use super::*;

    /// `powershell` starts a grandchild and prints its pid; the job ends both.
    #[test]
    fn kill_ends_a_grandchild_through_the_job() {
        let script = "$p = Start-Process ping -ArgumentList '-n','600','127.0.0.1' -PassThru \
                      -WindowStyle Hidden; Write-Output \"gc=$($p.Id)\"; Write-Output ready; \
                      Start-Sleep 600";
        let mut t = Terminal::start("job", &["powershell", "-NoProfile", "-Command", script]);
        t.output_until("ready");
        let grandchild = t.pid("gc");
        t.send(&ClientMessage::Kill);
        assert_eq!(t.exit_tree(), Some(TreeEnd::Ended));
        assert!(
            gone_within(grandchild, Duration::from_secs(5)),
            "grandchild {grandchild} runs"
        );
    }
}
