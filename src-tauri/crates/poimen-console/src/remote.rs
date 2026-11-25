//! Remote machines for the console (#1176, ADR 0008): `--machine <name>` reaches the
//! `poimen-server` of a machine of the user config (`machines[]`) through the same
//! `ssh -T -o BatchMode=yes ... poimen-server stdio` connection as the app
//! (`poimen_client::ssh::SshConnector`), and `poimen machines` lists them with the state of their
//! server. The messages for a server that is missing, unreachable or of another protocol are the
//! sentences of the app's machine hints (`core/machines.ts`). poimen only prints a command, it
//! never runs one.

use std::path::Path;
use std::sync::Arc;

use poimen_client::protocol::{PROTOCOL_VERSION, PingResult};
use poimen_client::ssh::SshConnector;
use poimen_client::{Client, ClientError};
use poimen_core::machines::{MachineConfig, parse};
use serde_json::{Value, json};

/// The machines of `<config dir>/config.json` under `home`, and the warnings of invalid entries.
/// A missing file or key is no machines; a file that is not JSON is an error.
pub fn load(home: Option<&Path>) -> Result<(Vec<MachineConfig>, Vec<String>), String> {
    let Some(home) = home else {
        return Ok((Vec::new(), Vec::new()));
    };
    let root = match std::fs::read_to_string(poimen_core::settings::config_path(home)) {
        Ok(text) => serde_json::from_str::<Value>(&text)
            .map_err(|e| format!("config.json is not valid JSON ({e})"))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Value::Null,
        Err(e) => return Err(format!("config.json cannot be read ({e})")),
    };
    let mut warnings = Vec::new();
    let machines = match root.get("machines") {
        Some(value) => parse(value, &mut warnings),
        None => Vec::new(),
    };
    Ok((machines, warnings))
}

/// The machine called `name`, or the error naming the configured ones.
pub fn find(machines: &[MachineConfig], name: &str) -> Result<MachineConfig, String> {
    machines
        .iter()
        .find(|m| m.name == name)
        .cloned()
        .ok_or_else(|| {
            let known: Vec<&str> = machines.iter().map(|m| m.name.as_str()).collect();
            if known.is_empty() {
                format!("no machine {name:?}: `machines` in config.json is empty")
            } else {
                format!("no machine {name:?}; configured: {}", known.join(", "))
            }
        })
}

/// A client whose every connection is one ssh process to the machine's server.
pub fn client(machine: &MachineConfig) -> Client {
    Client::with_connector(
        format!("ssh:{}", machine.ssh),
        Arc::new(SshConnector::new(
            &machine.ssh,
            machine.server_path.as_deref(),
        )),
    )
}

/// Whether the server of a machine answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Connected {
        server_version: String,
    },
    /// Not reachable; `error` is what ssh or the connection said.
    Offline {
        error: String,
    },
    /// The server speaks another protocol.
    Incompatible {
        server_version: String,
        server_protocol: u32,
    },
}

/// The state from the answer of a `ping`.
pub fn classify(ping: Result<PingResult, ClientError>) -> State {
    match ping {
        Ok(p) if p.protocol == PROTOCOL_VERSION => State::Connected {
            server_version: p.version,
        },
        Ok(p) => State::Incompatible {
            server_version: p.version,
            server_protocol: p.protocol,
        },
        Err(e) => State::Offline {
            error: error_text(&e),
        },
    }
}

/// ssh's own message (the connector puts it behind `ssh: `), else the client error.
fn error_text(e: &ClientError) -> String {
    match e {
        ClientError::Io(e) | ClientError::Connect(e) => e.to_string(),
        other => other.to_string(),
    }
}

/// `ping` the machine's server.
pub fn check(machine: &MachineConfig) -> State {
    classify(client(machine).ping())
}

