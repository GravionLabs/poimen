//! Agent harnesses (`harnesses` in `config.json`): how an agent CLI is started, with which model
//! and prompt. Templates expand per argv element and never go through a shell, so a prompt with
//! spaces, quotes, `$()`, `;` or newlines stays one argument. `settings.rs` resolves the config.

// The expansion is consumed by the task start (a later PBI); until then only tests call it.
#![allow(dead_code)]

use serde::Serialize;
use serde_json::Value;

use crate::gui_path;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Harness {
    pub id: String,
    /// The program and its fixed arguments.
    pub command: Vec<String>,
    /// Appended when a model is chosen; `{model}` is replaced inside an element.
    pub model_args: Vec<String>,
    /// Appended for a prompt; `{prompt}` is replaced inside an element.
    pub prompt_args: Vec<String>,
    pub models: Vec<String>,
    pub default_model: Option<String>,
    /// Text typed at the agent's prompt to end its session (`/exit`); none when unset.
    pub exit_input: Option<String>,
    /// Appended to `command` to resume a session; `{session}` is replaced inside an element.
    /// Empty: the harness cannot resume.
    pub resume_args: Vec<String>,
    /// Appended to `command` for a headless one-shot call that prints its answer and exits;
    /// `{prompt}` is replaced inside an element. Empty: the harness cannot be called headless.
    pub print_args: Vec<String>,
}

/// A harness with whether its program was found on the `PATH` of a child.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessInfo {
    #[serde(flatten)]
    pub harness: Harness,
    pub available: bool,
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// The built-in harnesses. The Copilot CLI lists no models in `copilot --help` (it documents
/// `--model <model>` and `auto`, and uses `gpt-5.4` in its examples), so those are the defaults.
pub fn default_harnesses() -> Vec<Harness> {
    vec![
        Harness {
            id: "claude".into(),
            command: strings(&["claude"]),
            model_args: strings(&["--model", "{model}"]),
            prompt_args: strings(&["{prompt}"]),
            models: strings(&["opus", "sonnet", "haiku"]),
            default_model: None,
            exit_input: Some("/exit".into()),
            resume_args: strings(&["--resume", "{session}"]),
            print_args: strings(&["-p", "{prompt}"]),
        },
        Harness {
            id: "copilot".into(),
            command: strings(&["copilot"]),
            model_args: strings(&["--model", "{model}"]),
            prompt_args: strings(&["-i", "{prompt}"]),
            models: strings(&["auto", "gpt-5.4"]),
            default_model: None,
            exit_input: Some("/exit".into()),
            resume_args: strings(&["--resume", "{session}"]),
            print_args: strings(&["-p", "{prompt}"]),
        },
    ]
}

fn string_list(value: &Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|v| v.as_str().map(str::to_owned))
        .collect()
}

fn optional_list(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    match obj.get(key) {
        None => Ok(Vec::new()),
        Some(v) => string_list(v).ok_or_else(|| format!("{key} must be an array of strings")),
    }
}

fn parse_one(id: &str, value: &Value) -> Result<Harness, String> {
    let obj = value.as_object().ok_or("must be an object")?;
    let command = obj
        .get("command")
        .and_then(string_list)
        .filter(|c| c.first().is_some_and(|p| !p.trim().is_empty()))
        .ok_or("command must be a non-empty array of strings")?;
    let default_model = match obj.get("defaultModel") {
        None => None,
        Some(Value::String(m)) if !m.trim().is_empty() => Some(m.clone()),
        Some(_) => return Err("defaultModel must be a non-empty string".into()),
    };
    let exit_input = match obj.get("exitInput") {
        None => None,
        Some(Value::String(m)) if !m.trim().is_empty() => Some(m.clone()),
        Some(_) => return Err("exitInput must be a non-empty string".into()),
    };
    Ok(Harness {
        id: id.to_owned(),
        command,
        model_args: optional_list(obj, "modelArgs")?,
        prompt_args: optional_list(obj, "promptArgs")?,
        models: optional_list(obj, "models")?,
        default_model,
        exit_input,
        resume_args: optional_list(obj, "resumeArgs")?,
        print_args: optional_list(obj, "printArgs")?,
    })
}

/// Resolves the `harnesses` map. Invalid entries warn and are skipped; a valid entry with the id
/// of a default replaces it. Without a valid entry the defaults stay.
pub fn resolve(value: &Value, warnings: &mut Vec<String>) -> Vec<Harness> {
    let mut out = default_harnesses();
    let Some(map) = value.as_object() else {
        warnings.push("harnesses must be an object".into());
        return out;
    };
    for (id, entry) in map {
        match parse_one(id, entry) {
            Ok(h) => match out.iter_mut().find(|d| d.id == *id) {
                Some(slot) => *slot = h,
                None => out.push(h),
            },
            Err(e) => warnings.push(format!("harnesses.{id}: {e}; ignored")),
        }
    }
    out
}

fn expand(template: &[String], placeholder: &str, value: &str) -> Vec<String> {
    template
        .iter()
        .map(|t| t.replace(placeholder, value))
        .collect()
}

