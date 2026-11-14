//! The agent kinds with hooks, in one place: `poimen-server hook <kind>` and `poimen-server
//! integration install|uninstall|status <kind>` both dispatch through [`KINDS`]. A further kind
//! is a mapper module (`claude.rs`, `copilot.rs`, `gemini.rs`) plus one line here (and in the
//! frontend's `core/backend.ts`). A kind without an entry is still started and prompted; it just
//! reports no status.

use std::path::{Path, PathBuf};

use poimen_protocol::Channel;

use crate::hook::HookEnv;
use crate::{claude, copilot, gemini};

/// One agent kind with hooks.
pub struct Kind {
    /// The name on the command line and in the terminal's `agent` field.
    pub name: &'static str,
    /// Number of hook events the installer registers.
    pub events: usize,
    /// The file the installer writes by default, below the home directory.
    pub default_path: fn(&Path, Channel) -> PathBuf,
    pub install: fn(&Path, &Path, Channel) -> Result<(), String>,
    pub uninstall: fn(&Path, Channel) -> Result<bool, String>,
    /// The registered events whose hook is there.
    pub status: fn(&Path, Channel) -> Result<Vec<&'static str>, String>,
    /// `hook <kind> [event]`: reads stdin and reports; never fails the agent.
    pub run_hook: fn(Option<String>, &HookEnv),
}

/// Every kind with hooks.
pub const KINDS: [Kind; 3] = [
    Kind {
        name: "claude",
        events: claude::EVENTS.len(),
        default_path: |home, _| claude::default_settings_path(home),
        install: claude::install,
        uninstall: claude::uninstall,
        status: claude::status,
        run_hook: |_, env| {
            claude::run_hook(std::io::stdin(), env);
        },
    },
    Kind {
        name: "copilot",
        events: copilot::EVENTS.len(),
        default_path: copilot::default_hook_path,
        install: copilot::install,
        uninstall: copilot::uninstall,
        status: copilot::status,
        run_hook: |event, env| {
            copilot::run_hook(std::io::stdin(), event, env);
        },
    },
    Kind {
        name: "gemini",
        events: gemini::EVENTS.len(),
        default_path: |home, _| gemini::default_settings_path(home),
        install: gemini::install,
        uninstall: gemini::uninstall,
        status: gemini::status,
        run_hook: |_, env| {
            gemini::run_hook(std::io::stdin(), env);
        },
    },
];

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
        assert_eq!(kind("gemini").unwrap().events, gemini::EVENTS.len());
        assert!(kind("codex").is_none());
        assert_eq!(super::names(), "claude|copilot|gemini");
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
            assert!((k.status)(&path, Channel::Stable).unwrap().is_empty());
            (k.install)(&path, &exe, Channel::Stable).unwrap();
            assert_eq!(
                (k.status)(&path, Channel::Stable).unwrap().len(),
                k.events,
                "{}",
                k.name
            );
            assert!((k.uninstall)(&path, Channel::Stable).unwrap());
            assert!((k.status)(&path, Channel::Stable).unwrap().is_empty());
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
