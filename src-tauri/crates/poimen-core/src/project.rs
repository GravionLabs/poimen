//! The project file `.poimen/config.json` at the repository root: `layout`, `setup`, `teardown`,
//! `commands[]` and `presets[]` (schema `docs/project-config.schema.json`). Parsing is tolerant:
//! an invalid entry is dropped with a warning naming file and key, the rest loads. Parsing runs
//! nothing; the only runner is [`run_teardown`] (child process, captured output). Callers must
//! pass `trust::gate` first.

use std::collections::HashSet;
use std::path::Path;
use std::time::Duration;

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
    /// `github.agentActions`: entries that override the user's by `id`.
    pub agent_actions: Vec<crate::agent_actions::Spec>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Loaded {
    pub config: ProjectConfig,
    pub warnings: Vec<String>,
}

/// Label of the pane that runs the `setup` command.
pub const SETUP_LABEL: &str = "setup";

/// The `setup` command of the project file for the current platform; `None` without one.
pub fn setup_of(project: Option<(&str, &str)>) -> Option<String> {
    let (path, text) = project?;
    parse(path, text, platform()).config.setup
}

/// Longest a teardown may run.
pub const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(300);
/// Most output (stdout and stderr together) that is kept.
pub const OUTPUT_CAP: usize = 1024 * 1024;

/// How a command ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunResult {
    /// Exit code; `None` when killed (timeout or signal).
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// Captured stdout and stderr, at most [`OUTPUT_CAP`] bytes.
    pub output: String,
    /// Output beyond the cap was dropped.
    pub truncated: bool,
}

impl RunResult {
    pub fn success(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }

    /// One line plus the output, for a failed run.
    pub fn failure_text(&self) -> String {
        let head = if self.timed_out {
            "the teardown command timed out".to_owned()
        } else {
            match self.exit_code {
                Some(c) => format!("the teardown command failed (exit {c})"),
                None => "the teardown command was killed".to_owned(),
            }
        };
        if self.output.trim().is_empty() {
            head
        } else {
            format!("{head}\n{}", self.output.trim_end())
        }
    }
}

/// Starts a process and collects its result; faked in tests.
pub trait ProcessRunner {
    fn run(
        &self,
        argv: &[String],
        cwd: &Path,
        timeout: Duration,
        cap: usize,
    ) -> Result<RunResult, String>;
}

/// The argv that runs `command` through the user's shell: `$SHELL -lc` on Linux and macOS
/// (`/bin/sh` without `$SHELL`); on Windows `pwsh -NoProfile -Command`, else `powershell.exe`.
pub fn shell_argv(
    platform: &str,
    shell: Option<&str>,
    has_pwsh: bool,
    command: &str,
) -> Vec<String> {
    let v = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let mut argv = if platform == "windows" {
        v(&[
            if has_pwsh { "pwsh" } else { "powershell.exe" },
            "-NoProfile",
            "-Command",
        ])
    } else {
        v(&[shell.filter(|s| !s.is_empty()).unwrap_or("/bin/sh"), "-lc"])
    };
    argv.push(command.to_owned());
    argv
}

/// The real [`ProcessRunner`]: stdin closed, the login-shell `PATH`, stdout and stderr read on
/// their own threads into one capped buffer, the child killed after `timeout`.
pub struct SystemRunner;