/// The argv to start `h`: the command, then the model arguments when a model is chosen (else the
/// default model, else none: the harness default), then the prompt arguments when there is a prompt.
pub fn build_argv(h: &Harness, model: Option<&str>, prompt: Option<&str>) -> Vec<String> {
    let mut argv = h.command.clone();
    let model = model
        .filter(|m| !m.trim().is_empty())
        .or(h.default_model.as_deref());
    if let Some(m) = model {
        argv.extend(expand(&h.model_args, "{model}", m));
    }
    if let Some(p) = prompt.filter(|p| !p.is_empty()) {
        argv.extend(expand(&h.prompt_args, "{prompt}", p));
    }
    argv
}

/// The argv to resume `session` in `h`: the command, then the resume arguments with `{session}`
/// replaced per element (a session with spaces stays one element). `None` without `resumeArgs`.
pub fn build_resume_argv(h: &Harness, session: &str) -> Option<Vec<String>> {
    if h.resume_args.is_empty() {
        return None;
    }
    let mut argv = h.command.clone();
    argv.extend(expand(&h.resume_args, "{session}", session));
    Some(argv)
}

/// The argv for a headless call of `h` with `prompt`: the command, then the print arguments with
/// `{prompt}` replaced per element. `None` without `printArgs` or without a `{prompt}` in them.
pub fn build_print_argv(h: &Harness, prompt: &str) -> Option<Vec<String>> {
    if !h.print_args.iter().any(|a| a.contains("{prompt}")) {
        return None;
    }
    let mut argv = h.command.clone();
    argv.extend(expand(&h.print_args, "{prompt}", prompt));
    Some(argv)
}

fn info_with(harnesses: Vec<Harness>, available: impl Fn(&str) -> bool) -> Vec<HarnessInfo> {
    harnesses
        .into_iter()
        .map(|harness| {
            let available = available(&harness.command[0]);
            HarnessInfo { harness, available }
        })
        .collect()
}

