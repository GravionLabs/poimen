//! Starting a host binary or an in-process host.

use super::*;

/// Creates the private `hosts` directory and removes a stale entry at the endpoint (a host of an
/// earlier server that ended); a host that still answers there is an error.
pub(super) fn prepare_endpoint(endpoint: &Path) -> Result<(), String> {
    if let Some(dir) = endpoint.parent().filter(|d| !d.as_os_str().is_empty()) {
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
            .create(dir)
            .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    }
    match transport::inspect(endpoint) {
        Ok(Entry::Socket) if transport::is_listening(endpoint) => {
            Err(format!("a host already listens on {}", endpoint.display()))
        }
        Ok(Entry::Socket) => std::fs::remove_file(endpoint)
            .map_err(|e| format!("could not remove {}: {e}", endpoint.display())),
        _ => Ok(()),
    }
}

/// Starts the host; for a binary also the process, to reap it (or kill it when it cannot be
/// reached).
pub(super) fn launch(
    host: &PtyHost,
    start: &HostStart,
    stderr: Option<std::fs::File>,
) -> Result<Option<std::process::Child>, String> {
    match host {
        PtyHost::Launch(f) => f(start).map(|()| None),
        PtyHost::Binary(program) => {
            if !program.is_file() {
                return Err(format!(
                    "poimen-pty-host is missing: {} (it ships next to poimen-server)",
                    program.display()
                ));
            }
            run_binary(program, start, stderr).map(Some)
        }
    }
}

/// Runs the host binary in its own process group without a terminal, writes the start line and
/// waits for `hello` (or the host's `error`) on its stdout.
#[allow(clippy::disallowed_methods)] // CREATE_NO_WINDOW is set below; gui_path is the app's
fn run_binary(
    program: &Path,
    start: &HostStart,
    stderr: Option<std::fs::File>,
) -> Result<std::process::Child, String> {
    use std::process::{Command, Stdio};
    let mut command = Command::new(program);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr.map_or_else(Stdio::null, Stdio::from));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Signals to the server's group (a Ctrl+C in the terminal that started it) do not reach
        // the host, which is meant to outlive the server.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW
        command.creation_flags(0x0000_0200 | 0x0800_0000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", program.display()))?;
    let fail = |child: &mut std::process::Child, message: String| {
        let _ = child.kill();
        let _ = child.wait();
        Err(message)
    };
    let written = child
        .stdin
        .take()
        .map(|mut stdin| stdin.write_all(&to_line(start)));
    if let Some(Err(e)) = written {
        return fail(&mut child, format!("could not start the host: {e}"));
    }
    let Some(stdout) = child.stdout.take() else {
        return fail(&mut child, "the host has no stdout".into());
    };
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(first_line(stdout));
    });
    let line = match rx.recv_timeout(START_TIMEOUT) {
        Ok(Ok(line)) => line,
        Ok(Err(e)) => return fail(&mut child, format!("the host did not answer: {e}")),
        Err(_) => {
            let message = format!(
                "the host did not answer within {} s",
                START_TIMEOUT.as_secs()
            );
            return fail(&mut child, message);
        }
    };
    match poimen_protocol::host::parse_host_line(&line) {
        Ok(HostMessage::Hello { .. }) => Ok(child),
        Ok(HostMessage::Error { message }) => fail(&mut child, message),
        _ => fail(
            &mut child,
            format!("the host answered {:?}", line.trim_end()),
        ),
    }
}

fn first_line(input: impl Read) -> io::Result<String> {
    let mut line = String::new();
    BufReader::new(input).read_line(&mut line)?;
    if line.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "it ended without a line",
        ));
    }
    Ok(line)
}
