//! Start a deliver loop from the app (#746): the user's explicit action opens a tab named
//! `deliver <target>` in the space of the repository and types `deliver-loop …` into it. poimen
//! is only the launcher; the loop starts its agent sessions itself. The arguments are built here
//! from validated values (never concatenated from free text) and quoted for the shell of the
//! terminal with `poimen_client::agent::command_line`.

use std::collections::BTreeMap;

use poimen_client::agent::{ShellKind, command_line};
use serde::{Deserialize, Serialize};

use crate::backend::Backend;
use crate::trust::{self, Decision, Gate, Sources, TrustRequest};

/// The installed command (the shim of skills-manage).
pub const COMMAND: &str = "deliver-loop";

/// The harnesses the loop knows.
const HARNESSES: [&str; 2] = ["claude", "copilot"];

/// What the loop runs on.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Target {
    /// `--milestone M<nnn>`.
    Milestone { name: String },
    /// `--issues 12,15`.
    Issues { numbers: Vec<u64> },
}

/// The dialog's values, as the webview sends them.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub target: Target,
    /// `claude` or `copilot`; absent leaves the loop's default.
    #[serde(default)]
    pub harness: Option<String>,
    /// Free text; absent or empty leaves the loop's default.
    #[serde(default)]
    pub model: Option<String>,
    /// `--max`, a positive number.
    #[serde(default)]
    pub max: Option<u64>,
}

/// The arguments of `deliver-loop` for `request`, or why a value is rejected.
pub fn args(request: &Request) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    match &request.target {
        Target::Milestone { name } => {
            let digits = name.strip_prefix('M').unwrap_or("");
            if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                return Err(format!("\"{name}\" is not a milestone name (M<nnn>)"));
            }
            out.extend(["--milestone".to_owned(), name.clone()]);
        }
        Target::Issues { numbers } => {
            if numbers.is_empty() || numbers.contains(&0) {
                return Err("an issue number must be a positive number".into());
            }
            let list: Vec<String> = numbers.iter().map(u64::to_string).collect();
            out.extend(["--issues".to_owned(), list.join(",")]);
        }
    }
    if let Some(harness) = request.harness.as_deref().filter(|h| !h.is_empty()) {
        if !HARNESSES.contains(&harness) {
            return Err(format!("\"{harness}\" is not a harness of the loop"));
        }
        out.extend(["--harness".to_owned(), harness.to_owned()]);
    }
    if let Some(model) = request
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        if model.chars().any(char::is_control) {
            return Err("the model has a control character".into());
        }
        out.extend(["--model".to_owned(), model.to_owned()]);
    }
    if let Some(max) = request.max {
        if max == 0 {
            return Err("--max must be a positive number".into());
        }
        out.extend(["--max".to_owned(), max.to_string()]);
    }
    Ok(out)
}

/// The name of the tab: `deliver M016` or `deliver #12,#15`.
pub fn tab_name(request: &Request) -> String {
    match &request.target {
        Target::Milestone { name } => format!("deliver {name}"),
        Target::Issues { numbers } => {
            let list: Vec<String> = numbers.iter().map(|n| format!("#{n}")).collect();
            format!("deliver {}", list.join(","))
        }
    }
}

/// The command line to type, quoted for a shell of `kind`.
pub fn line(request: &Request, kind: ShellKind) -> Result<String, String> {
    let args = args(request)?;
    command_line(COMMAND, &args, kind)
        .ok_or_else(|| "cmd.exe cannot take the command line; use PowerShell".to_owned())
}

/// The kind of the shell a new local terminal runs (`poimen-server`'s default shell).
fn local_shell() -> ShellKind {
    if cfg!(windows) {
        return ShellKind::PowerShell;
    }
    ShellKind::of(&std::env::var("SHELL").unwrap_or_default())
}

/// Whether `deliver-loop` is a file in a directory of `path`.
pub fn available_in(path: &str) -> bool {
    poimen_core::gui_path::resolve_program(COMMAND, path).is_some()
}

