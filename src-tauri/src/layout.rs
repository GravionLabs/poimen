//! The pane layout of a new space, in the schema of the herdr-plugins `layout-bootstrap`
//! (`.agents/herdr-layout.json`): `panes[]` with `name`, optional `command` and, from the second
//! pane on, `split.from` / `split.direction` / `split.ratio`. Resolution is pure; `apply` takes the
//! request function so tests never touch a live herdr.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

use crate::herdr::{
    SplitDirection, created_pane_id, pane_rename_params, send_input_params, split_params_ratio,
};

/// The project layout file, relative to the repository root (or the directory outside a repo).
pub const REPO_LAYOUT_FILE: &str = ".agents/herdr-layout.json";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Split {
    pub from: String,
    pub direction: SplitDirection,
    /// The share the split pane keeps; the new pane gets the rest.
    pub ratio: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PaneSpec {
    pub name: String,
    pub command: Option<String>,
    /// `None` for the first pane, which is the root pane of the new space.
    pub split: Option<Split>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Layout {
    pub panes: Vec<PaneSpec>,
}

/// The built-in layout: `agent` left (60 %) running `claude`, `tests` right top, `shell` right bottom.
pub fn default_layout() -> Layout {
    let pane =
        |name: &str, command: Option<&str>, split: Option<(&str, SplitDirection, f64)>| PaneSpec {
            name: name.into(),
            command: command.map(str::to_owned),
            split: split.map(|(from, direction, ratio)| Split {
                from: from.into(),
                direction,
                ratio,
            }),
        };
    Layout {
        panes: vec![
            pane("agent", Some("claude"), None),
            pane("tests", None, Some(("agent", SplitDirection::Right, 0.6))),
            pane("shell", None, Some(("tests", SplitDirection::Down, 0.5))),
        ],
    }
}

/// Validates a parsed layout; the error names the problem.
pub fn parse_layout_value(data: &Value) -> Result<Layout, String> {
    let list = data["panes"]
        .as_array()
        .filter(|l| !l.is_empty())
        .ok_or("\"panes\" must be a non-empty list")?;
    let mut panes: Vec<PaneSpec> = Vec::new();
    let mut names: HashSet<String> = HashSet::new();
    for (index, raw) in list.iter().enumerate() {
        if !raw.is_object() {
            return Err(format!("pane {}: expected an object", index + 1));
        }
        let name = raw["name"].as_str().map(str::trim).unwrap_or_default();
        if name.is_empty() {
            return Err(format!("pane {}: \"name\" is required", index + 1));
        }
        if names.contains(name) {
            return Err(format!("pane \"{name}\": duplicate name"));
        }
        let command = match &raw["command"] {
            Value::Null => None,
            Value::String(c) => Some(c.trim().to_owned()).filter(|c| !c.is_empty()),
            _ => return Err(format!("pane \"{name}\": \"command\" must be a string")),
        };
        let split = if index == 0 {
            if !raw["split"].is_null() {
                return Err(format!(
                    "pane \"{name}\": the first pane must not have \"split\""
                ));
            }
            None
        } else {
            let s = raw["split"]
                .as_object()
                .ok_or(format!("pane \"{name}\": \"split\" is required"))?;
            let from = match s.get("from") {
                Some(Value::String(f)) => f.clone(),
                _ => panes[index - 1].name.clone(),
            };
            if !names.contains(&from) {
                return Err(format!(
                    "pane \"{name}\": split.from \"{from}\" is not an earlier pane"
                ));
            }
            let direction = match s.get("direction").and_then(Value::as_str) {
                Some("right") => SplitDirection::Right,
                Some("down") => SplitDirection::Down,
                _ => {
                    return Err(format!(
                        "pane \"{name}\": split.direction must be \"right\" or \"down\""
                    ));
                }
            };
            let ratio = match s.get("ratio") {
                None => 0.5,
                Some(r) => r.as_f64().filter(|r| *r > 0.0 && *r < 1.0).ok_or(format!(
                    "pane \"{name}\": split.ratio must be between 0 and 1"
                ))?,
            };
            Some(Split {
                from,
                direction,
                ratio,
            })
        };
        names.insert(name.to_owned());
        panes.push(PaneSpec {
            name: name.to_owned(),
            command,
            split,
        });
    }
    Ok(Layout { panes })
}

/// Validates the text of a layout file.
pub fn parse_layout(text: &str) -> Result<Layout, String> {
    let data: Value = serde_json::from_str(text).map_err(|_| "invalid JSON".to_owned())?;
    parse_layout_value(&data)
}

/// The layout for a new space: the project file (`(path, text)`), else the `spaces.layout` of the
/// poimen config, else the built-in default. An invalid project file is an `Err` with a warning
/// naming the file and the problem; the caller then creates a single pane.
pub fn resolve(repo: Option<(&str, &str)>, config: Option<&Layout>) -> Result<Layout, String> {
    match repo {
        Some((path, text)) => parse_layout(text).map_err(|e| format!("{path}: {e}")),
        None => Ok(config.cloned().unwrap_or_else(default_layout)),
    }
}

/// Builds the layout in the new space: the root pane is named and gets its command, every other
/// pane splits an earlier one (`pane.split` in `cwd`, without focus), is named and gets its
/// command typed. `request` sends one herdr call.
pub fn apply(
    request: &impl Fn(&str, Value) -> Result<Value, String>,
    layout: &Layout,
    cwd: &str,
    root_pane: &str,
) -> Result<(), String> {
    let mut ids: Vec<(&str, String)> = Vec::new();
    for spec in &layout.panes {
        let id = match &spec.split {
            None => root_pane.to_owned(),
            Some(s) => {
                let target = ids
                    .iter()
                    .find(|(n, _)| *n == s.from)
                    .map(|(_, id)| id.as_str())
                    .ok_or(format!("pane {}: unknown split.from {}", spec.name, s.from))?;
                let params = split_params_ratio(target, s.direction, Some(cwd), Some(s.ratio));
                let result = request("pane.split", params)?;
                created_pane_id(&result).ok_or("herdr did not report the new pane")?
            }
        };
        request("pane.rename", pane_rename_params(&id, Some(&spec.name)))?;
        if let Some(command) = &spec.command {
            request("pane.send_input", send_input_params(&id, command))?;
        }
        ids.push((&spec.name, id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::RefCell;

    const FILE: &str = "/work/app/.agents/herdr-layout.json";

    #[test]
    fn default_is_agent_left_tests_right_top_shell_right_bottom() {
        let l = default_layout();
        let names: Vec<_> = l.panes.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["agent", "tests", "shell"]);
        assert_eq!(l.panes[0].command.as_deref(), Some("claude"));
        assert_eq!(l.panes[0].split, None);
        let tests = l.panes[1].split.as_ref().unwrap();
        assert_eq!(
            (tests.from.as_str(), tests.direction, tests.ratio),
            ("agent", SplitDirection::Right, 0.6)
        );
        let shell = l.panes[2].split.as_ref().unwrap();
        assert_eq!(
            (shell.from.as_str(), shell.direction),
            ("tests", SplitDirection::Down)
        );
    }

    #[test]
    fn parses_a_valid_layout_with_defaults() {
        let l = parse_layout(
            r#"{"panes":[{"name":"a","command":" vim "},
                {"name":"b","split":{"direction":"down"}},
                {"name":"c","command":"","split":{"from":"a","direction":"right","ratio":0.3}}]}"#,
        )
        .unwrap();
        assert_eq!(l.panes[0].command.as_deref(), Some("vim"));
        let b = l.panes[1].split.as_ref().unwrap();
        assert_eq!((b.from.as_str(), b.ratio), ("a", 0.5));
        assert_eq!(l.panes[2].command, None);
        let c = l.panes[2].split.as_ref().unwrap();
        assert_eq!((c.from.as_str(), c.ratio), ("a", 0.3));
    }

    #[test]
    fn rejects_invalid_layouts_naming_the_problem() {
        let cases = [
            ("not json", "invalid JSON"),
            (r#"{"panes":[]}"#, "non-empty list"),
            (r#"{"panes":[{"command":"x"}]}"#, "\"name\" is required"),
            (r#"{"panes":[1]}"#, "expected an object"),
            (
                r#"{"panes":[{"name":"a","command":3}]}"#,
                "\"command\" must be a string",
            ),
            (
                r#"{"panes":[{"name":"a","split":{"direction":"down"}}]}"#,
                "must not have \"split\"",
            ),
            (
                r#"{"panes":[{"name":"a"},{"name":"b"}]}"#,
                "\"split\" is required",
            ),
            (
                r#"{"panes":[{"name":"a"},{"name":"a","split":{"direction":"down"}}]}"#,
                "duplicate name",
            ),
            (
                r#"{"panes":[{"name":"a"},{"name":"b","split":{"from":"z","direction":"down"}}]}"#,
                "not an earlier pane",
            ),
            (
                r#"{"panes":[{"name":"a"},{"name":"b","split":{"direction":"left"}}]}"#,
                "\"right\" or \"down\"",
            ),
            (
                r#"{"panes":[{"name":"a"},{"name":"b","split":{"direction":"down","ratio":1}}]}"#,
                "between 0 and 1",
            ),
        ];
        for (text, problem) in cases {
            let err = parse_layout(text).unwrap_err();
            assert!(err.contains(problem), "{text}: {err}");
        }
    }

    #[test]
    fn resolution_order_project_file_then_config_then_default() {
        let project = r#"{"panes":[{"name":"solo"}]}"#;
        let config = parse_layout(r#"{"panes":[{"name":"cfg"}]}"#).unwrap();
        let from_file = resolve(Some((FILE, project)), Some(&config)).unwrap();
        assert_eq!(from_file.panes[0].name, "solo");
        let from_config = resolve(None, Some(&config)).unwrap();
        assert_eq!(from_config.panes[0].name, "cfg");
        assert_eq!(resolve(None, None).unwrap(), default_layout());
    }

    #[test]
    fn an_invalid_project_file_is_a_warning_naming_file_and_problem() {
        let config = default_layout();
        let err = resolve(Some((FILE, r#"{"panes":[]}"#)), Some(&config)).unwrap_err();
        assert!(
            err.starts_with(FILE) && err.contains("non-empty list"),
            "{err}"
        );
    }

    fn fake(
        calls: &RefCell<Vec<(String, Value)>>,
    ) -> impl Fn(&str, Value) -> Result<Value, String> + '_ {
        move |method, params| {
            let mut calls = calls.borrow_mut();
            let n = calls.len();
            calls.push((method.to_owned(), params));
            Ok(match method {
                "pane.split" => json!({ "pane": { "pane_id": format!("w1:p{}", n + 10) } }),
                _ => json!({}),
            })
        }
    }

    #[test]
    fn apply_splits_renames_and_types_in_order() {
        let calls = RefCell::new(Vec::new());
        apply(&fake(&calls), &default_layout(), "/work/app", "w1:root").unwrap();
        let calls = calls.into_inner();
        let methods: Vec<_> = calls.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(
            methods,
            [
                "pane.rename",
                "pane.send_input",
                "pane.split",
                "pane.rename",
                "pane.split",
                "pane.rename"
            ]
        );
        assert_eq!(
            calls[0].1,
            json!({ "pane_id": "w1:root", "label": "agent" })
        );
        assert_eq!(
            calls[1].1,
            json!({ "pane_id": "w1:root", "text": "claude", "keys": ["enter"] })
        );
        assert_eq!(
            calls[2].1,
            json!({ "target_pane_id": "w1:root", "direction": "right", "cwd": "/work/app", "ratio": 0.6 })
        );
        assert_eq!(calls[3].1, json!({ "pane_id": "w1:p12", "label": "tests" }));
        // `shell` splits `tests` (the pane created by the first split), downward.
        assert_eq!(
            calls[4].1,
            json!({ "target_pane_id": "w1:p12", "direction": "down", "cwd": "/work/app", "ratio": 0.5 })
        );
        assert_eq!(calls[5].1, json!({ "pane_id": "w1:p14", "label": "shell" }));
    }

    #[test]
    fn apply_stops_on_a_failed_call() {
        let request = |method: &str, _: Value| -> Result<Value, String> {
            if method == "pane.split" {
                Err("boom".into())
            } else {
                Ok(json!({}))
            }
        };
        assert_eq!(
            apply(&request, &default_layout(), "/work/app", "w1:root"),
            Err("boom".into())
        );
    }
}