impl State {
    pub fn tag(&self) -> &'static str {
        match self {
            State::Connected { .. } => "connected",
            State::Offline { .. } => "offline",
            State::Incompatible { .. } => "incompatible",
        }
    }

    /// The state line, as the app's tag: `connected: server 0.36.1`, `offline: <ssh error>`,
    /// `incompatible: server 0.1.0 (protocol 1), poimen 0.36.1 (protocol 6)`.
    pub fn label(&self) -> String {
        match self {
            State::Connected { server_version } => format!("connected: server {server_version}"),
            State::Offline { error } => format!("offline: {error}"),
            State::Incompatible {
                server_version,
                server_protocol,
            } => format!(
                "incompatible: server {server_version} (protocol {server_protocol}), poimen {} (protocol {PROTOCOL_VERSION})",
                env!("CARGO_PKG_VERSION"),
            ),
        }
    }

    /// Why a command cannot run on the machine: the label and one sentence of advice (with the
    /// command to copy, when there is one). `None` when the server is connected.
    pub fn failure(&self, machine: &MachineConfig) -> Option<String> {
        if matches!(self, State::Connected { .. }) {
            return None;
        }
        let mut text = format!("machine {}: {}", machine.name, self.label());
        if let Some((advice, command)) = self.hint(machine) {
            text.push('\n');
            text.push_str(&advice);
            if let Some(command) = command {
                text.push_str("\n  ");
                text.push_str(&command);
            }
        }
        Some(text)
    }

    /// The advice of `machineHint` in `core/machines.ts`, without the buttons of the app.
    pub fn hint(&self, machine: &MachineConfig) -> Option<(String, Option<String>)> {
        let (name, ssh) = (&machine.name, &machine.ssh);
        let target = quote(ssh);
        match self {
            State::Connected { .. } => None,
            State::Incompatible {
                server_protocol, ..
            } => Some((
                if *server_protocol < PROTOCOL_VERSION {
                    format!(
                        "The server on {name} is older than this poimen. Install the same version on it, then try again."
                    )
                } else {
                    format!(
                        "The server on {name} is newer than this poimen. Update poimen, then try again."
                    )
                },
                None,
            )),
            State::Offline { error } => {
                let e = error.to_lowercase();
                let has = |s: &str| e.contains(s);
                Some(
                    if has("remote host identification has changed") || has("offending") {
                        (
                            format!(
                                "The host key of {ssh} has changed; if you expect that, remove the old key, then try again."
                            ),
                            Some(format!("ssh-keygen -R {}", quote(host_of(ssh)))),
                        )
                    } else if has("host key") {
                        (
                            format!(
                                "The host key of {ssh} is not known yet; connect once in a terminal, check the fingerprint and accept it, then try again."
                            ),
                            Some(format!("ssh {target}")),
                        )
                    } else if has("passphrase") {
                        (
                            format!(
                                "The key for {ssh} needs its passphrase; add it to the ssh agent, then try again."
                            ),
                            Some("ssh-add".into()),
                        )
                    } else if has("permission denied") {
                        (
                            format!(
                                "ssh refused the login to {ssh}; if your key has a passphrase, add it to the ssh agent (poimen never asks for a password), then try again."
                            ),
                            Some("ssh-add".into()),
                        )
                    } else if has("no route to host") || has("network is unreachable") {
                        (
                            format!(
                                "There is no route to {ssh}; check the network or VPN, then try again."
                            ),
                            Some(format!("ssh -v {target}")),
                        )
                    } else if has("connection refused") {
                        (
                            format!(
                                "{ssh} refused the connection; check that sshd runs there and the port is right, then try again."
                            ),
                            Some(format!("ssh -v {target}")),
                        )
                    } else if has("command not found")
                        || has("not found")
                        || has("no such file")
                        || has("poimen-server")
                    {
                        (
                            format!(
                                "ssh works, but poimen-server is not installed on {name} or not on its PATH. Install it on the machine (the app's Install server… does it), then try again."
                            ),
                            Some(format!("ssh {target} 'command -v poimen-server'")),
                        )
                    } else {
                        (
                            format!(
                                "{name} cannot be reached. Check that ssh works without a prompt (network, VPN, host name), then try again."
                            ),
                            Some(format!("ssh {target}")),
                        )
                    },
                )
            }
        }
    }
}

/// Pings the machine and returns the message to print when its server cannot be used.
pub fn require(machine: &MachineConfig) -> Result<(), String> {
    match check(machine).failure(machine) {
        Some(message) => Err(message),
        None => Ok(()),
    }
}

/// The host of an ssh target without its `user@`.
fn host_of(target: &str) -> &str {
    target.rsplit('@').next().unwrap_or(target)
}

