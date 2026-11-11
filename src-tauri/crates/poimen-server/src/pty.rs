//! What the server does for a terminal's PTY that its `poimen-pty-host` does not (ADR 0009):
//! the default shell and terminal name, the input handle, and the batching of output before it
//! reaches the emulator. The PTY and the child live in the host (`host.rs`).

use std::io::Write;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

/// A shared handle on the input side of a terminal, so a blocking write can happen without
/// holding the server's global lock. Behind it is the terminal's host connection (`host.rs`).
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

/// Unix: `$SHELL`, else `/bin/sh`. Windows: `pwsh.exe` on `PATH`, else `powershell.exe` on
/// `PATH`, else `%COMSPEC%`, else `cmd.exe`.
pub(crate) fn default_shell() -> String {
    if cfg!(windows) {
        let dirs = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default();
        windows_shell(&dirs, std::env::var("COMSPEC").ok(), |p| p.is_file())
    } else {
        std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".into())
    }
}

/// The Windows [`default_shell`]: the full path of the first PowerShell found in `dirs`.
pub(crate) fn windows_shell(
    dirs: &[std::path::PathBuf],
    comspec: Option<String>,
    is_file: impl Fn(&Path) -> bool,
) -> String {
    ["pwsh.exe", "powershell.exe"]
        .iter()
        .find_map(|name| {
            dirs.iter()
                .filter(|d| !d.as_os_str().is_empty())
                .map(|d| d.join(name))
                .find(|p| is_file(p))
        })
        .map(|p| p.to_string_lossy().into_owned())
        .or_else(|| comspec.filter(|c| !c.is_empty()))
        .unwrap_or_else(|| "cmd.exe".into())
}

/// The file name of a program path, for the default terminal name (on Windows without `.exe`).
pub(crate) fn program_name(program: &str) -> String {
    program_name_for(program, cfg!(windows))
}

pub(crate) fn program_name_for(program: &str, windows: bool) -> String {
    if !windows {
        return Path::new(program)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(program)
            .to_string();
    }
    let name = program.rsplit(['/', '\\']).next().unwrap_or(program);
    match name.len().checked_sub(4) {
        Some(i) if i > 0 && name.is_char_boundary(i) && name[i..].eq_ignore_ascii_case(".exe") => {
            name[..i].to_string()
        }
        _ => name.to_string(),
    }
}

/// Output is passed on in batches: what arrives within this time, up to [`MAX_BATCH`] bytes.
const COALESCE: Duration = Duration::from_millis(4);
pub(crate) const MAX_BATCH: usize = 32 * 1024;

/// What the reader of a host hands to [`coalesce`].
pub(crate) enum Chunk {
    /// The bytes of an output record.
    Data(Vec<u8>),
    /// The terminal ended: its child exited with the code, or its host is gone (`None`).
    End(Option<u32>),
}