/// The harnesses with availability: the program resolves on the `PATH` of a child (on Windows
/// including `.cmd`/`.exe` shims through `PATHEXT`).
pub fn with_availability(harnesses: Vec<Harness>) -> Vec<HarnessInfo> {
    let path = gui_path::child_path();
    info_with(harnesses, |p| gui_path::resolve_program(p, &path).is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn resolved(v: Value) -> (Vec<Harness>, Vec<String>) {
        let mut w = Vec::new();
        let h = resolve(&v, &mut w);
        (h, w)
    }

    #[test]
    fn defaults_are_claude_and_copilot() {
        let d = default_harnesses();
        assert_eq!(d[0].id, "claude");
        assert_eq!(d[0].command, ["claude"]);
        assert_eq!(d[0].model_args, ["--model", "{model}"]);
        assert_eq!(d[0].prompt_args, ["{prompt}"]);
        assert_eq!(d[0].models, ["opus", "sonnet", "haiku"]);
        assert_eq!(d[1].id, "copilot");
        assert_eq!(d[1].prompt_args, ["-i", "{prompt}"]);
    }

    #[test]
    fn defaults_end_and_resume_a_session() {
        for h in default_harnesses() {
            assert_eq!(h.exit_input.as_deref(), Some("/exit"), "{}", h.id);
            assert_eq!(h.resume_args, ["--resume", "{session}"], "{}", h.id);
        }
    }

    #[test]
    fn defaults_print_headless_and_a_config_entry_can_drop_it() {
        for h in default_harnesses() {
            assert_eq!(h.print_args, ["-p", "{prompt}"], "{}", h.id);
        }
        let (h, w) = resolved(json!({
            "claude": {"command": ["claude"]},
            "other": {"command": ["other"], "printArgs": ["--print", "{prompt}"]},
            "bad": {"command": ["bad"], "printArgs": "x"}
        }));
        assert_eq!(w.len(), 1);
        assert!(h[0].print_args.is_empty());
        assert_eq!(h[2].print_args, ["--print", "{prompt}"]);
        assert_eq!(
            serde_json::to_value(&h[2]).unwrap()["printArgs"][0],
            "--print"
        );
    }

    #[test]
    fn print_argv_keeps_the_prompt_one_element() {
        let mut h = default_harnesses().remove(0);
        let argv = build_print_argv(&h, "a b; $(x)\nc").unwrap();
        assert_eq!(argv, ["claude", "-p", "a b; $(x)\nc"]);
        h.print_args.clear();
        assert_eq!(build_print_argv(&h, "x"), None);
        h.print_args = vec!["-p".into()];
        assert_eq!(build_print_argv(&h, "x"), None);
    }

    #[test]
    fn parses_exit_input_and_resume_args() {
        let (h, w) = resolved(json!({
            "claude": {"command": ["claude"], "exitInput": "/quit", "resumeArgs": ["-r", "{session}"]},
            "other": {"command": ["other"]}
        }));
        assert!(w.is_empty());
        assert_eq!(h[0].exit_input.as_deref(), Some("/quit"));
        assert_eq!(h[0].resume_args, ["-r", "{session}"]);
        assert_eq!(h[2].exit_input, None);
        assert!(h[2].resume_args.is_empty());
        let wire = serde_json::to_value(&h[0]).unwrap();
        assert_eq!(wire["exitInput"], "/quit");
        assert_eq!(wire["resumeArgs"], json!(["-r", "{session}"]));
    }

    #[test]
    fn invalid_exit_input_and_resume_args_warn() {
        let (h, w) = resolved(json!({
            "a": {"command": ["a"], "exitInput": ""},
            "b": {"command": ["b"], "resumeArgs": "x"}
        }));
        assert_eq!(h.len(), 2);
        assert_eq!(w.len(), 2);
    }

    #[test]
    fn resume_argv_expands_per_element() {
        let h = &default_harnesses()[0];
        assert_eq!(
            build_resume_argv(h, "abc-123").unwrap(),
            ["claude", "--resume", "abc-123"]
        );
        let spaced = "my session; $(id) \"q\"";
        assert_eq!(
            build_resume_argv(&default_harnesses()[1], spaced).unwrap(),
            ["copilot", "--resume", spaced]
        );
        let mut h = h.clone();
        h.resume_args = vec!["--resume={session}".into()];
        assert_eq!(
            build_resume_argv(&h, "a b").unwrap(),
            ["claude", "--resume=a b"]
        );
    }

    #[test]
    fn a_harness_without_resume_args_has_no_resume_argv() {
        let (h, _) = resolved(json!({"other": {"command": ["other"]}}));
        assert_eq!(build_resume_argv(&h[2], "s"), None);
    }

    #[test]
    fn parses_an_entry_and_overrides_a_default() {
        let (h, w) = resolved(json!({
            "claude": {"command": ["claude", "--x"], "modelArgs": ["-m", "{model}"],
                       "promptArgs": ["{prompt}"], "models": ["a"], "defaultModel": "a"},
            "other": {"command": ["other"]}
        }));
        assert!(w.is_empty());
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].command, ["claude", "--x"]);
        assert_eq!(h[0].default_model.as_deref(), Some("a"));
        assert_eq!(h[2].id, "other");
        assert!(h[2].models.is_empty());
    }

    #[test]
    fn invalid_entries_warn_and_keep_defaults() {
        let (h, w) = resolved(json!({
            "claude": {"command": "claude"},
            "a": {"command": []},
            "b": {"command": ["b"], "models": [1]},
            "c": {"command": ["c"], "defaultModel": ""},
            "d": 5
        }));
        assert_eq!(h, default_harnesses());
        assert_eq!(w.len(), 5);
        assert!(w.contains(
            &"harnesses.a: command must be a non-empty array of strings; ignored".to_owned()
        ));
        let (h, w) = resolved(json!([]));
        assert_eq!(h, default_harnesses());
        assert_eq!(w, ["harnesses must be an object"]);
    }

    #[test]
    fn no_model_means_no_model_args() {
        let h = &default_harnesses()[0];
        assert_eq!(build_argv(h, None, None), ["claude"]);
        assert_eq!(build_argv(h, Some(" "), Some("go")), ["claude", "go"]);
        assert_eq!(
            build_argv(h, Some("opus"), Some("go")),
            ["claude", "--model", "opus", "go"]
        );
    }

    #[test]
    fn default_model_applies_when_none_is_chosen() {
        let mut h = default_harnesses()[0].clone();
        h.default_model = Some("sonnet".into());
        assert_eq!(build_argv(&h, None, None), ["claude", "--model", "sonnet"]);
        assert_eq!(
            build_argv(&h, Some("haiku"), None),
            ["claude", "--model", "haiku"]
        );
    }

    #[test]
    fn a_hostile_prompt_stays_one_argv_element() {
        let h = &default_harnesses()[1];
        let prompt = "fix it; rm -rf / \"$(whoami)\" `id`\nline two 'q'";
        let argv = build_argv(h, Some("auto"), Some(prompt));
        assert_eq!(argv, ["copilot", "--model", "auto", "-i", prompt]);
    }

    #[test]
    fn placeholders_expand_inside_an_element() {
        let mut h = default_harnesses()[0].clone();
        h.model_args = vec!["--model={model}".into()];
        assert_eq!(
            build_argv(&h, Some("opus"), None),
            ["claude", "--model=opus"]
        );
    }

    #[test]
    fn a_windows_npm_shim_makes_a_harness_available() {
        let exists = |p: &std::path::Path| p == std::path::Path::new("/npm").join("copilot.cmd");
        let found = |p: &str, windows: bool| {
            gui_path::resolve_program_with(p, "/npm", windows, Some(".EXE;.CMD"), exists).is_some()
        };
        assert!(found("copilot", true));
        assert!(!found("copilot", false));
        assert!(!found("claude", true));
    }

    #[test]
    fn availability_follows_the_lookup() {
        let info = info_with(default_harnesses(), |p| p == "claude");
        assert!(info[0].available);
        assert!(!info[1].available);
        let wire = serde_json::to_value(&info[0]).unwrap();
        assert_eq!(wire["id"], "claude");
        assert_eq!(wire["available"], true);
        assert_eq!(wire["modelArgs"], json!(["--model", "{model}"]));
    }
}
