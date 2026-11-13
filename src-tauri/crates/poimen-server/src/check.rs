//! `machine.check` (#281): what a machine needs for tasks, as one result per check with the
//! command to run on the machine when something is missing. Read-only: nothing is installed,
//! logged in or changed, and the commands in `fix` are only shown.
//!
//! Tools are looked for on the login-shell `PATH` (`poimen_core::gui_path::child_path`), since a
//! server started over SSH has a thin one. `gh auth status` is the only command that runs.
//! Everything outside the process goes through [`Env`], which tests fake.

use std::io;
use std::path::PathBuf;
use std::process::Stdio;

use poimen_core::gui_path;
use poimen_protocol::{CHANNEL, Channel, CheckStatus, MachineCheckItem, MachineCheckResult};

use crate::{claude, copilot};

/// What a check needs from the machine.
pub trait Env {
    /// Whether `program` is on the `PATH`.
    fn has(&self, program: &str) -> bool;
    /// Runs `program` with `args`; the output on success or failure of the run. `Err` means it
    /// could not be started.
    fn run(&self, program: &str, args: &[&str]) -> io::Result<(bool, String)>;
    /// The user's home, whose settings the hook integrations are read from.
    fn home(&self) -> Option<PathBuf>;
}

/// The real machine.
pub struct System;

impl Env for System {
    fn has(&self, program: &str) -> bool {
        gui_path::resolve_program(program, &gui_path::child_path()).is_some()
    }

    fn run(&self, program: &str, args: &[&str]) -> io::Result<(bool, String)> {
        let path = gui_path::child_path();
        let exe = gui_path::resolve_program(program, &path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "not found"))?;
        let out = gui_path::command(exe)
            .args(args)
            .env("PATH", &path)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .output()?;
        let text = [out.stdout, out.stderr]
            .iter()
            .map(|b| String::from_utf8_lossy(b).trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        Ok((out.status.success(), text))
    }

    fn home(&self) -> Option<PathBuf> {
        std::env::home_dir().filter(|h| !h.as_os_str().is_empty())
    }
}

fn item(
    id: &str,
    status: CheckStatus,
    detail: Option<String>,
    fix: Option<String>,
) -> MachineCheckItem {
    MachineCheckItem {
        id: id.to_owned(),
        status,
        detail,
        fix,
    }
}

fn server_binary(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => "poimen-server",
        Channel::Rc => "poimen-server-rc",
    }
}

/// Runs every check. `version` and `protocol` are the server's own.
pub fn run(env: &dyn Env, version: &str, protocol: u32, channel: Channel) -> MachineCheckResult {
    let server = server_binary(channel);
    let mut checks = vec![item(
        "server",
        CheckStatus::Ok,
        Some(format!("{server} {version} (protocol {protocol})")),
        Some(format!(
            "gh release download --repo GravionLabs/poimen --pattern '{server}-*-unknown-linux-musl.tar.gz' (Linux; unpack next to the existing {server})"
        )),
    )];

    checks.push(if env.has("git") {
        item("git", CheckStatus::Ok, None, None)
    } else {
        item(
            "git",
            CheckStatus::Missing,
            Some("git is not on PATH".into()),
            Some("install git with the package manager of the machine".into()),
        )
    });

    let gh = env.has("gh");
    checks.push(if gh {
        item("gh", CheckStatus::Ok, None, None)
    } else {
        item(
            "gh",
            CheckStatus::Missing,
            Some("gh is not on PATH".into()),
            Some("install the GitHub CLI (https://cli.github.com), then run: gh auth login".into()),
        )
    });
    if gh {
        checks.push(match env.run("gh", &["auth", "status"]) {
            Ok((true, _)) => item("gh_auth", CheckStatus::Ok, None, None),
            Ok((false, text)) => item(
                "gh_auth",
                CheckStatus::Missing,
                Some(first_line(&text).unwrap_or("not logged in").to_owned()),
                Some("gh auth login".into()),
            ),
            Err(e) => item(
                "gh_auth",
                CheckStatus::Missing,
                Some(format!("gh auth status failed: {e}")),
                Some("gh auth login".into()),
            ),
        });
    }

    let claude_on = env.has("claude");
    let copilot_on = env.has("copilot");
    // One agent is enough; with none, both are errors.
    let missing_agent = if claude_on || copilot_on {
        CheckStatus::Warning
    } else {
        CheckStatus::Missing
    };
    checks.push(agent(
        "claude",
        claude_on,
        missing_agent,
        "install Claude Code (https://claude.com/claude-code), then run: claude",
    ));
    checks.push(agent(
        "copilot",
        copilot_on,
        missing_agent,
        "install the GitHub Copilot CLI (npm install -g @github/copilot), then run: copilot",
    ));

    let home = env.home();
    if claude_on {
        checks.push(hooks(
            "hooks_claude",
            "claude",
            home.as_deref()
                .map(|h| claude::status(&claude::default_settings_path(h), CHANNEL)),
            claude::EVENTS.len(),
            server,
        ));
    }
    if copilot_on {
        checks.push(hooks(
            "hooks_copilot",
            "copilot",
            home.as_deref()
                .map(|h| copilot::status(&copilot::default_hook_path(h, CHANNEL), CHANNEL)),
            copilot::EVENTS.len(),
            server,
        ));
    }

    MachineCheckResult {
        version: version.to_owned(),
        protocol,
        checks,
    }
}