impl ProcessRunner for SystemRunner {
    fn run(
        &self,
        argv: &[String],
        cwd: &Path,
        timeout: Duration,
        cap: usize,
    ) -> Result<RunResult, String> {
        use std::io::Read;
        use std::process::Stdio;
        use std::sync::{Arc, Mutex};
        use std::time::Instant;

        let (program, args) = argv.split_first().ok_or("no command to run")?;
        let mut child = crate::gui_path::command(program)
            .args(args)
            .current_dir(cwd)
            .env("PATH", crate::gui_path::child_path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{program} could not be started: {e}"))?;
        let buffer = Arc::new(Mutex::new((Vec::<u8>::new(), false)));
        let readers: Vec<_> = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn Read + Send>),
        ]
        .into_iter()
        .flatten()
        .map(|mut stream| {
            let buffer = Arc::clone(&buffer);
            std::thread::spawn(move || {
                let mut chunk = [0u8; 8192];
                while let Ok(n) = stream.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    let mut b = buffer.lock().unwrap();
                    let room = cap.saturating_sub(b.0.len());
                    b.0.extend_from_slice(&chunk[..n.min(room)]);
                    if n > room {
                        b.1 = true;
                    }
                }
            })
        })
        .collect();
        let start = Instant::now();
        let mut timed_out = false;
        let status = loop {
            match child.try_wait().map_err(|e| e.to_string())? {
                Some(status) => break status,
                None if start.elapsed() >= timeout => {
                    timed_out = true;
                    let _ = child.kill();
                    break child.wait().map_err(|e| e.to_string())?;
                }
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        };
        // A killed shell can leave grandchildren holding the pipes; do not wait for them.
        if !timed_out {
            for r in readers {
                let _ = r.join();
            }
        }
        let b = buffer.lock().unwrap();
        Ok(RunResult {
            exit_code: status.code(),
            timed_out,
            output: String::from_utf8_lossy(&b.0).into_owned(),
            truncated: b.1,
        })
    }
}

/// Runs the `teardown` `command` in `worktree` through the user's shell. The caller has passed
/// `trust::gate`.
pub fn run_teardown(
    runner: &dyn ProcessRunner,
    command: &str,
    worktree: &Path,
) -> Result<RunResult, String> {
    let has_pwsh =
        crate::gui_path::resolve_program("pwsh", &crate::gui_path::child_path()).is_some();
    let argv = shell_argv(
        platform(),
        std::env::var("SHELL").ok().as_deref(),
        has_pwsh,
        command,
    );
    runner.run(&argv, worktree, TEARDOWN_TIMEOUT, OUTPUT_CAP)
}

/// Whether and how the teardown of a repository may run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TeardownPlan {
    /// The project file has no teardown for this platform.
    None,
    /// It has one, but the project file is not trusted: nothing runs.
    Untrusted,
    Run(String),
}

/// Decides through `trust::gate` (never asks here: no answer is passed) and reads the command
/// from the same `sources`.
pub fn teardown_plan(
    sources: &crate::trust::Sources,
    trusted: &std::collections::BTreeMap<String, String>,
) -> TeardownPlan {
    let (project, _) = sources.refs();
    let Some(command) = teardown_of(project) else {
        return TeardownPlan::None;
    };
    match crate::trust::gate(sources, trusted, None) {
        crate::trust::Gate::Run { .. } => TeardownPlan::Run(command),
        _ => TeardownPlan::Untrusted,
    }
}

/// What `project_teardown` (the palette entry) reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeardownReport {
    pub plan: &'static str,
    pub result: Option<RunResult>,
}

/// Runs the teardown of `plan` in `worktree`; `plan` is `none`, `untrusted` or `ran`.
pub fn teardown_now(
    runner: &dyn ProcessRunner,
    plan: &TeardownPlan,
    worktree: &Path,
) -> Result<TeardownReport, String> {
    Ok(match plan {
        TeardownPlan::None => TeardownReport {
            plan: "none",
            result: None,
        },
        TeardownPlan::Untrusted => TeardownReport {
            plan: "untrusted",
            result: None,
        },
        TeardownPlan::Run(command) => TeardownReport {
            plan: "ran",
            result: Some(run_teardown(runner, command, worktree)?),
        },
    })
}

