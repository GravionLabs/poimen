//! The commands a built layout types into its panes (`space.create`, `space.apply_layout`), typed
//! once the lock is released, and `setupPolicy: "wait"` (#992): the root pane's command is held
//! until the setup reported `ok` through a marker file.

use poimen_protocol::SpaceCreated;

use crate::pty;

/// Types the commands of a built layout once the lock is released (a write may block on a full
/// PTY); one that cannot be typed is a warning.
pub(crate) fn type_commands(typing: Vec<Typing>, created: &mut SpaceCreated) {
    for typed in typing {
        if let Some(marker) = typed.wait.clone() {
            type_after_setup(typed, marker);
            continue;
        }
        if let Err(e) = typed
            .writer
            .write(format!("{}\r", typed.command).as_bytes())
        {
            created.warnings.push(format!(
                "pane \"{}\": the command was not typed: {e}",
                typed.pane
            ));
            if typed.setup {
                created.setup_started = false;
            }
        }
    }
}

/// A command to type into a pane of a new space.
pub(crate) struct Typing {
    pub(crate) pane: String,
    pub(crate) writer: pty::PtyWriter,
    pub(crate) command: String,
    pub(crate) setup: bool,
    /// `setupPolicy: "wait"`: typed only once this marker file says the setup exited 0.
    pub(crate) wait: Option<std::path::PathBuf>,
    /// The root pane's command (the one `setupPolicy: "wait"` holds back).
    pub(crate) root: bool,
}

/// Which pane a command belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Pane,
    /// The first pane of the first tab, the one `setupPolicy: "wait"` holds back.
    Root,
    Setup,
}

/// Longest a held root command waits for the setup to end.
const SETUP_WAIT: std::time::Duration = std::time::Duration::from_secs(30 * 60);

/// Single-quotes `text` for a POSIX shell, or for PowerShell with `powershell`.
fn quote(text: &str, powershell: bool) -> String {
    if powershell {
        format!("'{}'", text.replace('\'', "''"))
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// `setup` typed so that it leaves `ok` or `fail` in `marker` when it ends.
pub(crate) fn marked_setup(setup: &str, marker: &std::path::Path, powershell: bool) -> String {
    let m = quote(&marker.to_string_lossy(), powershell);
    if powershell {
        format!(
            "{setup}; if ($?) {{ Set-Content -NoNewline -Path {m} -Value ok }} else {{ Set-Content -NoNewline -Path {m} -Value fail }}"
        )
    } else {
        format!("{setup} && echo ok > {m} || echo fail > {m}")
    }
}

/// Types the held command of `typed` once its marker says `ok`; on `fail` (or no answer in
/// [`SETUP_WAIT`]) the pane stays at its prompt with a notice typed into it.
fn type_after_setup(typed: Typing, marker: std::path::PathBuf) {
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let answer = loop {
            if let Ok(text) = std::fs::read_to_string(&marker) {
                let text = text.trim().to_owned();
                if !text.is_empty() {
                    break text;
                }
            }
            if start.elapsed() >= SETUP_WAIT {
                break "timeout".to_owned();
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        };
        let _ = std::fs::remove_file(&marker);
        let line = if answer == "ok" {
            typed.command.clone()
        } else {
            let why = if answer == "fail" {
                "the setup failed"
            } else {
                "the setup did not finish"
            };
            format!(
                "echo {}",
                quote(
                    &format!("poimen: {why}; not started: {}", typed.command),
                    false
                )
            )
        };
        let _ = typed.writer.write(format!("{line}\r").as_bytes());
    });
}

/// `setupPolicy: "wait"`: holds the root pane's command until the setup (typed into the terminal
/// `setup_terminal`) reported `ok`; the setup command reports its end through the marker file.
pub(crate) fn hold_root(typing: &mut [Typing], setup_terminal: &str) {
    if !typing.iter().any(|t| t.root) {
        return;
    }
    // Terminal ids repeat across servers: the process id and the time keep two setups apart.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let marker = std::env::temp_dir().join(format!(
        "poimen-setup-{}-{setup_terminal}-{nanos}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&marker);
    for t in typing {
        if t.root {
            t.wait = Some(marker.clone());
        } else if t.setup {
            t.command = marked_setup(&t.command, &marker, cfg!(windows));
        }
    }
}