/// Calls `on_output` with batches of the records of `rx` (what arrives within [`COALESCE`], up
/// to [`MAX_BATCH`] bytes), in order. Returns the exit code of [`Chunk::End`] once everything before it was handed on (`None`
/// also when the sender went away).
pub(crate) fn coalesce(rx: &Receiver<Chunk>, mut on_output: impl FnMut(&[u8])) -> Option<u32> {
    let mut end: Option<Option<u32>> = None;
    while end.is_none() {
        let mut batch = match rx.recv() {
            Ok(Chunk::Data(data)) => data,
            Ok(Chunk::End(code)) => return code,
            Err(_) => return None,
        };
        let deadline = Instant::now() + COALESCE;
        while batch.len() < MAX_BATCH {
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(Chunk::Data(d)) => batch.extend_from_slice(&d),
                Ok(Chunk::End(code)) => {
                    end = Some(code);
                    break;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    end = Some(None);
                    break;
                }
                Err(RecvTimeoutError::Timeout) => break,
            }
        }
        on_output(&batch);
    }
    end.flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    /// Records every `write` call of the PTY input.
    #[derive(Clone, Default)]
    struct Calls(Arc<Mutex<Vec<Vec<u8>>>>);

    impl Write for Calls {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().push(buf.to_vec());
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_mouse_report_reaches_the_pty_in_one_write_and_unchanged() {
        let calls = Calls::default();
        let writer = PtyWriter::new(Box::new(calls.clone()));
        writer.write(b"\x1b[<0;12;5M").unwrap();
        writer.write(b"\x1b[<0;12;5m").unwrap();
        assert_eq!(
            *calls.0.lock().unwrap(),
            vec![b"\x1b[<0;12;5M".to_vec(), b"\x1b[<0;12;5m".to_vec()]
        );
    }

    fn fed(chunks: Vec<Chunk>) -> (Vec<Vec<u8>>, Option<u32>) {
        let (tx, rx) = std::sync::mpsc::channel();
        for chunk in chunks {
            tx.send(chunk).unwrap();
        }
        drop(tx);
        let mut batches = Vec::new();
        let code = coalesce(&rx, |b| batches.push(b.to_vec()));
        (batches, code)
    }

    #[test]
    fn queued_output_is_fed_in_one_batch_in_order_before_the_exit() {
        let (batches, code) = fed(vec![
            Chunk::Data(b"ab".to_vec()),
            Chunk::Data(b"cd".to_vec()),
            Chunk::End(Some(2)),
        ]);
        assert_eq!(batches, [b"abcd".to_vec()]);
        assert_eq!(code, Some(2));
    }

    #[test]
    fn a_batch_stops_at_max_batch_bytes() {
        let (batches, _) = fed(vec![
            Chunk::Data(vec![b'x'; MAX_BATCH]),
            Chunk::Data(b"y".to_vec()),
            Chunk::End(None),
        ]);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), MAX_BATCH);
        assert_eq!(batches[1], b"y");
    }

    #[test]
    fn without_output_the_end_comes_at_once_and_a_lost_sender_is_no_code() {
        assert_eq!(fed(vec![Chunk::End(Some(0))]), (vec![], Some(0)));
        assert_eq!(
            fed(vec![Chunk::Data(b"z".to_vec())]),
            (vec![b"z".to_vec()], None)
        );
        assert_eq!(fed(vec![]), (vec![], None));
    }

    #[test]
    fn program_name_is_the_file_name() {
        assert_eq!(program_name("/usr/bin/zsh"), "zsh");
        assert_eq!(program_name("sh"), "sh");
        assert_eq!(program_name_for("/opt/x.exe", false), "x.exe");
    }

    #[test]
    fn a_windows_program_name_drops_the_directory_and_exe() {
        assert_eq!(
            program_name_for(r"C:\Program Files\PowerShell\7\pwsh.exe", true),
            "pwsh"
        );
        assert_eq!(program_name_for("CMD.EXE", true), "CMD");
        assert_eq!(program_name_for("tool", true), "tool");
        assert_eq!(program_name_for(".exe", true), ".exe");
    }

    #[test]
    fn the_windows_shell_prefers_pwsh_then_powershell_then_comspec() {
        let dirs = [PathBuf::new(), PathBuf::from("win"), PathBuf::from("ps7")];
        let ps7 = PathBuf::from("ps7").join("pwsh.exe");
        let win = PathBuf::from("win").join("powershell.exe");
        let found = |files: Vec<PathBuf>| move |p: &Path| files.iter().any(|f| f == p);
        let comspec = || Some(r"C:\Windows\cmd.exe".to_string());
        assert_eq!(
            windows_shell(&dirs, comspec(), found(vec![win.clone(), ps7.clone()])),
            ps7.to_string_lossy()
        );
        assert_eq!(
            windows_shell(&dirs, comspec(), found(vec![win.clone()])),
            win.to_string_lossy()
        );
        assert_eq!(
            windows_shell(&dirs, comspec(), found(vec![])),
            r"C:\Windows\cmd.exe"
        );
        assert_eq!(
            windows_shell(&dirs, Some(String::new()), found(vec![])),
            "cmd.exe"
        );
        assert_eq!(windows_shell(&[], None, found(vec![])), "cmd.exe");
    }
}
