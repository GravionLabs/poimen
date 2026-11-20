//! The collector of `poimen doctor`: reads the facts from the machine, changes nothing.

use std::path::{Path, PathBuf};

use poimen_client::{Client, ClientError};
use poimen_core::gui_path;

use super::{
    AgentProbe, FileProbe, Hook, HostProbe, Inputs, Integration, ServerProbe, StateProbe, ToolProbe,
};

/// Runs a program and returns its stdout; faked in tests.
pub trait Runner {
    fn run(&self, program: &Path, args: &[&str]) -> Result<String, String>;
}

/// Runs the real program with the login-shell `PATH`.
pub struct SystemRunner {
    pub path: String,
}

impl Runner for SystemRunner {
    fn run(&self, program: &Path, args: &[&str]) -> Result<String, String> {
        let mut command = gui_path::command(program);
        command.args(args);
        if !cfg!(windows) {
            command.env("PATH", &self.path);
        }
        let output = command.output().map_err(|e| e.to_string())?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
        }
    }
}

/// Where the collector looks.
pub struct Env<'a> {
    pub client: &'a Client,
    pub state_file: PathBuf,
    /// The directory of the console binary: `poimen-server` ships next to it.
    pub bin_dir: Option<PathBuf>,
    /// The login-shell `PATH`.
    pub path: String,
    pub version: String,
}

/// `poimen-server` next to the console, else on `path`.
pub fn server_binary(env: &Env<'_>) -> Option<PathBuf> {
    let name = poimen_protocol::CHANNEL.server_program(cfg!(windows));
    env.bin_dir
        .as_ref()
        .map(|d| d.join(&name))
        .filter(|p| p.is_file())
        .or_else(|| gui_path::resolve_program(&name, &env.path))
}

pub fn collect(env: &Env<'_>, runner: &dyn Runner) -> Inputs {
    let server_bin = server_binary(env);
    Inputs {
        version: env.version.clone(),
        channel: poimen_protocol::CHANNEL.name().to_string(),
        protocol: poimen_protocol::PROTOCOL_VERSION,
        server: match env.client.ping() {
            Ok(p) => ServerProbe::Answers {
                version: p.version,
                protocol: p.protocol,
            },
            Err(e) if e.is_not_running() => ServerProbe::NotRunning,
            Err(e) => ServerProbe::Failed(describe(&e)),
        },
        host: host(server_bin.as_deref(), runner),
        state: state(&env.state_file),
        agents: poimen_protocol::HOOK_KINDS
            .iter()
            .map(|kind| AgentProbe {
                kind: (*kind).to_string(),
                cli: gui_path::resolve_program(kind, &env.path).is_some(),
                integration: integration(server_bin.as_deref(), kind, runner),
            })
            .collect(),
        tools: ["gh", "git"]
            .iter()
            .map(|name| ToolProbe {
                name: (*name).to_string(),
                found: gui_path::resolve_program(name, &env.path).is_some(),
            })
            .collect(),
    }
}

fn describe(e: &ClientError) -> String {
    e.to_string()
}

fn host(server_bin: Option<&Path>, runner: &dyn Runner) -> HostProbe {
    let Some(server) = server_bin else {
        return HostProbe::NoServer;
    };
    let name = poimen_protocol::CHANNEL.host_program(cfg!(windows));
    let host = server.with_file_name(name);
    if !host.is_file() {
        return HostProbe::Missing;
    }
    match runner.run(&host, &["--version"]) {
        Ok(out) => HostProbe::Version(out.lines().next().unwrap_or("").trim().to_string()),
        Err(e) => HostProbe::Failed(e),
    }
}

fn integration(
    server_bin: Option<&Path>,
    kind: &str,
    runner: &dyn Runner,
) -> Result<Integration, String> {
    let server = server_bin.ok_or("poimen-server was not found")?;
    let out = runner.run(server, &["integration", "status", kind])?;
    Ok(parse_integration(&out))
}

fn hook_state(text: &str) -> Hook {
    match text.trim() {
        "installed" => Hook::Installed,
        "not installed" => Hook::Missing,
        t if t.starts_with("partially installed") => Hook::Partial,
        _ => Hook::Unknown,
    }
}

/// The output of `integration status <kind>`: the hooks on the first line, `mcp: ...` after it.
pub fn parse_integration(stdout: &str) -> Integration {
    let mcp = stdout
        .lines()
        .find_map(|l| l.trim().strip_prefix("mcp:"))
        .map(str::trim)
        .filter(|m| *m != "unavailable")
        .map(hook_state);
    Integration {
        hooks: hook_state(stdout.lines().next().unwrap_or("")),
        mcp,
    }
}

/// Reads `server.json` and what lies next to it.
pub fn state(path: &Path) -> StateProbe {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    // Every save leaves the previous file as `.bak`, so it is only a finding when it is what the
    // server would load: `server.json` is gone (a save was interrupted) or older than the copy.
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let backup = match modified(&dir.join(format!("{name}.bak"))) {
        None => false,
        Some(bak) => modified(path).is_none_or(|current| bak > current),
    };
    let damaged_prefix = format!("{name}.damaged-");
    let damaged = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with(&damaged_prefix))
                .count()
        })
        .unwrap_or(0);
    let file = match std::fs::read(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => FileProbe::Missing,
        Err(e) => FileProbe::Unreadable(e.to_string()),
        Ok(bytes) => match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(v) if v.is_object() => {
                let count = |key: &str| v.get(key).and_then(|a| a.as_array()).map_or(0, Vec::len);
                FileProbe::Parsed {
                    spaces: count("spaces"),
                    tabs: count("tabs"),
                    terminals: count("terminals"),
                }
            }
            Ok(_) => FileProbe::Damaged("not a JSON object".into()),
            Err(e) => FileProbe::Damaged(e.to_string()),
        },
    };
    StateProbe {
        path: path.display().to_string(),
        file,
        backup,
        damaged,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integration_status_output_is_parsed() {
        let i = parse_integration("installed\nmcp: not installed\n");
        assert_eq!((i.hooks, i.mcp), (Hook::Installed, Some(Hook::Missing)));
        let i = parse_integration("partially installed (1 of 3)\nmcp: unavailable\n");
        assert_eq!((i.hooks, i.mcp), (Hook::Partial, None));
        let i = parse_integration("???");
        assert_eq!((i.hooks, i.mcp), (Hook::Unknown, None));
    }
}