/// The `teardown` command of the project file for the current platform; `None` without one.
pub fn teardown_of(project: Option<(&str, &str)>) -> Option<String> {
    let (path, text) = project?;
    parse(path, text, platform()).config.teardown
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

    if !data["github"]["agentActions"].is_null() {
        let mut warnings = Vec::new();
        out.config.agent_actions = crate::agent_actions::parse(
            &data["github"]["agentActions"],
            "github.agentActions",
            &mut warnings,
        );
        out.warnings
            .extend(warnings.into_iter().map(|w| format!("{file}: {w}")));
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

/// A palette entry of the project file, as the frontend lists it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CommandInfo {
    pub name: String,
    pub target: Target,
}

/// What the palette offers for a repository: names only, nothing runs. `locked` while the project
/// file is not trusted (running an entry then asks first).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Entries {
    pub commands: Vec<CommandInfo>,
    pub presets: Vec<String>,
    pub locked: bool,
}

/// The `commands[]` and `presets[]` of the project file in `sources`, and whether `trust::gate`
/// would ask (never asks here: no answer is passed).
pub fn entries(
    sources: &crate::trust::Sources,
    trusted: &std::collections::BTreeMap<String, String>,
) -> Entries {
    let Some((path, text)) = &sources.project else {
        return Entries::default();
    };
    let config = parse(path, text, platform()).config;
    Entries {
        commands: config
            .commands
            .iter()
            .map(|c| CommandInfo {
                name: c.name.clone(),
                target: c.target,
            })
            .collect(),
        presets: config.presets.iter().map(|p| p.name.clone()).collect(),
        locked: matches!(
            crate::trust::gate(sources, trusted, None),
            crate::trust::Gate::Ask(_)
        ),
    }
}

/// The entry the user picked in the palette.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Entry {
    Command { name: String },
    Preset { name: String },
}

