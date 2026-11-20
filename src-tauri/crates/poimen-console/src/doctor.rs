//! `poimen doctor` (#972): one line per check, as text or JSON, and a local diagnostics bundle.
//!
//! This file is pure: [`report`] turns the facts of [`Inputs`] into a [`Report`]. [`collect`]
//! reads the facts from the machine (read-only), [`bundle`] writes the JSON file a user can send.

pub mod bundle;
pub mod collect;

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warning,
    Error,
}

impl Status {
    pub fn word(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warning => "warning",
            Status::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub id: String,
    pub status: Status,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub checks: Vec<Check>,
}

impl Report {
    /// 0 when no check is an error, else 1.
    pub fn exit_code(&self) -> u8 {
        u8::from(self.checks.iter().any(|c| c.status == Status::Error))
    }

    pub fn to_json(&self) -> Value {
        json!({"checks": self.checks.iter().map(|c| json!({
            "id": c.id, "status": c.status.word(), "detail": c.detail,
        })).collect::<Vec<_>>()})
    }

    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for c in &self.checks {
            out.push_str(&format!(
                "{:<7} {:<14} {}\n",
                c.status.word(),
                c.id,
                c.detail
            ));
        }
        out
    }
}

/// What the server answered to `ping`, or why it did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerProbe {
    /// Nothing listens on the socket.
    NotRunning,
    Answers {
        version: String,
        protocol: u32,
    },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostProbe {
    /// There is no `poimen-server` to look beside.
    NoServer,
    Missing,
    /// The line `poimen-pty-host --version` printed.
    Version(String),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileProbe {
    Missing,
    Unreadable(String),
    Damaged(String),
    Parsed {
        spaces: usize,
        tabs: usize,
        terminals: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateProbe {
    pub path: String,
    pub file: FileProbe,
    /// `server.json.bak` is what the server would load: the file is missing or older than it.
    pub backup: bool,
    /// How many `server.json.damaged-<seconds>` files exist.
    pub damaged: usize,
}

/// A state of the hooks or the MCP entry of `poimen-server integration status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    Installed,
    Partial,
    Missing,
    Unknown,
}

impl Hook {
    fn word(self) -> &'static str {
        match self {
            Hook::Installed => "installed",
            Hook::Partial => "partially installed",
            Hook::Missing => "not installed",
            Hook::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Integration {
    pub hooks: Hook,
    /// `None`: the agent has no MCP entry to register.
    pub mcp: Option<Hook>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentProbe {
    pub kind: String,
    /// The CLI is on the login-shell `PATH`.
    pub cli: bool,
    /// `Err`: why `integration status` could not be read.
    pub integration: Result<Integration, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolProbe {
    pub name: String,
    pub found: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inputs {
    pub version: String,
    pub channel: String,
    /// The protocol version this console speaks.
    pub protocol: u32,
    pub server: ServerProbe,
    pub host: HostProbe,
    pub state: StateProbe,
    pub agents: Vec<AgentProbe>,
    pub tools: Vec<ToolProbe>,
}

fn check(id: &str, status: Status, detail: impl Into<String>) -> Check {
    Check {
        id: id.into(),
        status,
        detail: detail.into(),
    }
}

/// `running` is older than `own` (numeric `x.y.z`, a pre-release suffix ignored).
fn is_older(running: &str, own: &str) -> bool {
    fn parse(v: &str) -> Option<Vec<u64>> {
        v.split(['-', '+'])
            .next()?
            .split('.')
            .map(|p| p.parse().ok())
            .collect()
    }
    matches!((parse(running), parse(own)), (Some(r), Some(o)) if r < o)
}

/// The checks for `inputs`. With `paths` false no directory is named (for the bundle).
pub fn report(i: &Inputs, paths: bool) -> Report {
    let mut checks = vec![
        check(
            "console",
            Status::Ok,
            format!("poimen {} ({})", i.version, i.channel),
        ),
        server_check(i),
        host_check(&i.host),
        state_check(&i.state, paths),
    ];
    checks.extend(i.agents.iter().map(agent_check));
    checks.extend(i.tools.iter().map(|t| {
        // git is needed for worktrees and the Changes tab; gh only for the GitHub views.
        let missing = if t.name == "git" {
            Status::Error
        } else {
            Status::Warning
        };
        match t.found {
            true => check(
                &format!("tool-{}", t.name),
                Status::Ok,
                "found on the login-shell PATH",
            ),
            false => check(
                &format!("tool-{}", t.name),
                missing,
                "not found on the login-shell PATH",
            ),
        }
    }));
    Report { checks }
}

fn server_check(i: &Inputs) -> Check {
    match &i.server {
        ServerProbe::NotRunning => check(
            "server",
            Status::Warning,
            "not running (the app and `poimen-server` start it)",
        ),
        ServerProbe::Failed(e) => check("server", Status::Error, format!("no answer: {e}")),
        ServerProbe::Answers { version, protocol } => {
            let line = format!("version {version}, protocol {protocol}");
            if *protocol != i.protocol {
                check(
                    "server",
                    Status::Error,
                    format!(
                        "{line}: incompatible with this console (protocol {})",
                        i.protocol
                    ),
                )
            } else if is_older(version, &i.version) {
                check(
                    "server",
                    Status::Warning,
                    format!("{line}: outdated, this console is {}", i.version),
                )
            } else {
                check("server", Status::Ok, line)
            }
        }
    }
}

fn host_check(host: &HostProbe) -> Check {
    match host {
        HostProbe::NoServer => check(
            "pty-host",
            Status::Warning,
            "poimen-server was not found next to poimen or on PATH",
        ),
        HostProbe::Missing => check(
            "pty-host",
            Status::Error,
            "poimen-pty-host is missing next to poimen-server",
        ),
        HostProbe::Version(line) => check("pty-host", Status::Ok, line.clone()),
        HostProbe::Failed(e) => check(
            "pty-host",
            Status::Error,
            format!("poimen-pty-host --version failed: {e}"),
        ),
    }
}

fn state_check(s: &StateProbe, paths: bool) -> Check {
    let file = if paths {
        s.path.as_str()
    } else {
        "server.json"
    };
    let mut notes = Vec::new();
    if s.backup {
        notes.push("server.json.bak is newer than the file or the file is missing (a save was interrupted)".to_string());
    }
    if s.damaged > 0 {
        notes.push(format!("{} damaged copy(ies) kept beside it", s.damaged));
    }
    let (mut status, mut detail) = match &s.file {
        FileProbe::Missing => (Status::Ok, format!("{file}: not written yet")),
        FileProbe::Unreadable(e) => (Status::Error, format!("{file}: cannot be read: {e}")),
        FileProbe::Damaged(e) => (Status::Error, format!("{file}: does not parse: {e}")),
        FileProbe::Parsed {
            spaces,
            tabs,
            terminals,
        } => (
            Status::Ok,
            format!("{file}: {spaces} spaces, {tabs} tabs, {terminals} running terminals"),
        ),
    };
    if !notes.is_empty() {
        if status == Status::Ok {
            status = Status::Warning;
        }
        detail = format!("{detail}; {}", notes.join("; "));
    }
    check("state", status, detail)
}

fn agent_check(a: &AgentProbe) -> Check {
    let id = format!("agent-{}", a.kind);
    let cli = if a.cli {
        "cli found"
    } else {
        "cli not found on the login-shell PATH"
    };
    match &a.integration {
        Err(e) => check(&id, Status::Warning, format!("{cli}; hooks unknown: {e}")),
        Ok(int) => {
            let mcp = match int.mcp {
                Some(m) => format!("mcp {}", m.word()),
                None => "mcp unavailable".to_string(),
            };
            let good = a.cli
                && int.hooks == Hook::Installed
                && matches!(int.mcp, None | Some(Hook::Installed));
            let status = if good { Status::Ok } else { Status::Warning };
            check(
                &id,
                status,
                format!("{cli}; hooks {}; {mcp}", int.hooks.word()),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn good() -> Inputs {
        Inputs {
            version: "0.27.0".into(),
            channel: "stable".into(),
            protocol: 9,
            server: ServerProbe::Answers {
                version: "0.27.0".into(),
                protocol: 9,
            },
            host: HostProbe::Version("poimen-pty-host 0.27.0 (host protocol 1)".into()),
            state: StateProbe {
                path: "/home/me/.local/state/poimen/server.json".into(),
                file: FileProbe::Parsed {
                    spaces: 2,
                    tabs: 3,
                    terminals: 4,
                },
                backup: false,
                damaged: 0,
            },
            agents: vec![AgentProbe {
                kind: "claude".into(),
                cli: true,
                integration: Ok(Integration {
                    hooks: Hook::Installed,
                    mcp: Some(Hook::Installed),
                }),
            }],
            tools: vec![
                ToolProbe {
                    name: "gh".into(),
                    found: true,
                },
                ToolProbe {
                    name: "git".into(),
                    found: true,
                },
            ],
        }
    }

    fn status_of(r: &Report, id: &str) -> Status {
        r.checks.iter().find(|c| c.id == id).unwrap().status
    }

    #[test]
    fn all_ok_exits_zero() {
        let r = report(&good(), true);
        assert!(r.checks.iter().all(|c| c.status == Status::Ok));
        assert_eq!(r.exit_code(), 0);
    }

    #[test]
    fn a_stopped_server_is_a_warning_and_exits_zero() {
        let mut i = good();
        i.server = ServerProbe::NotRunning;
        let r = report(&i, true);
        assert_eq!(status_of(&r, "server"), Status::Warning);
        assert_eq!(r.exit_code(), 0);
    }

    #[test]
    fn incompatible_server_is_an_error_and_outdated_a_warning() {
        let mut i = good();
        i.server = ServerProbe::Answers {
            version: "0.27.0".into(),
            protocol: 8,
        };
        let r = report(&i, true);
        assert_eq!(status_of(&r, "server"), Status::Error);
        assert_eq!(r.exit_code(), 1);
        i.server = ServerProbe::Answers {
            version: "0.26.0".into(),
            protocol: 9,
        };
        assert_eq!(status_of(&report(&i, true), "server"), Status::Warning);
        i.server = ServerProbe::Answers {
            version: "0.28.0".into(),
            protocol: 9,
        };
        assert_eq!(status_of(&report(&i, true), "server"), Status::Ok);
    }

    #[test]
    fn backup_or_damaged_copy_is_a_warning_and_a_broken_file_an_error() {
        let mut i = good();
        i.state.backup = true;
        assert_eq!(status_of(&report(&i, true), "state"), Status::Warning);
        i.state.backup = false;
        i.state.damaged = 1;
        assert_eq!(status_of(&report(&i, true), "state"), Status::Warning);
        i.state.file = FileProbe::Damaged("expected value".into());
        let r = report(&i, true);
        assert_eq!(status_of(&r, "state"), Status::Error);
        assert_eq!(r.exit_code(), 1);
    }

    #[test]
    fn missing_hooks_and_cli_are_warnings() {
        let mut i = good();
        i.agents[0].integration = Ok(Integration {
            hooks: Hook::Missing,
            mcp: Some(Hook::Installed),
        });
        assert_eq!(
            status_of(&report(&i, true), "agent-claude"),
            Status::Warning
        );
        i.agents[0].integration = Err("poimen-server not found".into());
        assert_eq!(
            status_of(&report(&i, true), "agent-claude"),
            Status::Warning
        );
        let mut i = good();
        i.agents[0].cli = false;
        assert_eq!(
            status_of(&report(&i, true), "agent-claude"),
            Status::Warning
        );
    }

    #[test]
    fn missing_host_or_git_is_an_error_and_missing_gh_a_warning() {
        let mut i = good();
        i.host = HostProbe::Missing;
        assert_eq!(report(&i, true).exit_code(), 1);
        let mut i = good();
        i.tools[1].found = false;
        assert_eq!(report(&i, true).exit_code(), 1);
        let mut i = good();
        i.tools[0].found = false;
        let r = report(&i, true);
        assert_eq!(status_of(&r, "tool-gh"), Status::Warning);
        assert_eq!(r.exit_code(), 0);
    }

    #[test]
    fn json_has_id_status_and_detail_and_paths_can_be_left_out() {
        let r = report(&good(), true);
        let v = r.to_json();
        let first = &v["checks"][0];
        assert_eq!(first["id"], "console");
        assert_eq!(first["status"], "ok");
        assert!(first["detail"].as_str().unwrap().contains("0.27.0"));
        assert!(
            r.to_text()
                .contains("/home/me/.local/state/poimen/server.json")
        );
        assert!(!report(&good(), false).to_text().contains("/home/me"));
    }
}