/// `deliver.base` of `<root>/.agents/config.json`: the base branch the loop will use, if set.
pub fn configured_base(root: &str) -> Option<String> {
    let text =
        std::fs::read_to_string(std::path::Path::new(root).join(".agents/config.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    let base = json.get("deliver")?.get("base")?.as_str()?.trim();
    (!base.is_empty()).then(|| base.to_owned())
}

/// What the dialog shows before anything starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Plan {
    /// `deliver-loop` is on the `PATH`.
    pub available: bool,
    /// `deliver.base` of the repository's `.agents/config.json`; `None`: the loop's default.
    pub base: Option<String>,
    /// The exact command line, empty while a value is rejected.
    pub command: String,
    /// The first rejected value, if any.
    pub error: Option<String>,
    pub tab: String,
}

/// The plan for `request` in the repository of `dir`. Read-only.
pub fn plan(dir: &str, request: &Request, path: &str, kind: ShellKind) -> Plan {
    let root = poimen_core::git::toplevel(dir).unwrap_or_else(|| dir.to_owned());
    let (command, error) = match line(request, kind) {
        Ok(command) => (command, None),
        Err(e) => (String::new(), Some(e)),
    };
    Plan {
        available: available_in(path),
        base: configured_base(&root),
        command,
        error,
        tab: tab_name(request),
    }
}

/// What a start answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Outcome {
    /// The tab exists and the command is typed.
    Started { terminal_id: String },
    /// The project file is not trusted; nothing was started.
    Ask { request: TrustRequest },
    /// The user declined the trust.
    Declined,
    /// `deliver-loop` is not installed; nothing was started.
    Missing,
}

/// Starts the loop in a new tab of the space `space_id`. The project file goes through
/// `trust::gate` first (the shim can run a project-local copy of the skill, which is project
/// code); `persist(hash)` stores a fresh trust. Nothing is started without the command on `path`.
#[allow(clippy::too_many_arguments)]
pub fn start(
    backend: &dyn Backend,
    sources: &Sources,
    trusted: &BTreeMap<String, String>,
    decision: Option<&Decision>,
    persist: &dyn Fn(&str) -> Result<(), String>,
    path: &str,
    kind: ShellKind,
    space_id: &str,
    dir: &str,
    request: &Request,
) -> Result<Outcome, String> {
    let command = line(request, kind)?;
    if !available_in(path) {
        return Ok(Outcome::Missing);
    }
    match trust::gate(sources, trusted, decision) {
        Gate::Ask(request) => return Ok(Outcome::Ask { request }),
        Gate::Strip => return Ok(Outcome::Declined),
        Gate::Run { store } => {
            if let Some(hash) = store {
                persist(&hash)?;
            }
        }
    }
    let pane = backend
        .create_terminal(space_id, Some(dir))?
        .ok_or("the backend did not report the new tab")?;
    backend.rename_terminal(&pane, Some(&tab_name(request)))?;
    backend.send_input(&pane, &command)?;
    Ok(Outcome::Started { terminal_id: pane })
}

/// The plan of the dialog for the repository of `dir`.
#[tauri::command]
pub async fn deliver_plan(dir: String, request: Request) -> Result<Plan, String> {
    tauri::async_runtime::spawn_blocking(move || {
        plan(
            &dir,
            &request,
            &poimen_core::gui_path::child_path(),
            local_shell(),
        )
    })
    .await
    .map_err(|e| e.to_string())
}