fn first_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|l| !l.is_empty())
}

fn agent(name: &str, present: bool, missing: CheckStatus, fix: &str) -> MachineCheckItem {
    if present {
        item(name, CheckStatus::Ok, None, None)
    } else {
        item(
            name,
            missing,
            Some(format!("{name} is not on PATH")),
            Some(fix.to_owned()),
        )
    }
}

fn hooks(
    id: &str,
    agent: &str,
    installed: Option<Result<Vec<&'static str>, String>>,
    all: usize,
    server: &str,
) -> MachineCheckItem {
    let fix = Some(format!("{server} integration install {agent}"));
    match installed {
        Some(Ok(events)) if events.len() == all => item(id, CheckStatus::Ok, None, None),
        Some(Ok(events)) if events.is_empty() => item(
            id,
            CheckStatus::Warning,
            Some("agent status hooks are not installed".into()),
            fix,
        ),
        Some(Ok(events)) => item(
            id,
            CheckStatus::Warning,
            Some(format!("partially installed: {}", events.join(", "))),
            fix,
        ),
        Some(Err(e)) => item(id, CheckStatus::Warning, Some(e), fix),
        None => item(
            id,
            CheckStatus::Warning,
            Some("no home directory".into()),
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    struct Fake {
        tools: HashSet<&'static str>,
        auth: Option<io::Result<(bool, String)>>,
        home: Option<PathBuf>,
    }

    impl Env for Fake {
        fn has(&self, program: &str) -> bool {
            self.tools.contains(program)
        }
        fn run(&self, program: &str, args: &[&str]) -> io::Result<(bool, String)> {
            assert_eq!((program, args), ("gh", &["auth", "status"][..]));
            match self.auth.as_ref().expect("auth was not expected") {
                Ok(v) => Ok(v.clone()),
                Err(e) => Err(io::Error::new(e.kind(), e.to_string())),
            }
        }
        fn home(&self) -> Option<PathBuf> {
            self.home.clone()
        }
    }

    fn home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-check-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake(tools: &[&'static str], auth: Option<io::Result<(bool, String)>>, h: &str) -> Fake {
        Fake {
            tools: tools.iter().copied().collect(),
            auth,
            home: Some(home(h)),
        }
    }

    fn get<'a>(r: &'a MachineCheckResult, id: &str) -> &'a MachineCheckItem {
        r.checks
            .iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("no {id}"))
    }

    #[test]
    fn a_complete_machine_has_no_missing_check() {
        let env = fake(
            &["git", "gh", "claude", "copilot"],
            Some(Ok((true, "Logged in".into()))),
            "full",
        );
        let h = env.home.clone().unwrap();
        let exe = std::env::current_exe().unwrap();
        claude::install(&claude::default_settings_path(&h), &exe, CHANNEL).unwrap();
        copilot::install(&copilot::default_hook_path(&h, CHANNEL), &exe, CHANNEL).unwrap();
        let r = run(&env, "0.18.0", 6, Channel::Stable);
        assert_eq!(get(&r, "hooks_claude").status, CheckStatus::Ok);
        assert_eq!(get(&r, "hooks_copilot").status, CheckStatus::Ok);
        assert_eq!(get(&r, "git").status, CheckStatus::Ok);
        assert_eq!(get(&r, "gh_auth").status, CheckStatus::Ok);
        assert_eq!(
            get(&r, "server").detail.as_deref(),
            Some("poimen-server 0.18.0 (protocol 6)")
        );
        assert_eq!(r.version, "0.18.0");
        assert_eq!(r.protocol, 6);
        assert!(r.ok());
    }

    #[test]
    fn missing_git_and_gh_are_errors_with_a_fix_and_skip_the_login_check() {
        let env = fake(&["claude"], None, "nogit");
        let r = run(&env, "1", 6, Channel::Stable);
        assert_eq!(get(&r, "git").status, CheckStatus::Missing);
        assert_eq!(get(&r, "gh").status, CheckStatus::Missing);
        assert!(
            get(&r, "gh")
                .fix
                .as_deref()
                .unwrap()
                .contains("gh auth login")
        );
        assert!(r.checks.iter().all(|c| c.id != "gh_auth"));
        assert!(!r.ok());
    }

    #[test]
    fn not_logged_in_is_missing_and_shows_the_first_line_of_gh() {
        let env = fake(
            &["git", "gh", "claude"],
            Some(Ok((
                false,
                "\nYou are not logged into any GitHub hosts.\nRun gh auth login".into(),
            ))),
            "login",
        );
        let c = run(&env, "1", 6, Channel::Stable)
            .checks
            .into_iter()
            .find(|c| c.id == "gh_auth")
            .unwrap();
        assert_eq!(c.status, CheckStatus::Missing);
        assert_eq!(
            c.detail.as_deref(),
            Some("You are not logged into any GitHub hosts.")
        );
        assert_eq!(c.fix.as_deref(), Some("gh auth login"));
    }

    #[test]
    fn a_gh_that_cannot_run_is_not_logged_in() {
        let env = fake(
            &["git", "gh", "claude"],
            Some(Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "denied",
            ))),
            "denied",
        );
        let r = run(&env, "1", 6, Channel::Stable);
        assert_eq!(get(&r, "gh_auth").status, CheckStatus::Missing);
        assert!(
            get(&r, "gh_auth")
                .detail
                .as_deref()
                .unwrap()
                .contains("denied")
        );
    }

    #[test]
    fn one_missing_agent_is_a_warning() {
        let env = fake(
            &["git", "gh", "claude"],
            Some(Ok((true, String::new()))),
            "one",
        );
        let r = run(&env, "1", 6, Channel::Stable);
        assert_eq!(get(&r, "claude").status, CheckStatus::Ok);
        assert_eq!(get(&r, "copilot").status, CheckStatus::Warning);
        assert!(get(&r, "copilot").fix.is_some());
        assert!(r.checks.iter().all(|c| c.id != "hooks_copilot"));
        assert!(r.ok());
    }

    #[test]
    fn no_agent_at_all_is_an_error() {
        let env = fake(&["git", "gh"], Some(Ok((true, String::new()))), "none");
        let r = run(&env, "1", 6, Channel::Stable);
        assert_eq!(get(&r, "claude").status, CheckStatus::Missing);
        assert_eq!(get(&r, "copilot").status, CheckStatus::Missing);
        assert!(!r.ok());
    }

    #[test]
    fn hooks_not_installed_warn_with_the_integration_command() {
        let env = fake(
            &["git", "gh", "claude"],
            Some(Ok((true, String::new()))),
            "hooks",
        );
        let r = run(&env, "1", 6, Channel::Stable);
        let c = get(&r, "hooks_claude");
        assert_eq!(c.status, CheckStatus::Warning);
        assert_eq!(
            c.fix.as_deref(),
            Some("poimen-server integration install claude")
        );
        let rc = run(&env, "1", 6, Channel::Rc);
        assert_eq!(
            get(&rc, "hooks_claude").fix.as_deref(),
            Some("poimen-server-rc integration install claude")
        );
    }

    #[test]
    fn unreadable_settings_warn_with_the_reason() {
        let env = fake(
            &["git", "gh", "claude"],
            Some(Ok((true, String::new()))),
            "bad",
        );
        let h = env.home.clone().unwrap();
        let path = claude::default_settings_path(&h);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        let c = run(&env, "1", 6, Channel::Stable)
            .checks
            .into_iter()
            .find(|c| c.id == "hooks_claude")
            .unwrap();
        assert_eq!(c.status, CheckStatus::Warning);
        assert!(c.detail.is_some());
    }
}