fn quote(s: &str) -> String {
    let safe = |c: char| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c);
    if !s.is_empty() && s.chars().all(safe) {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// One machine of `poimen machines --json`.
pub fn status_json(machine: &MachineConfig, state: &State) -> Value {
    let mut v = json!({"name": machine.name, "ssh": machine.ssh, "state": state.tag()});
    match state {
        State::Connected { server_version } => v["serverVersion"] = json!(server_version),
        State::Offline { error } => v["error"] = json!(error),
        State::Incompatible {
            server_version,
            server_protocol,
        } => {
            v["serverVersion"] = json!(server_version);
            v["serverProtocol"] = json!(server_protocol);
            v["appProtocol"] = json!(PROTOCOL_VERSION);
        }
    }
    v
}

/// `poimen machines`: one line per machine, checked in parallel (each ssh bounds itself with
/// `ConnectTimeout`).
pub fn list(
    machines: &[MachineConfig],
    json: bool,
    out: &mut impl std::io::Write,
) -> std::io::Result<()> {
    let states: Vec<State> = std::thread::scope(|s| {
        let handles: Vec<_> = machines.iter().map(|m| s.spawn(move || check(m))).collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or(State::Offline {
                    error: "the check failed".into(),
                })
            })
            .collect()
    });
    if json {
        let rows: Vec<Value> = machines
            .iter()
            .zip(&states)
            .map(|(m, s)| status_json(m, s))
            .collect();
        return writeln!(out, "{}", Value::Array(rows));
    }
    if machines.is_empty() {
        return writeln!(out, "no machines (add `machines` to config.json)");
    }
    let width = machines.iter().map(|m| m.name.len()).max().unwrap_or(0);
    for (m, s) in machines.iter().zip(&states) {
        writeln!(out, "{:width$}  {}  {}", m.name, m.ssh, s.label())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine() -> MachineConfig {
        MachineConfig {
            name: "box".into(),
            ssh: "me@box.lan".into(),
            repos_dir: "~/Source/{owner}/{repo}".into(),
            server_path: None,
        }
    }

    fn ping(protocol: u32) -> PingResult {
        PingResult {
            version: "0.1.0".into(),
            protocol,
            pid: 1,
            clients: Vec::new(),
        }
    }

    #[test]
    fn machines_are_read_from_the_user_config_with_warnings() {
        let home = std::env::temp_dir().join(format!("poimen-remote-{}", std::process::id()));
        let path = poimen_core::settings::config_path(&home);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"machines":[{"name":"box","ssh":"me@box"},{"name":"bad","ssh":"-oProxy"}]}"#,
        )
        .unwrap();
        let (machines, warnings) = load(Some(&home)).unwrap();
        assert_eq!(machines.len(), 1);
        assert_eq!(machines[0].ssh, "me@box");
        assert_eq!(warnings.len(), 1);
        std::fs::write(&path, "{").unwrap();
        assert!(load(Some(&home)).is_err());
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(load(Some(&home)).unwrap().0, Vec::new());
    }

    #[test]
    fn an_unknown_machine_names_the_configured_ones() {
        assert!(find(&[], "x").unwrap_err().contains("empty"));
        let err = find(&[machine()], "x").unwrap_err();
        assert!(err.contains("configured: box"), "{err}");
        assert_eq!(find(&[machine()], "box").unwrap().ssh, "me@box.lan");
    }

    #[test]
    fn a_ping_is_connected_incompatible_or_offline() {
        assert_eq!(
            classify(Ok(ping(PROTOCOL_VERSION))),
            State::Connected {
                server_version: "0.1.0".into()
            }
        );
        let old = classify(Ok(ping(PROTOCOL_VERSION - 1)));
        assert_eq!(old.tag(), "incompatible");
        let text = old.failure(&machine()).unwrap();
        assert!(text.contains("is older than this poimen"), "{text}");
        assert!(
            text.contains("incompatible: server 0.1.0 (protocol"),
            "{text}"
        );
        let new = classify(Ok(ping(PROTOCOL_VERSION + 1)));
        assert!(
            new.failure(&machine())
                .unwrap()
                .contains("is newer than this poimen")
        );
        let down = classify(Err(ClientError::Io(std::io::Error::other("ssh: boom"))));
        assert_eq!(
            down,
            State::Offline {
                error: "ssh: boom".into()
            }
        );
    }

    #[test]
    fn a_missing_server_and_ssh_failures_get_the_apps_advice() {
        let offline = |e: &str| State::Offline { error: e.into() };
        let m = machine();
        let missing = offline("ssh: bash: poimen-server: command not found")
            .failure(&m)
            .unwrap();
        assert!(missing.contains("poimen-server is not installed on box or not on its PATH"));
        assert!(missing.contains("ssh me@box.lan 'command -v poimen-server'"));
        let key = offline("ssh: Permission denied (publickey).")
            .failure(&m)
            .unwrap();
        assert!(key.contains("ssh-add"));
        let changed = offline("WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!")
            .failure(&m)
            .unwrap();
        assert!(changed.contains("ssh-keygen -R box.lan"));
        let other = offline("ssh: Could not resolve hostname")
            .failure(&m)
            .unwrap();
        assert!(other.contains("box cannot be reached"));
        assert!(
            State::Connected {
                server_version: "1".into()
            }
            .failure(&m)
            .is_none()
        );
    }

    #[test]
    fn the_list_has_a_line_per_machine_and_json() {
        let mut m = machine();
        // A target ssh cannot resolve is offline; no network is needed to say so.
        m.ssh = "poimen-test-host.invalid".into();
        let mut out = Vec::new();
        list(&[], false, &mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("no machines"));
        let v = status_json(&m, &State::Offline { error: "x".into() });
        assert_eq!(v["state"], "offline");
        assert_eq!(v["error"], "x");
        let v = status_json(
            &m,
            &State::Connected {
                server_version: "1".into(),
            },
        );
        assert_eq!(v["serverVersion"], "1");
    }
}