/// Starts the loop in a new tab of the space `space_id` (a user action, from the dialog).
#[tauri::command]
pub async fn deliver_start(
    backend: tauri::State<'_, crate::Shared>,
    space_id: String,
    dir: String,
    request: Request,
    decision: Option<Decision>,
) -> Result<Outcome, String> {
    crate::on_backend(backend.inner(), move |b| {
        let root = poimen_core::git::toplevel(&dir).unwrap_or_else(|| dir.clone());
        let sources = Sources::read(&root);
        let home = crate::home_dir();
        let trusted = crate::get_settings().trusted_projects;
        let persist = |hash: &str| -> Result<(), String> {
            let home = home.as_deref().ok_or("the home directory is not known")?;
            crate::settings::trust_project(home, &sources.root, hash)
        };
        start(
            b,
            &sources,
            &trusted,
            decision.as_ref(),
            &persist,
            &poimen_core::gui_path::child_path(),
            local_shell(),
            &space_id,
            &dir,
            &request,
        )
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend_fake::FakeBackend;
    use crate::test_dir::TestDir;

    fn milestone(name: &str) -> Request {
        Request {
            target: Target::Milestone { name: name.into() },
            harness: None,
            model: None,
            max: None,
        }
    }

    fn issues(numbers: &[u64]) -> Request {
        Request {
            target: Target::Issues {
                numbers: numbers.to_vec(),
            },
            ..milestone("M1")
        }
    }

    fn words(args: Vec<String>) -> Vec<String> {
        args
    }

    /// A directory with an executable named `deliver-loop`, as a `PATH`.
    fn path_with_loop() -> (TestDir, String) {
        let dir = TestDir::new("poimen-deliver-");
        let name = if cfg!(windows) {
            "deliver-loop.cmd"
        } else {
            COMMAND
        };
        std::fs::write(dir.join(name), "").unwrap();
        let path = dir.to_string_lossy().into_owned();
        (dir, path)
    }

    const NOTHING: &str = "";

    fn untrusted_sources() -> Sources {
        Sources {
            root: "/work/app".into(),
            project: Some((
                "/work/app/.poimen/config.json".into(),
                r#"{"setup": "pnpm install"}"#.into(),
            )),
            legacy: None,
        }
    }

    fn ok_persist(_: &str) -> Result<(), String> {
        Ok(())
    }

    #[test]
    fn a_milestone_target_is_two_arguments() {
        assert_eq!(
            words(args(&milestone("M016")).unwrap()),
            ["--milestone", "M016"]
        );
    }

    #[test]
    fn issue_targets_are_one_comma_list() {
        assert_eq!(
            words(args(&issues(&[12, 15])).unwrap()),
            ["--issues", "12,15"]
        );
    }

    #[test]
    fn a_bad_milestone_is_rejected() {
        for name in [
            "",
            "M",
            "016",
            "m16",
            "M1 --max 9",
            "M1;ls",
            "M-1",
            "M1234567",
        ] {
            assert!(args(&milestone(name)).is_err(), "{name}");
        }
    }

    #[test]
    fn a_bad_issue_list_is_rejected() {
        assert!(args(&issues(&[])).is_err());
        assert!(args(&issues(&[12, 0])).is_err());
    }

    #[test]
    fn harness_model_and_max_follow_the_target() {
        let request = Request {
            harness: Some("copilot".into()),
            model: Some(" gpt 5 ".into()),
            max: Some(3),
            ..milestone("M2")
        };
        assert_eq!(
            words(args(&request).unwrap()),
            [
                "--milestone",
                "M2",
                "--harness",
                "copilot",
                "--model",
                "gpt 5",
                "--max",
                "3"
            ]
        );
    }

    #[test]
    fn an_empty_harness_model_or_max_leaves_the_default() {
        let request = Request {
            harness: Some(String::new()),
            model: Some("  ".into()),
            ..milestone("M2")
        };
        assert_eq!(words(args(&request).unwrap()), ["--milestone", "M2"]);
    }

    #[test]
    fn an_unknown_harness_a_zero_max_and_a_control_character_are_rejected() {
        let bad = |r: Request| args(&r).is_err();
        assert!(bad(Request {
            harness: Some("claude; rm -rf ~".into()),
            ..milestone("M2")
        }));
        assert!(bad(Request {
            max: Some(0),
            ..milestone("M2")
        }));
        assert!(bad(Request {
            model: Some("a\nb".into()),
            ..milestone("M2")
        }));
    }

    #[test]
    fn the_model_is_quoted_for_the_shell() {
        let request = Request {
            model: Some("my model's \"x\"".into()),
            ..milestone("M2")
        };
        assert_eq!(
            line(&request, ShellKind::Posix).unwrap(),
            r#"deliver-loop --milestone M2 --model 'my model'\''s "x"'"#
        );
        assert_eq!(
            line(&request, ShellKind::PowerShell).unwrap(),
            r#"deliver-loop --milestone M2 --model 'my model''s "x"'"#
        );
        assert!(line(&request, ShellKind::Cmd).is_err());
    }

    #[test]
    fn the_tab_is_named_after_the_target() {
        assert_eq!(tab_name(&milestone("M016")), "deliver M016");
        assert_eq!(tab_name(&issues(&[12, 15])), "deliver #12,#15");
    }

    #[test]
    fn the_command_is_found_only_on_the_path() {
        assert!(available_in(&path_with_loop().1));
        assert!(!available_in(NOTHING));
        assert!(!available_in(
            std::env::temp_dir()
                .join("poimen-deliver-nothing-here")
                .to_str()
                .unwrap()
        ));
    }

    #[test]
    fn the_base_is_read_from_the_agents_config() {
        let dir = TestDir::new("poimen-deliver-base-");
        std::fs::create_dir_all(dir.join(".agents")).unwrap();
        let root = dir.to_string_lossy().into_owned();
        assert_eq!(configured_base(&root), None);
        std::fs::write(
            dir.join(".agents/config.json"),
            r#"{"deliver": {"base": "release/next"}}"#,
        )
        .unwrap();
        assert_eq!(configured_base(&root).as_deref(), Some("release/next"));
        std::fs::write(dir.join(".agents/config.json"), r#"{"deliver": {}}"#).unwrap();
        assert_eq!(configured_base(&root), None);
    }

    #[test]
    fn a_start_opens_a_named_tab_and_types_the_command() {
        let fake = FakeBackend::default();
        let outcome = start(
            &fake,
            &Sources::default(),
            &BTreeMap::new(),
            None,
            &ok_persist,
            &path_with_loop().1,
            ShellKind::Posix,
            "w1",
            "/work/app",
            &milestone("M016"),
        )
        .unwrap();
        assert!(matches!(outcome, Outcome::Started { .. }), "{outcome:?}");
        let calls = fake.calls();
        assert!(
            calls[0].starts_with("create_terminal w1 /work/app"),
            "{calls:?}"
        );
        assert!(calls[1].contains("deliver M016"), "{calls:?}");
        assert!(
            calls[2].contains("deliver-loop --milestone M016"),
            "{calls:?}"
        );
    }

    #[test]
    fn without_the_command_nothing_starts() {
        let fake = FakeBackend::default();
        let outcome = start(
            &fake,
            &Sources::default(),
            &BTreeMap::new(),
            None,
            &ok_persist,
            NOTHING,
            ShellKind::Posix,
            "w1",
            "/work/app",
            &milestone("M016"),
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Missing);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn an_untrusted_project_asks_and_starts_nothing() {
        let fake = FakeBackend::default();
        let outcome = start(
            &fake,
            &untrusted_sources(),
            &BTreeMap::new(),
            None,
            &ok_persist,
            &path_with_loop().1,
            ShellKind::Posix,
            "w1",
            "/work/app",
            &milestone("M016"),
        )
        .unwrap();
        assert!(matches!(outcome, Outcome::Ask { .. }), "{outcome:?}");
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_trust_answer_stores_the_hash_and_starts() {
        let fake = FakeBackend::default();
        let sources = untrusted_sources();
        let stored = std::sync::Mutex::new(Vec::new());
        let persist = |hash: &str| -> Result<(), String> {
            stored.lock().unwrap().push(hash.to_owned());
            Ok(())
        };
        let decision = Decision::Trust {
            hash: sources.hash().unwrap(),
        };
        let outcome = start(
            &fake,
            &sources,
            &BTreeMap::new(),
            Some(&decision),
            &persist,
            &path_with_loop().1,
            ShellKind::Posix,
            "w1",
            "/work/app",
            &milestone("M016"),
        )
        .unwrap();
        assert!(matches!(outcome, Outcome::Started { .. }), "{outcome:?}");
        assert_eq!(stored.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_declined_trust_starts_nothing() {
        let fake = FakeBackend::default();
        let outcome = start(
            &fake,
            &untrusted_sources(),
            &BTreeMap::new(),
            Some(&Decision::Without),
            &ok_persist,
            &path_with_loop().1,
            ShellKind::Posix,
            "w1",
            "/work/app",
            &milestone("M016"),
        )
        .unwrap();
        assert_eq!(outcome, Outcome::Declined);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn a_failing_backend_step_is_an_error() {
        let fake = FakeBackend::default();
        fake.fail_on("create_terminal");
        let result = start(
            &fake,
            &Sources::default(),
            &BTreeMap::new(),
            None,
            &ok_persist,
            &path_with_loop().1,
            ShellKind::Posix,
            "w1",
            "/work/app",
            &milestone("M016"),
        );
        assert_eq!(result, Err("boom".into()));
    }

    #[test]
    fn the_plan_shows_the_command_and_the_missing_command() {
        let plan = plan(
            "/nonexistent-dir",
            &milestone("M2"),
            NOTHING,
            ShellKind::Posix,
        );
        assert!(!plan.available);
        assert_eq!(plan.base, None);
        assert_eq!(plan.command, "deliver-loop --milestone M2");
        let bad = super::plan(
            "/nonexistent-dir",
            &milestone("x"),
            NOTHING,
            ShellKind::Posix,
        );
        assert!(bad.command.is_empty() && bad.error.is_some());
    }
}
