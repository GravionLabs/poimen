//! Registers `poimen mcp` (the MCP server of the console binary, ADR 0012) in the MCP config of an
//! agent, next to its hooks (#614): `integration install|uninstall|status` of Claude Code
//! (`~/.claude.json`, user scope) and Copilot CLI (`~/.copilot/mcp-config.json`) do both.
//!
//! The entry is `mcpServers.<poimen|poimen-rc>` with `args: ["mcp"]` and the console binary as
//! command. That name, the arguments and the binary's name together identify an entry as ours:
//! an entry of the same name with anything else is the user's and is never changed or removed,
//! and the other channel's entry (another name) is never touched.

use std::path::{Path, PathBuf};

use poimen_protocol::Channel;
use serde_json::{Map, Value, json};

use crate::claude::{read_settings, write_atomic};

/// How an agent spells the entry and where its config lives.
#[derive(Clone, Copy)]
pub struct McpKind {
    pub default_path: fn(&Path) -> PathBuf,
    pub entry: fn(&Path) -> Value,
}

/// Claude Code reads user-scope MCP servers from `~/.claude.json`.
pub const CLAUDE: McpKind = McpKind {
    default_path: |home| home.join(".claude.json"),
    entry: |exe| json!({"type": "stdio", "command": exe.display().to_string(), "args": ["mcp"], "env": {}}),
};

/// Copilot CLI: `~/.copilot/mcp-config.json`, local servers list the tools they offer.
pub const COPILOT: McpKind = McpKind {
    default_path: |home| home.join(".copilot").join("mcp-config.json"),
    entry: |exe| json!({"type": "local", "command": exe.display().to_string(), "args": ["mcp"], "tools": ["*"]}),
};

/// What `status` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpState {
    Installed,
    Missing,
}

/// The server's name in `mcpServers`: `poimen`, on rc `poimen-rc`.
pub fn server_name(channel: Channel) -> String {
    channel.named("poimen")
}

/// File stem of the console binary: `poimen`, on rc `poimen-cli-rc` (the rc sidecar name).
pub const fn console_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => "poimen",
        Channel::Rc => "poimen-cli-rc",
    }
}

/// The console binary: next to `server_exe`, else on `path_var`. `None` when there is none.
pub fn console_exe(
    server_exe: &Path,
    channel: Channel,
    path_var: Option<&std::ffi::OsStr>,
) -> Option<PathBuf> {
    let file = format!("{}{}", console_name(channel), std::env::consts::EXE_SUFFIX);
    let beside = server_exe.parent().map(|d| d.join(&file));
    beside.into_iter().find(|p| p.is_file()).or_else(|| {
        std::env::split_paths(path_var?)
            .map(|d| d.join(&file))
            .find(|p| p.is_file())
    })
}

fn is_ours(entry: &Value, channel: Channel) -> bool {
    let args_ok = entry
        .get("args")
        .and_then(Value::as_array)
        .is_some_and(|a| a.len() == 1 && a[0].as_str() == Some("mcp"));
    let stem_ok = entry
        .get("command")
        .and_then(Value::as_str)
        .and_then(|c| Path::new(c).file_stem())
        .is_some_and(|s| s.to_str() == Some(console_name(channel)));
    args_ok && stem_ok
}

fn servers_mut(config: &mut Map<String, Value>) -> Result<&mut Map<String, Value>, String> {
    config
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "\"mcpServers\" is not an object; left untouched".to_owned())
}

/// Writes our entry into `path`, keeping everything else. Idempotent. An entry of the same name
/// that is not ours is an error and nothing is written.
pub fn install(path: &Path, kind: McpKind, exe: &Path, channel: Channel) -> Result<(), String> {
    let mut config = read_settings(path)?;
    let name = server_name(channel);
    let servers = servers_mut(&mut config)?;
    if servers.get(&name).is_some_and(|e| !is_ours(e, channel)) {
        return Err(format!(
            "mcpServers.{name} in {} is not poimen's; left untouched",
            path.display()
        ));
    }
    servers.insert(name, (kind.entry)(exe));
    write_atomic(path, &config)
}

/// Removes only our entry (and the `mcpServers` object when that leaves it empty). Returns
/// whether anything was removed; a missing file stays missing.
pub fn uninstall(path: &Path, channel: Channel) -> Result<bool, String> {
    let mut config = read_settings(path)?;
    let name = server_name(channel);
    let Some(servers) = config.get_mut("mcpServers").and_then(Value::as_object_mut) else {
        return Ok(false);
    };
    if !servers.get(&name).is_some_and(|e| is_ours(e, channel)) {
        return Ok(false);
    }
    servers.remove(&name);
    if servers.is_empty() {
        config.remove("mcpServers");
    }
    write_atomic(path, &config)?;
    Ok(true)
}

