//! One ssh process per connection to a machine's `poimen-server stdio` (ADR 0008), shared by the
//! app and the console: `ssh -T -o BatchMode=yes ... <target> poimen-server stdio`.

use std::io::{self, BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};

use poimen_core::gui_path;
use poimen_core::machines::check_target;

use crate::pipe::{Connector, Pipe};

mod args;
pub mod mux;
pub use args::{ssh_args, ssh_args_with};

/// How much of ssh's stderr is kept for the error of a failed connection.
const MAX_SSH_ERROR: usize = 4096;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// `ssh -T -o BatchMode=yes ... <target> poimen-server stdio`: one ssh process per connection.
pub struct SshConnector {
    target: String,
    /// The server's command on the machine: its name, or the `serverPath` of the machine.
    server: String,
    /// The ssh program; `None` resolves `ssh` on the login shell's `PATH` at every connect.
    program: Option<PathBuf>,
}

impl SshConnector {
    pub fn new(target: &str, server_path: Option<&str>) -> Self {
        Self {
            target: target.to_owned(),
            server: server_path.map_or_else(
                || poimen_protocol::CHANNEL.named("poimen-server"),
                str::to_owned,
            ),
            program: None,
        }
    }
}

impl Connector for SshConnector {
    fn connect(&self) -> io::Result<Pipe> {
        // A target from an older config would still be refused here, never passed as an option.
        check_target(&self.target).map_err(io::Error::other)?;
        let program = match &self.program {
            Some(p) => p.clone(),
            None => gui_path::resolve_program("ssh", &gui_path::child_path())
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "ssh is not on the PATH"))?,
        };
        let mux = mux::for_target(&program, &self.target);
        let mut child = gui_path::command(&program)
            .args(ssh_args_with(&self.target, &self.server, &mux.options))
            .env("PATH", gui_path::child_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (Some(stdin), Some(stdout), Some(stderr)) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take())
        else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("ssh started without its pipes"));
        };
        let message = Arc::new(Mutex::new(String::new()));
        let collect = {
            let message = message.clone();
            thread::spawn(move || collect_stderr(stderr, &message))
        };
        let child = Arc::new(Mutex::new(child));
        let reader = SshOutput {
            stdout,
            stderr: Some(collect),
            message,
        };
        Ok(Pipe::new(reader, stdin, move || end(&child)))
    }
}

/// Keeps the first [`MAX_SSH_ERROR`] bytes of ssh's stderr, one line at a time.
fn collect_stderr(stderr: impl Read, message: &Mutex<String>) {
    for line in BufReader::new(stderr).lines() {
        let Ok(line) = line else { break };
        let mut message = lock(message);
        if message.len() + line.len() < MAX_SSH_ERROR {
            if !message.is_empty() {
                message.push('\n');
            }
            message.push_str(line.trim_end());
        }
    }
}

fn end(child: &Mutex<Child>) {
    let mut child = lock(child);
    let _ = child.kill();
    let _ = child.wait();
}

/// ssh's stdout; its end of file is the ssh error when ssh wrote one (`Permission denied`,
/// `Could not resolve hostname`, `poimen-server: command not found`).
struct SshOutput<R> {
    stdout: R,
    stderr: Option<JoinHandle<()>>,
    message: Arc<Mutex<String>>,
}

impl<R: Read> Read for SshOutput<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.stdout.read(buf)?;
        if n > 0 || buf.is_empty() {
            return Ok(n);
        }
        // ssh has closed its stdout, so it is ending: wait for the rest of its stderr.
        if let Some(stderr) = self.stderr.take() {
            let _ = stderr.join();
        }
        let message = lock(&self.message).trim().to_owned();
        if message.is_empty() {
            Ok(0)
        } else {
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                format!("ssh: {message}"),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- ssh's error on the end of its output -------------------------------------------------

    #[test]
    fn the_end_of_ssh_output_is_its_error_message() {
        let message = Arc::new(Mutex::new(String::new()));
        let collect = {
            let message = message.clone();
            thread::spawn(move || {
                collect_stderr(&b"me@box: Permission denied (publickey).\n"[..], &message)
            })
        };
        let mut out = SshOutput {
            stdout: &b"{}\n"[..],
            stderr: Some(collect),
            message,
        };
        let mut text = String::new();
        let err = out.read_to_string(&mut text).unwrap_err();
        assert_eq!(text, "{}\n");
        assert_eq!(
            err.to_string(),
            "ssh: me@box: Permission denied (publickey)."
        );
    }

    #[test]
    fn a_quiet_end_of_ssh_output_is_end_of_file() {
        let mut out = SshOutput {
            stdout: &b""[..],
            stderr: None,
            message: Arc::default(),
        };
        assert_eq!(out.read(&mut [0; 8]).unwrap(), 0);
    }
}
