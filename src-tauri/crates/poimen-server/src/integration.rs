//! The agent kinds with hooks, in one place: `poimen-server hook <kind>` and `poimen-server
//! integration install|uninstall|status <kind>` both dispatch through [`KINDS`]. A further kind
//! is a mapper module (`claude.rs`, `copilot.rs`, `gemini.rs`) plus one line here (and in the
//! frontend's `core/backend.ts`). A kind without an entry is still started and prompted; it just
//! reports no status.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use poimen_protocol::{CHANNEL, Channel};

use crate::claude::{CliVersion, HookStatus, Version};
use crate::hook::HookEnv;
use crate::mcp_config::{self, McpKind};
use crate::{claude, copilot, gemini};

/// One agent kind with hooks.
pub struct Kind {
    /// The name on the command line and in the terminal's `agent` field.
    pub name: &'static str,
    /// The agent CLI whose version decides which events are registered (`claude`: an event an
    /// older CLI does not know makes it reject the whole settings file); `None`: all events.
    pub cli: Option<&'static str>,
    /// The file the installer writes by default, below the home directory.
    pub default_path: fn(&Path, Channel) -> PathBuf,
    /// Registers the hooks for the CLI version (`None`: unknown).
    pub install: fn(&Path, &Path, Channel, Option<Version>) -> Result<(), String>,
    pub uninstall: fn(&Path, Channel) -> Result<bool, String>,
    /// The events whose hook is there and those the CLI of that version needs and lacks.
    pub status: fn(&Path, Channel, Option<Version>) -> Result<HookStatus, String>,
    /// Where and how the agent's MCP config registers `poimen mcp` (#614); `None`: no MCP.
    pub mcp: Option<McpKind>,
    /// `hook <kind> [event]`: reads stdin and reports; never fails the agent.
    pub run_hook: fn(Option<String>, &HookEnv),
}

/// Every kind with hooks.
pub const KINDS: [Kind; 3] = [
    Kind {
        name: "claude",
        cli: Some("claude"),
        default_path: |home, _| claude::default_settings_path(home),
        install: claude::install,
        uninstall: claude::uninstall,
        status: claude::status,
        mcp: Some(mcp_config::CLAUDE),
        run_hook: |_, env| {
            claude::run_hook(std::io::stdin(), env);
        },
    },
    Kind {
        name: "copilot",
        cli: None,
        default_path: copilot::default_hook_path,
        install: copilot::install,
        uninstall: copilot::uninstall,
        status: copilot::status,
        mcp: Some(mcp_config::COPILOT),
        run_hook: |event, env| {
            copilot::run_hook(std::io::stdin(), event, env);
        },
    },
    Kind {
        name: "gemini",
        cli: None,
        default_path: |home, _| gemini::default_settings_path(home),
        install: gemini::install,
        uninstall: gemini::uninstall,
        status: gemini::status,
        mcp: None,
        run_hook: |_, env| {
            gemini::run_hook(std::io::stdin(), env);
        },
    },
];

impl Kind {
    /// The version of the CLI that decides the events of this kind; `None` when the kind has no
    /// such CLI or its version is unknown.
    pub fn cli_version(&self, cli: &impl CliVersion) -> Option<Version> {
        self.cli.and_then(|program| cli.version(program))
    }
}

/// The kind called `name`.
pub fn kind(name: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.name == name)
}

/// `claude|copilot|gemini`, for usage texts.
pub fn names() -> String {
    KINDS.map(|k| k.name).join("|")
}

