//! In-place editing of the project file `.poimen/config.json` from the space overview: `read`
//! returns the text, `save` merges a patch of the known keys (`layout`, `setup`, `teardown`) into
//! the existing JSON object, so every other key (`commands`, `presets`, `tasks`, unknown keys)
//! survives. The new content goes through `trust::gate` before anything is written: a changed
//! file has a new hash, so it is written only with a matching `Trust` answer or when the hash is
//! already trusted (or the file lists no command).

use std::collections::BTreeMap;
use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

use crate::layout::parse_layout_value;
use crate::project::PROJECT_FILE;
use crate::trust::{self, Decision, Gate, Sources, TrustRequest};

/// The project file as the editor shows it.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProjectFile {
    pub root: String,
    pub exists: bool,
    /// The file text; empty without a file.
    pub text: String,
}

/// An invalid field, shown inline next to it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SaveOutcome {
    pub saved: bool,
    pub errors: Vec<FieldError>,
    /// The content needs the user's trust first; nothing was written.
    pub trust: Option<TrustRequest>,
    /// The hash to persist as trusted for the root (a fresh `Trust` answer).
    pub store: Option<String>,
}

pub fn read(root: &str) -> ProjectFile {
    let text = std::fs::read_to_string(Path::new(root).join(PROJECT_FILE)).ok();
    ProjectFile {
        root: root.to_owned(),
        exists: text.is_some(),
        text: text.unwrap_or_default(),
    }
}

fn error(field: &str, message: impl Into<String>) -> FieldError {
    FieldError {
        field: field.to_owned(),
        message: message.into(),
    }
}

/// Merges `patch` (an object; a key present with `null` or a blank string removes it) into
/// `existing`. Unknown keys and the order of existing keys are kept.
pub fn apply_patch(existing: Option<&str>, patch: &Value) -> Result<String, Vec<FieldError>> {
    let mut object = match existing.map(str::trim).filter(|t| !t.is_empty()) {
        None => Map::new(),
        Some(text) => match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => map,
            Ok(_) => return Err(vec![error("file", "the project file is not a JSON object")]),
            Err(e) => {
                return Err(vec![error(
                    "file",
                    format!("the project file is not valid JSON ({e}); fix it by hand first"),
                )]);
            }
        },
    };
    let mut errors = Vec::new();
    for key in ["setup", "teardown"] {
        match patch.get(key) {
            None => {}
            Some(Value::Null) => {
                object.remove(key);
            }
            Some(Value::String(s)) if s.trim().is_empty() => {
                object.remove(key);
            }
            Some(Value::String(s)) => {
                object.insert(key.to_owned(), Value::String(s.trim().to_owned()));
            }
            Some(_) => errors.push(error(key, "must be text")),
        }
    }
    match patch.get("layout") {
        None => {}
        Some(Value::Null) => {
            object.remove("layout");
        }
        Some(layout) => match parse_layout_value(layout) {
            Ok(_) => {
                object.insert("layout".to_owned(), layout.clone());
            }
            Err(message) => errors.push(error("layout", message)),
        },
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut text = serde_json::to_string_pretty(&Value::Object(object))
        .map_err(|e| vec![error("file", e.to_string())])?;
    text.push('\n');
    Ok(text)
}

