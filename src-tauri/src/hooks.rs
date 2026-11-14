//! Agent hooks of the poimen backend: whether Claude Code and Copilot CLI call `poimen-server`
//! for their status, and installing them. Both go through the bundled `poimen-server integration
//! status|install <agent>` (the same CLI a user can run by hand); this module never edits the
//! agents' settings files itself.

use std::path::PathBuf;

use serde::Serialize;

use crate::gui_path;
use crate::poimen_server::{self, Runner, SystemRunner};

/// The agents `poimen-server integration` knows.
pub const AGENTS: [&str; 3] = ["claude", "copilot", "gemini"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HookState {
    Installed,
    /// Some of the hook events are installed, not all.
    Partial,
    Missing,
    /// The CLI could not be run or answered something unknown.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HookStatus {
    pub agent: String,
    pub state: HookState,
}

/// Runs `poimen-server integration <args>`; faked in tests.
pub trait Exec {
    fn run(&self, args: &[&str]) -> Result<String, String>;
}

/// Maps the output of `integration status` to a state.
pub fn parse_status(stdout: &str) -> HookState {
    let text = stdout.trim();
    if text == "installed" {
        HookState::Installed
    } else if text == "not installed" {
        HookState::Missing
    } else if text.starts_with("partially installed") {
        HookState::Partial
    } else {
        HookState::Unknown
    }
}

pub fn status_all(exec: &impl Exec) -> Vec<HookStatus> {
    AGENTS
        .iter()
        .map(|agent| HookStatus {
            agent: (*agent).to_owned(),
            state: exec
                .run(&["integration", "status", agent])
                .map_or(HookState::Unknown, |out| parse_status(&out)),
        })
        .collect()
}

/// Installs the hooks of `agent`; an unknown agent is refused before anything runs.
pub fn install(exec: &impl Exec, agent: &str) -> Result<(), String> {
    if !AGENTS.contains(&agent) {
        return Err(format!("unknown agent {agent}"));
    }
    exec.run(&["integration", "install", agent]).map(|_| ())
}

/// The bundled `poimen-server` (next to the app, else on the login-shell `PATH`). Inside a Linux
/// AppImage it runs the staged copy (as `poimen_server::SystemRunner` starts it) and has the
/// hooks call its stable link, not the mount of this run.
pub struct SystemExec;

/// Only `install` stages the server (the hooks it writes must call the stable link); a read-only
/// query like `status` runs the bundled binary as it is and changes no files.
fn staged_for(
    args: &[&str],
    program: &std::path::Path,
    prepare: impl FnOnce(&std::path::Path) -> Option<poimen_server::Staged>,
) -> Option<poimen_server::Staged> {
    let installs = args.get(1) == Some(&"install");
    (installs && !cfg!(windows))
        .then(|| prepare(program))
        .flatten()
}

impl Exec for SystemExec {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        let program: PathBuf = SystemRunner::new()
            .resolve()
            .ok_or("`poimen-server` was not found next to poimen or on PATH")?;
        let staged = staged_for(args, &program, poimen_server::prepare_here);
        let mut command = match &staged {
            Some(staged) => {
                let mut command = gui_path::command(&staged.program);
                if let Some(exe) = &staged.hook_exe {
                    command.env(poimen_client::protocol::ENV_HOOK_EXE, exe);
                }
                command
            }
            None => gui_path::command(program),
        };
        command.args(args);
        // A GUI app on Windows already has the user's PATH (as in `backend.rs`).
        if !cfg!(windows) {
            command.env("PATH", gui_path::child_path());
        }
        let output = command.output().map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct Fake {
        calls: RefCell<Vec<String>>,
        answer: Result<String, String>,
    }

    impl Exec for Fake {
        fn run(&self, args: &[&str]) -> Result<String, String> {
            self.calls.borrow_mut().push(args.join(" "));
            self.answer.clone()
        }
    }

    fn fake(answer: Result<&str, &str>) -> Fake {
        Fake {
            calls: RefCell::new(vec![]),
            answer: answer.map(str::to_owned).map_err(str::to_owned),
        }
    }

    #[test]
    fn parses_the_status_lines_of_the_cli() {
        assert_eq!(parse_status("installed\n"), HookState::Installed);
        assert_eq!(parse_status("not installed\n"), HookState::Missing);
        assert_eq!(
            parse_status("partially installed: Stop, Notification\n"),
            HookState::Partial
        );
        assert_eq!(parse_status(""), HookState::Unknown);
        assert_eq!(parse_status("whatever"), HookState::Unknown);
    }

    #[test]
    fn status_asks_for_every_agent() {
        let exec = fake(Ok("not installed\n"));
        let all = status_all(&exec);
        assert_eq!(
            all,
            vec![
                HookStatus {
                    agent: "claude".into(),
                    state: HookState::Missing
                },
                HookStatus {
                    agent: "copilot".into(),
                    state: HookState::Missing
                },
                HookStatus {
                    agent: "gemini".into(),
                    state: HookState::Missing
                },
            ]
        );
        assert_eq!(
            *exec.calls.borrow(),
            [
                "integration status claude",
                "integration status copilot",
                "integration status gemini"
            ]
        );
    }

    #[test]
    fn a_failing_cli_is_unknown_not_missing() {
        let all = status_all(&fake(Err("not found")));
        assert!(all.iter().all(|s| s.state == HookState::Unknown));
    }

    #[test]
    fn install_runs_the_cli_for_a_known_agent() {
        let exec = fake(Ok("installed in x\n"));
        assert_eq!(install(&exec, "copilot"), Ok(()));
        assert_eq!(*exec.calls.borrow(), ["integration install copilot"]);
    }

    #[test]
    fn install_refuses_an_unknown_agent_and_passes_errors_on() {
        let exec = fake(Ok(""));
        assert!(install(&exec, "../x").is_err());
        assert!(exec.calls.borrow().is_empty());
        assert_eq!(install(&fake(Err("boom")), "claude"), Err("boom".into()));
    }

    #[cfg(unix)]
    #[test]
    fn status_does_not_stage_and_install_does() {
        use std::cell::Cell;
        use std::path::Path;
        let program = Path::new("/mnt/app/poimen-server");
        let staged = || poimen_server::Staged {
            program: "/data/bin/1/poimen-server".into(),
            hook_exe: Some("/data/bin/current".into()),
        };

        let calls = Cell::new(0);
        let prepare = |_: &Path| {
            calls.set(calls.get() + 1);
            Some(staged())
        };
        assert_eq!(
            staged_for(&["integration", "status", "claude"], program, prepare),
            None
        );
        assert_eq!(calls.get(), 0);

        assert_eq!(
            staged_for(&["integration", "install", "claude"], program, prepare),
            Some(staged())
        );
        assert_eq!(calls.get(), 1);
    }
}
