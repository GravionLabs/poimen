//! The agent kinds with hooks, in one place: `poimen-server hook <kind>` and `poimen-server
//! integration install|uninstall|status <kind>` both dispatch through [`KINDS`]. A further kind
//! is a mapper module (`claude.rs`, `copilot.rs`, `gemini.rs`) plus one line here (and in the
//! frontend's `core/backend.ts`). A kind without an entry is still started and prompted; it just
//! reports no status.

use std::path::{Path, PathBuf};

use poimen_protocol::Channel;

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