/// What running an entry did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum EntryOutcome {
    /// A terminal tab or a preset tab was created.
    Done,
    /// An `agent` command: the caller sends `command` as a prompt to the space's agent.
    Agent { command: String },
    /// Not trusted yet; nothing ran. The answer comes back as the `decision` of the next call.
    Ask { request: crate::trust::TrustRequest },
    /// The user chose "Run without project commands": nothing ran.
    Declined,
    /// The file no longer lists the entry.
    Missing,
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

    #[test]
    fn shell_argv_per_platform() {
        let a = |p, sh, pwsh| shell_argv(p, sh, pwsh, "make down");
        assert_eq!(
            a("linux", Some("/usr/bin/zsh"), false),
            ["/usr/bin/zsh", "-lc", "make down"]
        );
        assert_eq!(a("macos", None, false), ["/bin/sh", "-lc", "make down"]);
        assert_eq!(
            a("windows", None, true),
            ["pwsh", "-NoProfile", "-Command", "make down"]
        );
        assert_eq!(
            a("windows", None, false),
            ["powershell.exe", "-NoProfile", "-Command", "make down"]
        );
    }

    #[test]
    fn teardown_of_picks_the_platform_command() {
        let text = r#"{"teardown": {"default": "d", "macos": "m", "linux": "l", "windows": "w"}}"#;
        let expected = match platform() {
            "macos" => "m",
            "windows" => "w",
            _ => "l",
        };
        assert_eq!(teardown_of(Some(("f", text))).as_deref(), Some(expected));
        assert_eq!(teardown_of(Some(("f", "{}"))), None);
        assert_eq!(teardown_of(None), None);
    }

    #[test]
    fn failure_text_names_the_cause_and_keeps_the_output() {
        let r = |exit_code, timed_out, output: &str| RunResult {
            exit_code,
            timed_out,
            output: output.into(),
            truncated: false,
        };
        assert!(r(Some(0), false, "").success());
        assert_eq!(
            r(Some(2), false, "boom\n").failure_text(),
            "the teardown command failed (exit 2)\nboom"
        );
        assert_eq!(
            r(None, true, "").failure_text(),
            "the teardown command timed out"
        );
        assert!(!r(None, true, "").success());
    }

    #[test]
    fn teardown_plan_follows_the_trust_gate() {
        use crate::trust::{Sources, sha256_hex};
        let text = r#"{"teardown": "make down"}"#;
        let sources = Sources {
            root: "/r".into(),
            project: Some(("/r/.poimen/config.json".into(), text.into())),
            legacy: None,
        };
        let none = std::collections::BTreeMap::new();
        let trusted = std::collections::BTreeMap::from([("/r".to_owned(), sha256_hex(text))]);
        let stale = std::collections::BTreeMap::from([("/r".to_owned(), sha256_hex("{}"))]);
        assert_eq!(teardown_plan(&sources, &none), TeardownPlan::Untrusted);
        assert_eq!(teardown_plan(&sources, &stale), TeardownPlan::Untrusted);
        assert_eq!(
            teardown_plan(&sources, &trusted),
            TeardownPlan::Run("make down".into())
        );
        let empty = Sources {
            root: "/r".into(),
            ..Default::default()
        };
        assert_eq!(teardown_plan(&empty, &none), TeardownPlan::None);
    }

    struct Fake(std::cell::RefCell<Vec<(Vec<String>, std::path::PathBuf)>>);
    impl ProcessRunner for Fake {
        fn run(
            &self,
            argv: &[String],
            cwd: &Path,
            t: Duration,
            cap: usize,
        ) -> Result<RunResult, String> {
            assert_eq!((t, cap), (TEARDOWN_TIMEOUT, OUTPUT_CAP));
            self.0.borrow_mut().push((argv.to_vec(), cwd.to_path_buf()));
            Ok(RunResult {
                exit_code: Some(0),
                timed_out: false,
                output: "ok".into(),
                truncated: false,
            })
        }
    }

    #[test]
    fn teardown_now_runs_only_a_trusted_plan_with_the_limits() {
        let fake = Fake(Default::default());
        let wt = Path::new("/work/app-7");
        let r = teardown_now(&fake, &TeardownPlan::Untrusted, wt).unwrap();
        assert_eq!((r.plan, r.result), ("untrusted", None));
        let r = teardown_now(&fake, &TeardownPlan::None, wt).unwrap();
        assert_eq!(r.plan, "none");
        assert!(fake.0.borrow().is_empty());
        let r = teardown_now(&fake, &TeardownPlan::Run("make down".into()), wt).unwrap();
        assert_eq!(r.plan, "ran");
        let calls = fake.0.borrow();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].1, wt);
        assert_eq!(calls[0].0.last().map(String::as_str), Some("make down"));
    }

    #[cfg(unix)]
    mod process {
        use super::super::*;

        fn sh(script: &str, timeout_ms: u64, cap: usize) -> RunResult {
            let argv = shell_argv("linux", Some("/bin/sh"), false, script);
            SystemRunner
                .run(
                    &argv,
                    Path::new("/tmp"),
                    Duration::from_millis(timeout_ms),
                    cap,
                )
                .unwrap()
        }

        #[test]
        fn captures_stdout_stderr_and_the_exit_code() {
            let r = sh("echo out; echo err >&2; exit 3", 10_000, 1024);
            assert_eq!(r.exit_code, Some(3));
            assert!(!r.timed_out && !r.success());
            assert!(r.output.contains("out") && r.output.contains("err"));
        }

        #[test]
        fn runs_in_the_given_directory() {
            let r = sh("pwd", 10_000, 1024);
            assert!(r.success());
            assert!(r.output.trim().ends_with("tmp"));
        }

        #[test]
        fn a_command_over_the_timeout_is_killed() {
            let r = sh("sleep 30", 200, 1024);
            assert!(r.timed_out && !r.success());
            assert_eq!(r.exit_code, None);
        }

        #[test]
        fn output_is_capped() {
            let r = sh("yes x | head -c 5000", 10_000, 100);
            assert!(r.success());
            assert_eq!(r.output.len(), 100);
            assert!(r.truncated);
        }

        #[test]
        fn a_missing_program_is_an_error() {
            let err = SystemRunner
                .run(
                    &["/no/such/shell".into()],
                    Path::new("/tmp"),
                    Duration::from_secs(1),
                    10,
                )
                .unwrap_err();
            assert!(err.contains("could not be started"));
        }
    }
}
