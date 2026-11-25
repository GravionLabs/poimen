//! The `machines[]` of the user config (ADR 0008): parsing and the checks the app and the console share.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

/// `reposDir` when a machine sets none.
pub const DEFAULT_REPOS_DIR: &str = "~/Source/{owner}/{repo}";

/// One entry of `machines[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineConfig {
    /// Unique among the machines.
    pub name: String,
    /// The ssh target as in `~/.ssh/config` (a `Host` alias or `user@host`).
    pub ssh: String,
    /// Where repositories live on the machine (`{owner}`, `{repo}`).
    pub repos_dir: String,
    /// Where the server is on the machine when its non-interactive `PATH` lacks it (set by the
    /// install of #865, e.g. `$HOME/.local/bin/poimen-server`); `None` calls it by name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_path: Option<String>,
}

/// The machines of `machines[]`; an invalid entry is a warning and is left out, the rest loads.
pub fn parse(value: &Value, warnings: &mut Vec<String>) -> Vec<MachineConfig> {
    let Some(entries) = value.as_array() else {
        warnings.push("machines must be a list; no machines".into());
        return Vec::new();
    };
    let mut names = HashSet::new();
    let mut machines = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        match parse_entry(entry) {
            Ok(m) if !names.insert(m.name.clone()) => warnings.push(format!(
                "machines[{i}]: the name {:?} is already used; ignored",
                m.name
            )),
            Ok(m) => machines.push(m),
            Err(e) => warnings.push(format!("machines[{i}]: {e}; ignored")),
        }
    }
    machines
}

fn parse_entry(entry: &Value) -> Result<MachineConfig, String> {
    let Some(obj) = entry.as_object() else {
        return Err("must be an object".into());
    };
    let text = |key: &str| match obj.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.trim().to_owned())),
        None | Some(Value::Null) => Ok(None),
        Some(_) => Err(format!("{key} must be a non-empty string")),
    };
    let name = text("name")?.ok_or("name is missing")?;
    let ssh = text("ssh")?.ok_or("ssh is missing")?;
    check_target(&ssh)?;
    let repos_dir = text("reposDir")?.unwrap_or_else(|| DEFAULT_REPOS_DIR.to_owned());
    check_repos_dir(&repos_dir)?;
    let server_path = match text("serverPath")? {
        Some(path) => {
            check_server_path(&path)?;
            Some(path)
        }
        None => None,
    };
    Ok(MachineConfig {
        name,
        ssh,
        repos_dir,
        server_path,
    })
}

/// A server path is one shell word of plain path characters (`$HOME` and `~` allowed): it is
/// passed to the remote shell as the command.
pub fn check_server_path(path: &str) -> Result<(), String> {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-$~".contains(c);
    if path.starts_with('-') || !path.chars().all(plain) {
        return Err(format!("serverPath {path:?} must be a plain path"));
    }
    Ok(())
}

/// A target is one ssh argument: no option (`-...`), no whitespace or control characters.
pub fn check_target(ssh: &str) -> Result<(), String> {
    if ssh.starts_with('-') {
        return Err(format!("ssh {ssh:?} must be a host, not an option"));
    }
    if ssh.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("ssh {ssh:?} must be one host without spaces"));
    }
    Ok(())
}

/// `reposDir` may only use the placeholders `{owner}` and `{repo}`.
pub fn check_repos_dir(dir: &str) -> Result<(), String> {
    let mut rest = dir;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            return Err(format!("reposDir {dir:?} has an unclosed {{"));
        };
        let name = &after[..close];
        if !matches!(name, "owner" | "repo") {
            return Err(format!(
                "reposDir {dir:?} uses {{{name}}}; only {{owner}} and {{repo}} are known"
            ));
        }
        rest = &after[close + 1..];
    }
    Ok(())
}