/// Installed when our entry is there and its binary exists.
pub fn status(path: &Path, channel: Channel) -> Result<McpState, String> {
    let config = read_settings(path)?;
    let live = config
        .get("mcpServers")
        .and_then(|s| s.get(server_name(channel)))
        .filter(|e| is_ours(e, channel))
        .and_then(|e| e.get("command").and_then(Value::as_str))
        .is_some_and(|c| Path::new(c).is_file());
    Ok(if live {
        McpState::Installed
    } else {
        McpState::Missing
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("poimen-mcpcfg-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Dir(dir)
        }
        fn file(&self, name: &str) -> PathBuf {
            self.0.join(name)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn console(dir: &Dir, channel: Channel) -> PathBuf {
        let p = dir.file(&format!(
            "{}{}",
            console_name(channel),
            std::env::consts::EXE_SUFFIX
        ));
        std::fs::write(&p, "").unwrap();
        p
    }

    fn read(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn install_adds_the_entry_and_keeps_the_rest_of_the_file() {
        let dir = Dir::new("keep");
        let exe = console(&dir, Channel::Stable);
        let path = dir.file("claude.json");
        std::fs::write(
            &path,
            r#"{"numStartups": 7, "mcpServers": {"other": {"command": "x", "args": ["y"]}}}"#,
        )
        .unwrap();
        install(&path, CLAUDE, &exe, Channel::Stable).unwrap();
        let v = read(&path);
        assert_eq!(v["numStartups"], 7);
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"]["poimen"]["args"], json!(["mcp"]));
        assert_eq!(
            v["mcpServers"]["poimen"]["command"],
            exe.display().to_string()
        );
        assert_eq!(status(&path, Channel::Stable), Ok(McpState::Installed));
        // Idempotent.
        install(&path, CLAUDE, &exe, Channel::Stable).unwrap();
        assert_eq!(read(&path)["mcpServers"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn copilot_spells_a_local_server_with_all_tools() {
        let dir = Dir::new("copilot");
        let exe = console(&dir, Channel::Stable);
        let path = dir.file("mcp-config.json");
        install(&path, COPILOT, &exe, Channel::Stable).unwrap();
        let v = read(&path);
        assert_eq!(v["mcpServers"]["poimen"]["type"], "local");
        assert_eq!(v["mcpServers"]["poimen"]["tools"], json!(["*"]));
    }

    #[test]
    fn channels_coexist_and_each_manages_only_its_own() {
        let dir = Dir::new("channels");
        let stable = console(&dir, Channel::Stable);
        let rc = console(&dir, Channel::Rc);
        let path = dir.file("claude.json");
        install(&path, CLAUDE, &stable, Channel::Stable).unwrap();
        install(&path, CLAUDE, &rc, Channel::Rc).unwrap();
        assert_eq!(
            read(&path)["mcpServers"]["poimen-rc"]["args"],
            json!(["mcp"])
        );
        assert!(uninstall(&path, Channel::Rc).unwrap());
        let v = read(&path);
        assert!(v["mcpServers"].get("poimen-rc").is_none());
        assert!(v["mcpServers"].get("poimen").is_some());
        assert_eq!(status(&path, Channel::Rc), Ok(McpState::Missing));
        assert_eq!(status(&path, Channel::Stable), Ok(McpState::Installed));
        assert!(uninstall(&path, Channel::Stable).unwrap());
        assert!(read(&path).get("mcpServers").is_none());
    }

    #[test]
    fn an_entry_of_the_users_with_our_name_is_never_touched() {
        let dir = Dir::new("foreign");
        let exe = console(&dir, Channel::Stable);
        let path = dir.file("claude.json");
        let text = r#"{"mcpServers": {"poimen": {"command": "/opt/mine", "args": ["serve"]}}}"#;
        std::fs::write(&path, text).unwrap();
        assert!(install(&path, CLAUDE, &exe, Channel::Stable).is_err());
        assert!(!uninstall(&path, Channel::Stable).unwrap());
        assert_eq!(status(&path, Channel::Stable), Ok(McpState::Missing));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn a_missing_file_stays_missing_and_garbage_is_left_alone() {
        let dir = Dir::new("edge");
        let path = dir.file("none.json");
        assert!(!uninstall(&path, Channel::Stable).unwrap());
        assert!(!path.exists());
        assert_eq!(status(&path, Channel::Stable), Ok(McpState::Missing));
        let bad = dir.file("bad.json");
        std::fs::write(&bad, "[1]").unwrap();
        assert!(install(&bad, CLAUDE, &dir.file("poimen"), Channel::Stable).is_err());
        assert_eq!(std::fs::read_to_string(&bad).unwrap(), "[1]");
        let odd = dir.file("odd.json");
        std::fs::write(&odd, r#"{"mcpServers": 3}"#).unwrap();
        assert!(install(&odd, CLAUDE, &dir.file("poimen"), Channel::Stable).is_err());
    }

    #[test]
    fn an_entry_whose_binary_is_gone_is_not_installed() {
        let dir = Dir::new("gone");
        let exe = console(&dir, Channel::Stable);
        let path = dir.file("claude.json");
        install(&path, CLAUDE, &exe, Channel::Stable).unwrap();
        std::fs::remove_file(&exe).unwrap();
        assert_eq!(status(&path, Channel::Stable), Ok(McpState::Missing));
    }

    #[test]
    fn the_console_binary_is_found_beside_the_server_else_on_path() {
        let dir = Dir::new("find");
        let server = dir.file("poimen-server");
        assert_eq!(console_exe(&server, Channel::Stable, None), None);
        let other = Dir::new("find-path");
        let on_path = console(&other, Channel::Stable);
        let var = std::env::join_paths([&other.0]).unwrap();
        assert_eq!(
            console_exe(&server, Channel::Stable, Some(&var)),
            Some(on_path)
        );
        let beside = console(&dir, Channel::Stable);
        assert_eq!(
            console_exe(&server, Channel::Stable, Some(&var)),
            Some(beside)
        );
        assert_eq!(console_exe(&server, Channel::Rc, None), None);
    }
}