/// `integration install|uninstall|status <kind> [--settings <path>]` (`args` after `integration`),
/// for `poimen integration` and the deprecated `poimen-server integration`. `prog` prefixes
/// errors; `hook_exe` is the `poimen-server` binary the hooks call (and where the `poimen`
/// binary is looked for first), or why it is unknown.
pub fn run(args: &[String], prog: &str, hook_exe: Result<PathBuf, String>) -> ExitCode {
    let (Some(action), Some(kind)) = (args.first(), args.get(1).and_then(|a| kind(a))) else {
        eprintln!("{}", usage(prog));
        return ExitCode::from(2);
    };
    let path = match args.get(2).map(String::as_str) {
        Some("--settings") => args.get(3).map(PathBuf::from),
        None => std::env::home_dir().map(|h| (kind.default_path)(&h, CHANNEL)),
        _ => None,
    };
    let Some(path) = path else {
        eprintln!("{}", usage(prog));
        return ExitCode::from(2);
    };
    // The MCP config is the agent's own file below the home directory; an explicit `--settings`
    // path is a test or special setup and leaves it alone.
    let mcp = kind
        .mcp
        .filter(|_| args.get(2).is_none())
        .and_then(|m| Some((m, (m.default_path)(&std::env::home_dir()?))));
    let cli = kind.cli_version(&claude::SystemCli);
    let result = match action.as_str() {
        "install" => hook_exe.and_then(|exe| {
            (kind.install)(&path, &exe, CHANNEL, cli)?;
            let mut message = format!("installed in {}", path.display());
            if let Some((m, mcp_path)) = &mcp {
                let path_var = std::env::var_os("PATH");
                match mcp_config::console_exe(&exe, CHANNEL, path_var.as_deref()) {
                    Some(console) => {
                        mcp_config::install(mcp_path, *m, &console, CHANNEL)?;
                        message.push_str(&format!("; mcp in {}", mcp_path.display()));
                    }
                    None => message.push_str("; mcp skipped: the poimen binary was not found"),
                }
            }
            Ok(message)
        }),
        "uninstall" => (kind.uninstall)(&path, CHANNEL).and_then(|removed| {
            let mcp_removed = match &mcp {
                Some((_, mcp_path)) => mcp_config::uninstall(mcp_path, CHANNEL)?,
                None => false,
            };
            Ok(if removed || mcp_removed {
                format!("removed from {}", path.display())
            } else {
                "nothing to remove".to_owned()
            })
        }),
        "status" => (kind.status)(&path, CHANNEL, cli).and_then(|status| {
            let mut out = if status.installed.is_empty() {
                "not installed".to_owned()
            } else if status.missing.is_empty() {
                "installed".to_owned()
            } else {
                format!(
                    "partially installed: {}; missing: {}",
                    status.installed.join(", "),
                    status.missing.join(", ")
                )
            };
            if let Some((_, mcp_path)) = &mcp {
                out.push('\n');
                out.push_str(mcp_status_line(mcp_path)?);
            }
            Ok(out)
        }),
        _ => {
            eprintln!("{}", usage(prog));
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(message) => {
            println!("{message}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{prog}: {e}");
            ExitCode::FAILURE
        }
    }
}

/// `mcp: installed|not installed|unavailable` (`unavailable`: no `poimen` binary to register).
fn mcp_status_line(path: &std::path::Path) -> Result<&'static str, String> {
    Ok(match mcp_config::status(path, CHANNEL)? {
        mcp_config::McpState::Installed => "mcp: installed",
        mcp_config::McpState::Missing => {
            let found = std::env::current_exe().ok().and_then(|exe| {
                mcp_config::console_exe(&exe, CHANNEL, std::env::var_os("PATH").as_deref())
            });
            if found.is_some() {
                "mcp: not installed"
            } else {
                "mcp: unavailable"
            }
        }
    })
}

fn usage(prog: &str) -> String {
    format!(
        "usage: {prog} integration install|uninstall|status {} [--settings <path>]",
        names()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_unique_and_found_by_name() {
        let mut names: Vec<_> = KINDS.iter().map(|k| k.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), KINDS.len());
        assert!(kind("codex").is_none());
        assert_eq!(super::names(), "claude|copilot|gemini");
    }

    #[test]
    fn claude_and_copilot_register_poimen_mcp_in_their_own_files_gemini_does_not() {
        let home = Path::new("/h");
        let path = |n: &str| (kind(n).unwrap().mcp.unwrap().default_path)(home);
        assert_eq!(path("claude"), PathBuf::from("/h/.claude.json"));
        assert_eq!(
            path("copilot"),
            PathBuf::from("/h/.copilot/mcp-config.json")
        );
        assert!(kind("gemini").unwrap().mcp.is_none());
    }

    #[test]
    fn the_table_matches_the_list_the_app_uses() {
        let names: Vec<_> = KINDS.iter().map(|k| k.name).collect();
        assert_eq!(names, poimen_protocol::HOOK_KINDS.to_vec());
    }

    #[test]
    fn every_kind_runs_its_hook_silently_outside_a_poimen_terminal() {
        let env = HookEnv {
            terminal_id: None,
            in_poimen: false,
            foreign: false,
            socket: PathBuf::from("/nonexistent/poimen/s.sock"),
            log: None,
        };
        // `hook <kind>` and the `hook copilot <event>` form.
        for k in &KINDS {
            (k.run_hook)(None, &env);
        }
        (kind("copilot").unwrap().run_hook)(Some("sessionStart".into()), &env);
    }

    #[test]
    fn every_kind_installs_reports_and_removes_in_a_temp_dir() {
        let dir = std::env::temp_dir().join(format!("poimen-kinds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("poimen-server");
        std::fs::write(&exe, "").unwrap();
        for k in &KINDS {
            let path = (k.default_path)(&dir, Channel::Stable);
            assert!(path.starts_with(&dir), "{}", k.name);
            let cli = Some((9, 9, 9));
            assert!(
                (k.status)(&path, Channel::Stable, cli)
                    .unwrap()
                    .installed
                    .is_empty()
            );
            (k.install)(&path, &exe, Channel::Stable, cli).unwrap();
            let status = (k.status)(&path, Channel::Stable, cli).unwrap();
            assert!(
                status.missing.is_empty() && !status.installed.is_empty(),
                "{}",
                k.name
            );
            assert!((k.uninstall)(&path, Channel::Stable).unwrap());
            assert!(
                (k.status)(&path, Channel::Stable, cli)
                    .unwrap()
                    .installed
                    .is_empty()
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
