//! The project file `.poimen/config.json` at the repository root: `layout`, `setup`, `teardown`,
//! `commands[]` and `presets[]` (schema `docs/project-config.schema.json`). Parsing is tolerant:
//! an invalid entry is dropped with a warning naming file and key, the rest loads. Nothing here
//! runs a command.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::layout::{Layout, parse_layout_value};

/// The project file, relative to the repository root.
pub const PROJECT_FILE: &str = ".poimen/config.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Terminal,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProjectCommand {
    pub name: String,
    /// The command for the current platform.
    pub command: String,
    pub target: Target,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Preset {
    pub name: String,
    pub layout: Layout,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProjectConfig {
    pub layout: Option<Layout>,
    pub setup: Option<String>,
    pub teardown: Option<String>,
    pub commands: Vec<ProjectCommand>,
    pub presets: Vec<Preset>,
    /// `tasks`: modes and `worktreeDir` that override the user's (merged by name).
    pub tasks: Option<crate::settings::TaskOverride>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Loaded {
    pub config: ProjectConfig,
    pub warnings: Vec<String>,
}

/// Label of the pane that runs the `setup` command.
pub const SETUP_LABEL: &str = "setup";

/// Runs `command` visibly in a new tab of the workspace, labelled `setup`, in `cwd`: the tab is
/// created, named, and the command typed with Enter. The caller decides whether it may run (trust).
pub fn start_setup(
    request: &impl Fn(&str, Value) -> Result<Value, String>,
    workspace_id: &str,
    cwd: &str,
    command: &str,
) -> Result<(), String> {
    let result = request(
        "tab.create",
        crate::herdr::tab_create_params(workspace_id, Some(cwd)),
    )?;
    let pane = crate::herdr::created_pane_id(&result).ok_or("herdr did not report the new tab")?;
    request(
        "pane.rename",
        crate::herdr::pane_rename_params(&pane, Some(SETUP_LABEL)),
    )?;
    request(
        "pane.send_input",
        crate::herdr::send_input_params(&pane, command),
    )?;
    Ok(())
}

/// The `setup` command of the project file for the current platform; `None` without one.
pub fn setup_of(project: Option<(&str, &str)>) -> Option<String> {
    let (path, text) = project?;
    parse(path, text, platform()).config.setup
}

/// The platform key used in per-platform commands.
pub fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

/// A command value: a string, or an object picked by `platform` with `default` as fallback.
/// `Ok(None)` means nothing for this platform.
fn command_for(value: &Value, platform: &str) -> Result<Option<String>, String> {
    let text = |v: &Value| -> Result<String, String> {
        match v.as_str().map(str::trim) {
            Some(s) if !s.is_empty() => Ok(s.to_owned()),
            _ => Err("a command must be a non-empty string".to_owned()),
        }
    };
    match value {
        Value::String(_) => text(value).map(Some),
        Value::Object(map) => match map.get(platform).or_else(|| map.get("default")) {
            Some(v) => text(v).map(Some),
            None => Ok(None),
        },
        _ => Err("a command must be a string or an object".to_owned()),
    }
}

/// The unique, non-empty `name` of a list entry, or the warning to record.
fn entry_name(raw: &Value, names: &mut HashSet<String>) -> Result<String, String> {
    let name = raw["name"].as_str().map(str::trim).unwrap_or_default();
    if name.is_empty() {
        return Err("\"name\" is required".into());
    }
    if !names.insert(name.to_owned()) {
        return Err(format!("duplicate name \"{name}\""));
    }
    Ok(name.to_owned())
}

/// Parses the text of a project file for `platform`; `file` names it in warnings.
pub fn parse(file: &str, text: &str, platform: &str) -> Loaded {
    let mut out = Loaded::default();
    let data: Value = match serde_json::from_str(text) {
        Ok(v @ Value::Object(_)) => v,
        _ => {
            out.warnings
                .push(format!("{file}: invalid JSON (expected an object)"));
            return out;
        }
    };
    let warn = |out: &mut Loaded, key: &str, msg: String| {
        out.warnings.push(format!("{file}: {key}: {msg}"));
    };

    if !data["layout"].is_null() {
        match parse_layout_value(&data["layout"]) {
            Ok(l) => out.config.layout = Some(l),
            Err(e) => warn(&mut out, "layout", e),
        }
    }
    for key in ["setup", "teardown"] {
        if data[key].is_null() {
            continue;
        }
        match command_for(&data[key], platform) {
            Ok(Some(c)) => {
                if key == "setup" {
                    out.config.setup = Some(c);
                } else {
                    out.config.teardown = Some(c);
                }
            }
            Ok(None) => warn(&mut out, key, format!("no command for {platform}; skipped")),
            Err(e) => warn(&mut out, key, e),
        }
    }

    if !data["tasks"].is_null() {
        let mut warnings = Vec::new();
        let tasks = crate::settings::parse_tasks(&data["tasks"], "ignored", &mut warnings);
        out.warnings
            .extend(warnings.into_iter().map(|w| format!("{file}: {w}")));
        if tasks.modes.is_some() || tasks.worktree_dir.is_some() {
            out.config.tasks = Some(tasks);
        }
    }

    let mut names: HashSet<String> = HashSet::new();
    match &data["commands"] {
        Value::Null => {}
        Value::Array(list) => {
            for (i, raw) in list.iter().enumerate() {
                let key = format!("commands[{i}]");
                let name = match entry_name(raw, &mut names) {
                    Ok(n) => n,
                    Err(e) => {
                        warn(&mut out, &key, e);
                        continue;
                    }
                };
                let target = match raw["target"].as_str() {
                    Some("terminal") => Target::Terminal,
                    Some("agent") => Target::Agent,
                    _ => {
                        let msg = format!("\"{name}\": target must be \"terminal\" or \"agent\"");
                        warn(&mut out, &key, msg);
                        continue;
                    }
                };
                match command_for(&raw["command"], platform) {
                    Ok(Some(command)) => out.config.commands.push(ProjectCommand {
                        name,
                        command,
                        target,
                    }),
                    Ok(None) => {
                        let msg = format!("\"{name}\": no command for {platform}; skipped");
                        warn(&mut out, &key, msg);
                    }
                    Err(e) => warn(&mut out, &key, format!("\"{name}\": {e}")),
                }
            }
        }
        _ => warn(&mut out, "commands", "expected a list".into()),
    }

    let mut names: HashSet<String> = HashSet::new();
    match &data["presets"] {
        Value::Null => {}
        Value::Array(list) => {
            for (i, raw) in list.iter().enumerate() {
                let key = format!("presets[{i}]");
                let name = match entry_name(raw, &mut names) {
                    Ok(n) => n,
                    Err(e) => {
                        warn(&mut out, &key, e);
                        continue;
                    }
                };
                match parse_layout_value(raw) {
                    Ok(layout) => out.config.presets.push(Preset { name, layout }),
                    Err(e) => warn(&mut out, &key, format!("\"{name}\": {e}")),
                }
            }
        }
        _ => warn(&mut out, "presets", "expected a list".into()),
    }
    out
}

/// Loads `<repo_root>/.poimen/config.json`; a missing file is an empty config without warnings.
pub fn load(repo_root: &str) -> Loaded {
    let path = Path::new(repo_root).join(PROJECT_FILE);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&path.to_string_lossy(), &text, platform()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Loaded::default(),
        Err(e) => Loaded {
            warnings: vec![format!("{}: {e}", path.to_string_lossy())],
            ..Loaded::default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "/work/app/.poimen/config.json";

    const FULL: &str = r#"{
        "layout": {"panes": [{"name": "agent", "command": "claude"},
                             {"name": "sh", "split": {"direction": "down"}}]},
        "setup": "pnpm install",
        "teardown": {"default": "echo bye", "windows": "echo bye-win"},
        "commands": [
            {"name": "test", "command": "pnpm test", "target": "terminal"},
            {"name": "review", "command": "/review", "target": "agent"}
        ],
        "presets": [{"name": "duo", "panes": [{"name": "a"}, {"name": "b", "split": {"direction": "right"}}]}]
    }"#;

    #[test]
    fn parses_every_key() {
        let l = parse(FILE, FULL, "macos");
        assert!(l.warnings.is_empty(), "{:?}", l.warnings);
        let c = l.config;
        assert_eq!(c.layout.unwrap().panes.len(), 2);
        assert_eq!(c.setup.as_deref(), Some("pnpm install"));
        assert_eq!(c.teardown.as_deref(), Some("echo bye"));
        assert_eq!(c.commands.len(), 2);
        assert_eq!(c.commands[0].name, "test");
        assert_eq!(c.commands[0].target, Target::Terminal);
        assert_eq!(c.commands[1].target, Target::Agent);
        assert_eq!(c.presets[0].name, "duo");
        assert_eq!(c.presets[0].layout.panes.len(), 2);
    }

    #[test]
    fn picks_the_command_by_platform_with_default_fallback() {
        let l = parse(FILE, FULL, "windows");
        assert_eq!(l.config.teardown.as_deref(), Some("echo bye-win"));
        let text = r#"{"commands":[{"name":"x","target":"terminal",
            "command":{"default":"d","linux":"l","macos":"m"}}]}"#;
        for (platform, want) in [("linux", "l"), ("macos", "m"), ("windows", "d")] {
            let l = parse(FILE, text, platform);
            assert_eq!(l.config.commands[0].command, want, "{platform}");
        }
    }

    #[test]
    fn a_command_missing_for_the_platform_is_skipped_with_a_warning() {
        let text = r#"{"setup":{"linux":"make"},
            "commands":[{"name":"x","target":"agent","command":{"linux":"l"}},
                        {"name":"y","target":"agent","command":"ok"}]}"#;
        let l = parse(FILE, text, "macos");
        assert_eq!(l.config.setup, None);
        assert_eq!(l.config.commands.len(), 1);
        assert_eq!(l.config.commands[0].name, "y");
        assert_eq!(l.warnings.len(), 2);
        assert!(l.warnings[0].starts_with(&format!("{FILE}: setup")));
        assert!(l.warnings[1].contains("commands[0]") && l.warnings[1].contains("macos"));
    }

    #[test]
    fn invalid_entries_are_dropped_and_the_rest_loads() {
        let text = r#"{
            "layout": {"panes": []},
            "setup": 3,
            "teardown": "ok",
            "commands": [
                {"command": "x", "target": "agent"},
                {"name": "a", "command": "x", "target": "agent"},
                {"name": "a", "command": "y", "target": "agent"},
                {"name": "b", "command": "x", "target": "other"},
                {"name": "c", "command": 5, "target": "terminal"},
                {"name": "d", "command": "x", "target": "terminal"}
            ],
            "presets": [{"name": "p", "panes": []}, {"panes": [{"name": "a"}]},
                        {"name": "q", "panes": [{"name": "a"}]},
                        {"name": "q", "panes": [{"name": "a"}]}]
        }"#;
        let l = parse(FILE, text, "linux");
        assert_eq!(l.config.layout, None);
        assert_eq!(l.config.setup, None);
        assert_eq!(l.config.teardown.as_deref(), Some("ok"));
        let names: Vec<_> = l.config.commands.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["a", "d"]);
        let presets: Vec<_> = l.config.presets.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(presets, ["q"]);
        assert_eq!(l.warnings.len(), 9, "{:#?}", l.warnings);
        assert!(l.warnings.iter().all(|w| w.starts_with(FILE)));
        assert!(l.warnings.iter().any(|w| w.contains("presets[0]")));
    }

    #[test]
    fn invalid_json_is_one_warning_and_an_empty_config() {
        let l = parse(FILE, "{nope", "linux");
        assert_eq!(l.config, ProjectConfig::default());
        assert_eq!(l.warnings.len(), 1);
        assert!(l.warnings[0].contains(FILE));
    }

    #[test]
    fn load_reads_the_file_and_a_missing_file_is_empty() {
        let dir = std::env::temp_dir().join(format!("poimen-project-{}", std::process::id()));
        let root = dir.to_string_lossy().into_owned();
        assert_eq!(load(&root), Loaded::default());
        std::fs::create_dir_all(dir.join(".poimen")).unwrap();
        std::fs::write(dir.join(PROJECT_FILE), r#"{"setup":"make"}"#).unwrap();
        assert_eq!(load(&root).config.setup.as_deref(), Some("make"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
