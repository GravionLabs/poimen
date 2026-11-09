//! The pane layout of a new space, in the schema of the herdr-plugins `layout-bootstrap`
//! (`.poimen/config.json` `layout`, formerly `.agents/herdr-layout.json`): `panes[]` with `name`, optional `command` and, from the second
//! pane on, `split.from` / `split.direction` / `split.ratio`. Resolution is pure;
//! `apply` (which needs a backend) lives in the app crate.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

/// Where a split puts the new terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitDirection {
    Right,
    Down,
}

impl SplitDirection {
    /// The wire name (`right`, `down`).
    pub fn as_str(self) -> &'static str {
        match self {
            SplitDirection::Right => "right",
            SplitDirection::Down => "down",
        }
    }
}

/// The deprecated project layout file, relative to the repository root (read until 1.0).
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

/// The `spaces.layout` JSON of a layout: the schema `parse_layout_value` reads.
pub fn to_config(layout: &Layout) -> Value {
    let panes: Vec<Value> = layout
        .panes
        .iter()
        .map(|p| {
            let mut pane = serde_json::json!({ "name": p.name });
            if let Some(command) = &p.command {
                pane["command"] = Value::from(command.as_str());
            }
            if let Some(s) = &p.split {
                let direction = match s.direction {
                    SplitDirection::Right => "right",
                    SplitDirection::Down => "down",
                };
                pane["split"] = serde_json::json!({
                    "from": s.from,
                    "direction": direction,
                    "ratio": s.ratio,
                });
            }
            pane
        })
        .collect();
    serde_json::json!({ "panes": panes })
}

/// Validates the text of a layout file.
pub fn parse_layout(text: &str) -> Result<Layout, String> {
    let data: Value = serde_json::from_str(text).map_err(|_| "invalid JSON".to_owned())?;
    parse_layout_value(&data)
}

/// The layout for a new space: the `layout` of the project file `.poimen/config.json`
/// (`(path, text)`), else the deprecated `.agents/herdr-layout.json` (with a deprecation warning),
/// else the `spaces.layout` of the poimen config, else the built-in default. An invalid layout in
/// a project file is an `Err` with a warning naming the file and the problem; the caller then
/// creates a single pane. The `Ok` warnings are non-fatal.
pub fn resolve(
    project: Option<(&str, &str)>,
    legacy: Option<(&str, &str)>,
    config: Option<&Layout>,
) -> Result<(Layout, Vec<String>), String> {
    resolve_origin(project, legacy, config).map(|(l, w, _)| (l, w))
}

/// `resolve` that also says whether the layout comes from a project file (`true`; its commands need
/// trust) or from the user config or the built-in default (`false`).
pub fn resolve_origin(
    project: Option<(&str, &str)>,
    legacy: Option<(&str, &str)>,
    config: Option<&Layout>,
) -> Result<(Layout, Vec<String>, bool), String> {
    if let Some((path, text)) = project
        && let Ok(data) = serde_json::from_str::<Value>(text)
        && !data["layout"].is_null()
    {
        return parse_layout_value(&data["layout"])
            .map(|l| (l, Vec::new(), true))
            .map_err(|e| format!("{path}: layout: {e}"));
    }
    if let Some((path, text)) = legacy {
        let layout = parse_layout(text).map_err(|e| format!("{path}: {e}"))?;
        let warning = format!(
            "{path} is deprecated; move it to \"layout\" in {}",
            crate::project::PROJECT_FILE
        );
        return Ok((layout, vec![warning], true));
    }
    Ok((
        config.cloned().unwrap_or_else(default_layout),
        Vec::new(),
        false,
    ))
}

impl Layout {
    /// The same layout with `command` as the command of the root pane (the one without a split).
    pub fn with_root_command(&self, command: &str) -> Layout {
        let mut layout = self.clone();
        if let Some(root) = layout.panes.iter_mut().find(|p| p.split.is_none()) {
            root.command = Some(command.to_owned());
        }
        layout
    }

