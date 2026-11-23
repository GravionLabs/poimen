//! Saved prompts and commands (#1007): `snippets[]` of the user config. A snippet is text the user
//! sends again and again, to a terminal (typed, with or without Enter) or to an agent (the prompt
//! path, which always submits). The project file's `commands[]` are separate and unchanged.

use serde::Serialize;
use serde_json::Value;
use tauri::State;

use crate::backend::Backend;
use crate::{Shared, SharedMachines, home_dir, on_routed};

/// Where a snippet goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Terminal,
    Agent,
}

/// One entry of `snippets[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Snippet {
    pub label: String,
    pub text: String,
    /// Press Enter after the text (terminal target only; an agent prompt is always submitted).
    pub enter: bool,
    pub target: Target,
}

/// The snippets of `snippets[]`; an invalid entry is a warning and is left out, the rest loads.
pub fn parse(value: &Value, warnings: &mut Vec<String>) -> Vec<Snippet> {
    let Some(entries) = value.as_array() else {
        warnings.push("snippets must be a list; no snippets".into());
        return Vec::new();
    };
    let mut out: Vec<Snippet> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        match parse_entry(entry) {
            Ok(s) if out.iter().any(|o| o.label == s.label) => warnings.push(format!(
                "snippets[{i}]: the label {:?} is already used; ignored",
                s.label
            )),
            Ok(s) => out.push(s),
            Err(e) => warnings.push(format!("snippets[{i}]: {e}; ignored")),
        }
    }
    out
}

fn parse_entry(entry: &Value) -> Result<Snippet, String> {
    let obj = entry.as_object().ok_or("must be an object")?;
    let label = match obj.get("label") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_owned(),
        _ => return Err("label must be a non-empty string".into()),
    };
    let text = match obj.get("text") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => return Err("text must be a non-empty string".into()),
    };
    let enter = match obj.get("enter") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("enter must be true or false".into()),
    };
    let target = match obj.get("target") {
        None | Some(Value::Null) => Target::Terminal,
        Some(Value::String(s)) if s == "terminal" => Target::Terminal,
        Some(Value::String(s)) if s == "agent" => Target::Agent,
        Some(_) => return Err("target must be \"terminal\" or \"agent\"".into()),
    };
    Ok(Snippet {
        label,
        text,
        enter,
        target,
    })
}

/// Returns `text` with `snippets` replaced by `snippets` after validating it like the loader does
/// (any warning rejects the write); other keys and comments stay.
pub fn set_text(text: &str, snippets: &Value) -> Result<String, String> {
    let mut warnings = Vec::new();
    let parsed = parse(snippets, &mut warnings);
    if warnings.is_empty() && parsed.len() != snippets.as_array().map_or(0, Vec::len) {
        warnings.push("a snippet was not accepted".into());
    }
    if !warnings.is_empty() {
        return Err(format!("snippets: {}", warnings.join("; ")));
    }
    poimen_core::settings::set_key_text(text, "snippets", &snippets.to_string())
}

/// Writes only `snippets` after validating it; the settings watcher emits the change.
#[tauri::command]
pub fn set_snippets(snippets: Value) -> Result<(), String> {
    let home = home_dir().ok_or("the home directory is not known (HOME or USERPROFILE)")?;
    poimen_core::settings::save_with(&home, |text| set_text(text, &snippets))
}

/// Types a snippet into a terminal (`enter`: press Enter after it). The target may be a pane of a
/// machine. Agent snippets use `agent_prompt` instead.
#[tauri::command]
pub async fn terminal_type(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    target: String,
    text: String,
    enter: bool,
) -> Result<(), String> {
    on_routed(backend.inner(), machines.inner(), &target, move |b, id| {
        type_text(b, id, &text, enter)
    })
    .await
}

fn type_text(b: &dyn Backend, id: &str, text: &str, enter: bool) -> Result<(), String> {
    if enter {
        b.send_input(id, text)
    } else {
        b.send_text(id, text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parsed(value: Value) -> (Vec<Snippet>, Vec<String>) {
        let mut warnings = Vec::new();
        (parse(&value, &mut warnings), warnings)
    }

    #[test]
    fn defaults_are_enter_and_terminal() {
        let (s, w) = parsed(json!([{"label": "tests", "text": "cargo test"}]));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(
            s,
            vec![Snippet {
                label: "tests".into(),
                text: "cargo test".into(),
                enter: true,
                target: Target::Terminal,
            }]
        );
    }

    #[test]
    fn an_agent_snippet_without_enter_loads_and_serializes_in_wire_shape() {
        let (s, w) = parsed(json!([
            {"label": "deliver", "text": "/deliver-loop", "enter": false, "target": "agent"}
        ]));
        assert!(w.is_empty(), "{w:?}");
        let wire = serde_json::to_value(&s).unwrap();
        assert_eq!(wire[0]["target"], "agent");
        assert_eq!(wire[0]["enter"], false);
    }

    #[test]
    fn invalid_and_duplicate_entries_are_warnings_and_the_rest_loads() {
        let (s, w) = parsed(json!([
            {"label": "a", "text": "x"},
            {"label": "a", "text": "y"},
            {"label": " ", "text": "y"},
            {"label": "b", "text": ""},
            {"label": "c", "text": "y", "target": "pane"},
            {"label": "d", "text": "y", "enter": "yes"},
            3
        ]));
        assert_eq!(s.len(), 1);
        assert_eq!(w.len(), 6, "{w:?}");
        assert!(w[0].starts_with("snippets[1]:"), "{w:?}");
        assert_eq!(
            parsed(json!({"a": 1})).1,
            vec!["snippets must be a list; no snippets"]
        );
    }

    #[test]
    fn enter_decides_whether_the_text_is_submitted() {
        let fake = crate::backend_fake::FakeBackend::default();
        type_text(&fake, "t1", "ls", true).unwrap();
        type_text(&fake, "t1", "ls", false).unwrap();
        let calls = fake.calls();
        assert!(calls[0].starts_with("input t1 ls"), "{calls:?}");
        assert!(calls[1].starts_with("text t1 ls"), "{calls:?}");
    }

    #[test]
    fn set_text_keeps_other_keys_and_rejects_invalid_input() {
        let ok = json!([{"label": "a", "text": "x", "enter": false, "target": "agent"}]);
        let out = set_text("{\n  \"theme\": \"dark\"\n}\n", &ok).unwrap();
        let back: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(back["theme"], "dark");
        assert_eq!(back["snippets"][0]["label"], "a");
        let err = set_text("{}", &json!([{"label": "a"}])).unwrap_err();
        assert!(err.starts_with("snippets: "), "{err}");
        assert!(set_text("{}", &json!({"label": "a"})).is_err());
    }
}
