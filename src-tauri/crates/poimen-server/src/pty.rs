//! One PTY with its child process (`portable-pty`).

use std::io::{Read, Write};
use std::path::Path;
use std::thread;

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
    writer: Box<dyn Write + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

impl Pty {
    pub fn write(&mut self, data: &[u8]) -> Result<(), String> {
        self.writer
            .write_all(data)
            .and_then(|()| self.writer.flush())
            .map_err(|e| e.to_string())
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

/// Starts the command in a new PTY. `on_exit` runs on a helper thread when the process ends,
/// with its exit code. Output is drained and dropped (streaming comes with the next protocol
/// version), so a chatty child never blocks on a full PTY buffer.
pub(crate) fn spawn(
    spec: &SpawnSpec<'_>,
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

    thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while matches!(reader.read(&mut buf), Ok(n) if n > 0) {}
    });
    thread::spawn(move || {
        let code = child.wait().ok().map(|status| status.exit_code());
        on_exit(code);
    });

    Ok(Pty {
        master: pair.master,
        writer,
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