    /// The same panes and splits with no command to type.
    pub fn without_commands(&self) -> Layout {
        Layout {
            panes: self
                .panes
                .iter()
                .map(|p| PaneSpec {
                    command: None,
                    ..p.clone()
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "/work/app/.agents/herdr-layout.json";

    #[test]
    fn split_direction_names_are_the_wire_names() {
        assert_eq!(SplitDirection::Right.as_str(), "right");
        assert_eq!(SplitDirection::Down.as_str(), "down");
    }

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

    const PROJECT: &str = "/work/app/.poimen/config.json";

    #[test]
    fn resolution_order_project_then_deprecated_file_then_config_then_default() {
        let new = r#"{"layout":{"panes":[{"name":"new"}]}}"#;
        let old = r#"{"panes":[{"name":"old"}]}"#;
        let config = parse_layout(r#"{"panes":[{"name":"cfg"}]}"#).unwrap();
        let name = |r: Result<(Layout, Vec<String>), String>| r.unwrap().0.panes[0].name.clone();
        assert_eq!(
            name(resolve(
                Some((PROJECT, new)),
                Some((FILE, old)),
                Some(&config)
            )),
            "new"
        );
        // A project file without `layout` falls through.
        assert_eq!(
            name(resolve(
                Some((PROJECT, "{}")),
                Some((FILE, old)),
                Some(&config)
            )),
            "old"
        );
        assert_eq!(
            name(resolve(Some((PROJECT, "{}")), None, Some(&config))),
            "cfg"
        );
        assert_eq!(name(resolve(None, None, Some(&config))), "cfg");
        assert_eq!(resolve(None, None, None).unwrap().0, default_layout());
    }

    #[test]
    fn origin_is_the_project_for_repo_files_only() {
        let new = r#"{"layout":{"panes":[{"name":"n"}]}}"#;
        let old = r#"{"panes":[{"name":"o"}]}"#;
        let config = default_layout();
        let origin = |p, l, c| resolve_origin(p, l, c).unwrap().2;
        assert!(origin(Some((PROJECT, new)), None, Some(&config)));
        assert!(origin(None, Some((FILE, old)), Some(&config)));
        // A project file without `layout` leaves the user layout in charge.
        assert!(!origin(Some((PROJECT, "{}")), None, Some(&config)));
        assert!(!origin(None, None, None));
    }

    #[test]
    fn without_commands_keeps_panes_and_splits() {
        let l = default_layout().without_commands();
        assert_eq!(l.panes.len(), 3);
        assert!(l.panes.iter().all(|p| p.command.is_none()));
        assert_eq!(l.panes[1].split, default_layout().panes[1].split);
    }

    #[test]
    fn only_the_deprecated_file_warns() {
        let old = r#"{"panes":[{"name":"old"}]}"#;
        let (_, warnings) = resolve(None, Some((FILE, old)), None).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains(FILE) && warnings[0].contains("deprecated"));
        let new = r#"{"layout":{"panes":[{"name":"n"}]}}"#;
        let (_, none) = resolve(Some((PROJECT, new)), Some((FILE, old)), None).unwrap();
        assert!(none.is_empty());
    }

    #[test]
    fn an_invalid_layout_is_a_warning_naming_file_and_problem() {
        let config = default_layout();
        let err = resolve(
            Some((PROJECT, r#"{"layout":{"panes":[]}}"#)),
            None,
            Some(&config),
        )
        .unwrap_err();
        assert!(
            err.starts_with(PROJECT) && err.contains("non-empty list"),
            "{err}"
        );
        let err = resolve(None, Some((FILE, r#"{"panes":[]}"#)), Some(&config)).unwrap_err();
        assert!(
            err.starts_with(FILE) && err.contains("non-empty list"),
            "{err}"
        );
    }
}