/// Validates, checks trust and writes. `trusted` is the user's map; `decision` the answer to an
/// earlier request. Nothing is written on errors or while a trust question is open.
pub fn save(
    root: &str,
    patch: &Value,
    trusted: &BTreeMap<String, String>,
    decision: Option<&Decision>,
) -> Result<SaveOutcome, String> {
    let mut sources = Sources::read(root);
    let existing = sources.project.as_ref().map(|(_, t)| t.clone());
    let text = match apply_patch(existing.as_deref(), patch) {
        Ok(text) => text,
        Err(errors) => {
            return Ok(SaveOutcome {
                errors,
                ..SaveOutcome::default()
            });
        }
    };
    let path = Path::new(root).join(PROJECT_FILE);
    sources.project = Some((path.to_string_lossy().into_owned(), text.clone()));
    let store = match trust::gate(&sources, trusted, decision) {
        Gate::Ask(request) => {
            return Ok(SaveOutcome {
                trust: Some(request),
                ..SaveOutcome::default()
            });
        }
        // The user declined the commands: nothing is saved.
        Gate::Strip => return Ok(SaveOutcome::default()),
        Gate::Run { store } => store,
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(SaveOutcome {
        saved: true,
        store,
        ..SaveOutcome::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-edit-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(root: &Path, text: &str) {
        std::fs::create_dir_all(root.join(".poimen")).unwrap();
        std::fs::write(root.join(PROJECT_FILE), text).unwrap();
    }

    #[test]
    fn round_trip_keeps_unknown_keys_and_other_sections() {
        let existing = r#"{"x-custom": {"a": [1, 2]}, "setup": "old", "commands": [{"name": "t", "command": "pnpm test"}]}"#;
        let text = apply_patch(Some(existing), &json!({"setup": "pnpm install"})).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["x-custom"], json!({"a": [1, 2]}));
        assert_eq!(v["commands"][0]["command"], "pnpm test");
        assert_eq!(v["setup"], "pnpm install");
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn layout_set_and_removed() {
        let layout = json!({"panes": [{"name": "agent", "command": "claude"}]});
        let text = apply_patch(None, &json!({"layout": layout})).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap()["layout"],
            layout
        );
        let text = apply_patch(Some(&text), &json!({"layout": null, "teardown": "  "})).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&text).unwrap(), json!({}));
    }

    #[test]
    fn invalid_fields_are_reported_per_field() {
        let errors = apply_patch(None, &json!({"layout": {"panes": []}, "setup": 3})).unwrap_err();
        let fields: Vec<_> = errors.iter().map(|e| e.field.as_str()).collect();
        assert_eq!(fields, ["setup", "layout"]);
        assert!(errors[1].message.contains("panes"));
    }

    #[test]
    fn a_broken_file_is_never_overwritten() {
        let errors = apply_patch(Some("{ nope"), &json!({"setup": "x"})).unwrap_err();
        assert_eq!(errors[0].field, "file");
        let errors = apply_patch(Some("[1]"), &json!({"setup": "x"})).unwrap_err();
        assert_eq!(errors[0].field, "file");
    }

    #[test]
    fn save_without_commands_writes_and_read_returns_it() {
        let root = temp("plain");
        let root_s = root.to_string_lossy().into_owned();
        assert!(!read(&root_s).exists);
        let patch = json!({"layout": {"panes": [{"name": "sh"}]}});
        let out = save(&root_s, &patch, &BTreeMap::new(), None).unwrap();
        assert!(out.saved && out.store.is_none() && out.trust.is_none());
        let file = read(&root_s);
        assert!(file.exists);
        assert!(file.text.contains("\"sh\""));
    }

    #[test]
    fn save_with_commands_asks_first_and_writes_nothing() {
        let root = temp("ask");
        let root_s = root.to_string_lossy().into_owned();
        write(&root, r#"{"keep": true}"#);
        let patch = json!({"setup": "pnpm install"});
        let out = save(&root_s, &patch, &BTreeMap::new(), None).unwrap();
        assert!(!out.saved);
        let request = out.trust.expect("trust request");
        assert_eq!(request.commands[0].command, "pnpm install");
        assert_eq!(read(&root_s).text, r#"{"keep": true}"#);
    }

    #[test]
    fn save_with_matching_trust_answer_writes_and_stores_the_new_hash() {
        let root = temp("trust");
        let root_s = root.to_string_lossy().into_owned();
        let patch = json!({"setup": "pnpm install"});
        let none = BTreeMap::new();
        let hash = save(&root_s, &patch, &none, None)
            .unwrap()
            .trust
            .unwrap()
            .hash;
        // A stale hash does not count.
        let stale = Decision::Trust { hash: "0".into() };
        assert!(!save(&root_s, &patch, &none, Some(&stale)).unwrap().saved);
        let answer = Decision::Trust { hash: hash.clone() };
        let out = save(&root_s, &patch, &none, Some(&answer)).unwrap();
        assert!(out.saved);
        assert_eq!(out.store.as_deref(), Some(hash.as_str()));
        assert_eq!(trust::sha256_hex(&read(&root_s).text), hash);
    }

    #[test]
    fn save_does_not_write_when_the_user_declines() {
        let root = temp("decline");
        let root_s = root.to_string_lossy().into_owned();
        let patch = json!({"setup": "x"});
        let out = save(&root_s, &patch, &BTreeMap::new(), Some(&Decision::Without)).unwrap();
        assert_eq!(out, SaveOutcome::default());
        assert!(!read(&root_s).exists);
    }

    #[test]
    fn save_with_an_already_trusted_hash_needs_no_question() {
        let root = temp("known");
        let root_s = root.to_string_lossy().into_owned();
        let patch = json!({"setup": "make"});
        let none = BTreeMap::new();
        let hash = save(&root_s, &patch, &none, None)
            .unwrap()
            .trust
            .unwrap()
            .hash;
        let trusted = BTreeMap::from([(root_s.clone(), hash)]);
        let out = save(&root_s, &patch, &trusted, None).unwrap();
        assert!(out.saved && out.store.is_none());
    }
}
