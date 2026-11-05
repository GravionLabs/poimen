//! One PTY with its child process (`portable-pty`).

use std::io::{Read, Write};
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use poimen_protocol::{ENV_SOCKET, ENV_TERMINAL_ID};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub(crate) struct SpawnSpec<'a> {
    pub terminal_id: &'a str,
    pub socket: &'a Path,
    pub command: &'a [String],
    pub cwd: &'a str,
    pub cols: u16,
    pub rows: u16,
}

pub(crate) struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: PtyWriter,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

/// A shared handle on the input side of a PTY, so a blocking write can happen without holding
/// the server's global lock.
#[derive(Clone)]
pub(crate) struct PtyWriter(std::sync::Arc<std::sync::Mutex<Box<dyn Write + Send>>>);

impl PtyWriter {
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(writer)))
    }

    pub fn write(&self, data: &[u8]) -> Result<(), String> {
        let mut w = self.0.lock().unwrap_or_else(|e| e.into_inner());
        w.write_all(data)
            .and_then(|()| w.flush())
            .map_err(|e| e.to_string())
    }
}

impl Pty {
    pub fn writer(&self) -> PtyWriter {
        self.writer.clone()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        self.master
            .resize(size(cols, rows))
            .map_err(|e| e.to_string())
    }

    /// Ends the child (SIGHUP); the master closes when the `Pty` is dropped.
    pub fn kill(mut self) {
        let _ = self.killer.kill();
    }
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// `$SHELL`, else `/bin/sh`.
pub(crate) fn default_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/sh".into())
}

/// The file name of a program path, for the default terminal name.
pub(crate) fn program_name(program: &str) -> String {
    Path::new(program)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(program)
        .to_string()
}

/// Output is passed on in batches: what arrives within this time, up to [`MAX_BATCH`] bytes.
const COALESCE: Duration = Duration::from_millis(4);
pub(crate) const MAX_BATCH: usize = 32 * 1024;
/// How long the exit report waits for the output of the dead process to be handled.
const DRAIN_WAIT: Duration = Duration::from_secs(1);

enum Chunk {
    Data(Vec<u8>),
    Eof,
}

/// Starts the command in a new PTY. A reader thread drains the PTY (a chatty child only blocks
/// when `on_output` is slow), a second thread coalesces what it reads and calls `on_output`
/// with batches of at most [`MAX_BATCH`] bytes. `on_exit` runs on a helper thread when the
/// process ends, with its exit code, after the output has been handled.
pub(crate) fn spawn(
    spec: &SpawnSpec<'_>,
    mut on_output: impl FnMut(&[u8]) + Send + 'static,
    on_exit: impl FnOnce(Option<u32>) + Send + 'static,
) -> Result<Pty, String> {
    let pair = native_pty_system()
        .openpty(size(spec.cols, spec.rows))
        .map_err(|e| e.to_string())?;
    let mut cmd = CommandBuilder::new(&spec.command[0]);
    cmd.args(&spec.command[1..]);
    cmd.cwd(spec.cwd);
    if std::env::var_os("TERM").is_none_or(|t| t.is_empty() || t == "dumb") {
        cmd.env("TERM", "xterm-256color");
    }
    cmd.env(ENV_SOCKET, spec.socket);
    cmd.env(ENV_TERMINAL_ID, spec.terminal_id);

    let mut child = pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| format!("could not start {}: {e}", spec.command[0]))?;
    // Only the child holds the slave, so the reader sees EOF when it ends.
    drop(pair.slave);
    let killer = child.clone_killer();
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

    let (tx, rx) = mpsc::sync_channel::<Chunk>(32);
    let (drained_tx, drained_rx) = mpsc::channel::<()>();
    thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 || tx.send(Chunk::Data(buf[..n].to_vec())).is_err() {
                break;
            }
        }
        let _ = tx.send(Chunk::Eof);
    });
    thread::spawn(move || {
        while let Ok(Chunk::Data(mut batch)) = rx.recv() {
            let deadline = Instant::now() + COALESCE;
            let mut eof = false;
            while batch.len() < MAX_BATCH {
                let left = deadline.saturating_duration_since(Instant::now());
                match rx.recv_timeout(left) {
                    Ok(Chunk::Data(d)) => batch.extend_from_slice(&d),
                    Ok(Chunk::Eof) | Err(RecvTimeoutError::Disconnected) => {
                        eof = true;
                        break;
                    }
                    Err(RecvTimeoutError::Timeout) => break,
                }
            }
            on_output(&batch);
            if eof {
                break;
            }
        }
        let _ = drained_tx.send(());
    });
    thread::spawn(move || {
        let code = child.wait().ok().map(|status| status.exit_code());
        // A grandchild that keeps the PTY open must not delay the report for ever.
        let _ = drained_rx.recv_timeout(DRAIN_WAIT);
        on_exit(code);
    });

    Ok(Pty {
        master: pair.master,
        writer: PtyWriter::new(writer),
        killer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn program_name_is_the_file_name() {
        assert_eq!(program_name("/usr/bin/zsh"), "zsh");
        assert_eq!(program_name("sh"), "sh");
    }
}
