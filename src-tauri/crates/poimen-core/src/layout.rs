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

/// A node of a tab's split tree (ADR 0011), as in the server protocol; `split_id` is the server's.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Node {
    Terminal {
        name: Option<String>,
        command: Option<String>,
        cwd: Option<String>,
    },
    Split {
        direction: SplitDirection,
        /// The share of the first child.
        ratio: f64,
        children: Box<[Node; 2]>,
    },
}

/// App-only placement of a tab in an editor group (the TUI and the server ignore it).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Place {
    pub direction: SplitDirection,
    /// A tab of the same layout; `None` is the parent tab (the first tab for a top-level one).
    pub of: Option<String>,
    pub ratio: f64,
}

/// One tab of a layout. A flat tab (one terminal) is read as a `root` leaf.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Tab {
    pub name: Option<String>,
    pub root: Node,
    pub children: Vec<Tab>,
    pub place: Option<Place>,
}

/// An entry of `layouts[]` in the user config.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NamedLayout {
    pub name: String,
    pub layout: TabLayout,
}

/// The name `spaces.layout` is read as.
pub const DEFAULT_NAME: &str = "default";

/// A layout in the shape of the server model: tabs, each with a split tree and child tabs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TabLayout {
    pub tabs: Vec<Tab>,
}

impl Layout {
    /// The `panes[]` layout as one tab (ADR 0011): the first pane is the root leaf, every later
    /// pane replaces the leaf named by `split.from` with a split of that leaf and the new pane.
    pub fn to_tabs(&self) -> TabLayout {
        let leaf = |p: &PaneSpec| Node::Terminal {
            name: Some(p.name.clone()),
            command: p.command.clone(),
            cwd: None,
        };
        let mut root = self.panes.first().map(leaf).unwrap_or(Node::Terminal {
            name: None,
            command: None,
            cwd: None,
        });
        for pane in self.panes.iter().skip(1) {
            if let Some(s) = &pane.split {
                replace_leaf(&mut root, &s.from, s.direction, s.ratio, leaf(pane));
            }
        }
        TabLayout {
            tabs: vec![Tab {
                name: self.panes.first().map(|p| p.name.clone()),
                root,
                children: Vec::new(),
                place: None,
            }],
        }
    }
}

fn replace_leaf(
    node: &mut Node,
    from: &str,
    direction: SplitDirection,
    ratio: f64,
    new: Node,
) -> Option<Node> {
    match node {
        Node::Terminal { name, .. } if name.as_deref() == Some(from) => {
            let old = std::mem::replace(
                node,
                Node::Terminal {
                    name: None,
                    command: None,
                    cwd: None,
                },
            );
            *node = Node::Split {
                direction,
                ratio,
                children: Box::new([old, new]),
            };
            None
        }
        Node::Terminal { .. } => Some(new),
        Node::Split { children, .. } => {
            let [a, b] = &mut **children;
            let back = replace_leaf(a, from, direction, ratio, new)?;
            replace_leaf(b, from, direction, ratio, back)
        }
    }
}

/// The built-in default as tabs (ADR 0011).
pub fn default_tabs() -> TabLayout {
    default_layout().to_tabs()
}

fn opt_text(raw: &Value, key: &str, at: &str) -> Result<Option<String>, String> {
    match &raw[key] {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s.trim().to_owned()).filter(|s| !s.is_empty())),
        _ => Err(format!("{at}: \"{key}\" must be a string")),
    }
}

fn parse_node(raw: &Value, at: &str) -> Result<Node, String> {
    match raw["kind"].as_str() {
        Some("terminal") => Ok(Node::Terminal {
            name: opt_text(raw, "name", at)?,
            command: opt_text(raw, "command", at)?,
            cwd: opt_text(raw, "cwd", at)?,
        }),
        Some("split") => {
            let direction = match raw["direction"].as_str() {
                Some("right") => SplitDirection::Right,
                Some("down") => SplitDirection::Down,
                _ => return Err(format!("{at}: direction must be \"right\" or \"down\"")),
            };
            let ratio = match raw.get("ratio") {
                None | Some(Value::Null) => 0.5,
                Some(r) => r
                    .as_f64()
                    .filter(|r| *r > 0.0 && *r < 1.0)
                    .ok_or(format!("{at}: ratio must be between 0 and 1"))?,
            };
            let kids = raw["children"]
                .as_array()
                .filter(|c| c.len() == 2)
                .ok_or(format!("{at}: a split needs exactly two children"))?;
            Ok(Node::Split {
                direction,
                ratio,
                children: Box::new([parse_node(&kids[0], at)?, parse_node(&kids[1], at)?]),
            })
        }
        _ => Err(format!("{at}: node kind must be \"terminal\" or \"split\"")),
    }
}

fn parse_place(raw: &Value, at: &str) -> Result<Place, String> {
    let direction = match raw["direction"].as_str() {
        Some("right") => SplitDirection::Right,
        Some("down") => SplitDirection::Down,
        _ => {
            return Err(format!(
                "{at}: place.direction must be \"right\" or \"down\""
            ));
        }
    };
    let ratio = match raw.get("ratio") {
        None | Some(Value::Null) => 0.5,
        Some(r) => r
            .as_f64()
            .filter(|r| *r > 0.0 && *r < 1.0)
            .ok_or(format!("{at}: place.ratio must be between 0 and 1"))?,
    };
    let of = opt_text(raw, "of", at)?;
    Ok(Place {
        direction,
        of,
        ratio,
    })
}

fn parse_tab(raw: &Value, index: usize, warnings: &mut Vec<String>) -> Result<Tab, String> {
    if !raw.is_object() {
        return Err(format!("tab {}: expected an object", index + 1));
    }
    let name = opt_text(raw, "name", &format!("tab {}", index + 1))?;
    let at = match &name {
        Some(n) => format!("tab \"{n}\""),
        None => format!("tab {}", index + 1),
    };
    let flat = ["command", "cwd"].iter().any(|k| !raw[*k].is_null());
    let root = if raw.get("root").is_some_and(|r| !r.is_null()) {
        if flat {
            return Err(format!(
                "{at}: use either \"command\"/\"cwd\" or \"root\", not both"
            ));
        }
        parse_node(&raw["root"], &at)?
    } else {
        Node::Terminal {
            name: name.clone(),
            command: opt_text(raw, "command", &at)?,
            cwd: opt_text(raw, "cwd", &at)?,
        }
    };
    let place = match raw.get("place") {
        None | Some(Value::Null) => None,
        Some(p) => match parse_place(p, &at) {
            Ok(p) => Some(p),
            Err(e) => {
                warnings.push(format!("{e}; place ignored"));
                None
            }
        },
    };
    let children = match raw.get("children") {
        None | Some(Value::Null) => Vec::new(),
        Some(c) => parse_tabs_list(c, warnings)?,
    };
    Ok(Tab {
        name,
        root,
        children,
        place,
    })
}

fn parse_tabs_list(raw: &Value, warnings: &mut Vec<String>) -> Result<Vec<Tab>, String> {
    let list = raw.as_array().ok_or("\"tabs\" must be a list")?;
    list.iter()
        .enumerate()
        .map(|(i, t)| parse_tab(t, i, warnings))
        .collect()
}

/// Names the `place.of` of tabs that point nowhere, or that form a cycle, and drops those places.
fn check_places(tabs: &mut [Tab], warnings: &mut Vec<String>) {
    // (name, parent index) of every tab in document order.
    fn flatten(
        tabs: &[Tab],
        parent: Option<usize>,
        out: &mut Vec<(Option<String>, Option<usize>)>,
    ) {
        for t in tabs {
            let me = out.len();
            out.push((t.name.clone(), parent));
            flatten(&t.children, Some(me), out);
        }
    }
    fn places<'a>(tabs: &'a mut [Tab], out: &mut Vec<&'a mut Option<Place>>) {
        for t in tabs {
            out.push(&mut t.place);
            places(&mut t.children, out);
        }
    }
    let mut info = Vec::new();
    flatten(tabs, None, &mut info);
    let target = |i: usize, of: &Option<String>| -> Option<usize> {
        match of {
            Some(n) => info.iter().position(|(m, _)| m.as_deref() == Some(n)),
            None => info[i].1.or(if i == 0 { None } else { Some(0) }),
        }
    };
    let mut wanted: Vec<Option<Option<String>>> = Vec::new();
    let mut slots = Vec::new();
    places(tabs, &mut slots);
    for slot in &slots {
        wanted.push(slot.as_ref().map(|p| p.of.clone()));
    }
    let mut drop = vec![false; wanted.len()];
    for (i, of) in wanted.iter().enumerate() {
        let Some(of) = of else { continue };
        let label = info[i]
            .0
            .clone()
            .unwrap_or_else(|| format!("tab {}", i + 1));
        if let Some(n) = of
            && target(i, of).is_none()
        {
            warnings.push(format!(
                "tab \"{label}\": place.of \"{n}\" is not a tab of the layout; place ignored"
            ));
            drop[i] = true;
            continue;
        }
        let mut at = i;
        for _ in 0..=info.len() {
            let Some(Some(next_of)) = wanted.get(at) else {
                break;
            };
            match target(at, next_of) {
                Some(n) if n == i => {
                    warnings.push(format!("tab \"{label}\": place is a cycle; place ignored"));
                    drop[i] = true;
                    break;
                }
                Some(n) => at = n,
                None => break,
            }
        }
    }
    for (slot, d) in slots.iter_mut().zip(drop) {
        if d {
            **slot = None;
        }
    }
}

/// Validates a layout object in either shape: `tabs[]` (ADR 0011) or today's `panes[]`, which is
/// converted. Invalid `place`s warn and are dropped; anything else invalid is an `Err`.
pub fn parse_tabs_value(data: &Value) -> Result<(TabLayout, Vec<String>), String> {
    let mut warnings = Vec::new();
    if data.get("tabs").is_none() {
        return parse_layout_value(data).map(|l| (l.to_tabs(), warnings));
    }
    let mut tabs = parse_tabs_list(&data["tabs"], &mut warnings)?;
    if tabs.is_empty() {
        return Err("\"tabs\" must be a non-empty list".into());
    }
    check_places(&mut tabs, &mut warnings);
    Ok((TabLayout { tabs }, warnings))
}

impl TabLayout {
    /// The same layout with `command` on the first terminal of the first tab (the root terminal).
    pub fn with_root_command(&self, command: &str) -> TabLayout {
        fn first(node: &mut Node) -> &mut Node {
            match node {
                Node::Split { children, .. } => first(&mut children[0]),
                leaf => leaf,
            }
        }
        let mut out = self.clone();
        if let Some(tab) = out.tabs.first_mut()
            && let Node::Terminal { command: c, .. } = first(&mut tab.root)
        {
            *c = Some(command.to_owned());
        }
        out
    }

    /// The same tabs and splits with no command to type.
    pub fn without_commands(&self) -> TabLayout {
        fn strip(node: &mut Node) {
            match node {
                Node::Terminal { command, .. } => *command = None,
                Node::Split { children, .. } => children.iter_mut().for_each(strip),
            }
        }
        fn tabs(list: &mut [Tab]) {
            for t in list {
                strip(&mut t.root);
                tabs(&mut t.children);
            }
        }
        let mut out = self.clone();
        tabs(&mut out.tabs);
        out
    }
}

/// [`resolve_origin`] in the tab shape: the project file and the deprecated file (both `panes[]`
/// or `tabs[]`), else `config` (the template's layout or `spaces.layout`), else the built-in
/// default. `panes[]` layouts are converted on read.
pub fn resolve_origin_tabs(
    project: Option<(&str, &str)>,
    legacy: Option<(&str, &str)>,
    config: Option<&TabLayout>,
) -> Result<(TabLayout, Vec<String>, bool), String> {
    if let Some((path, text)) = project
        && let Ok(data) = serde_json::from_str::<Value>(text)
        && !data["layout"].is_null()
    {
        return parse_tabs_value(&data["layout"])
            .map(|(l, w)| (l, w, true))
            .map_err(|e| format!("{path}: layout: {e}"));
    }
    if let Some((path, text)) = legacy {
        let data: Value =
            serde_json::from_str(text).map_err(|_| format!("{path}: invalid JSON"))?;
        let (layout, mut warnings) = parse_tabs_value(&data).map_err(|e| format!("{path}: {e}"))?;
        warnings.push(format!(
            "{path} is deprecated; move it to \"layout\" in {}",
            crate::project::PROJECT_FILE
        ));
        return Ok((layout, warnings, true));
    }
    Ok((
        config.cloned().unwrap_or_else(default_tabs),
        Vec::new(),
        false,
    ))
}

/// [`resolve_origin_tabs`] without the origin.
pub fn resolve_tabs(
    project: Option<(&str, &str)>,
    legacy: Option<(&str, &str)>,
    config: Option<&TabLayout>,
) -> Result<(TabLayout, Vec<String>), String> {
    resolve_origin_tabs(project, legacy, config).map(|(l, w, _)| (l, w))
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

    fn leaf(name: &str, command: Option<&str>) -> Node {
        Node::Terminal {
            name: Some(name.into()),
            command: command.map(str::to_owned),
            cwd: None,
        }
    }

    #[test]
    fn the_default_panes_convert_to_the_default_tab_of_the_adr() {
        let tabs = default_layout().to_tabs();
        assert_eq!(tabs.tabs.len(), 1);
        assert_eq!(tabs.tabs[0].name.as_deref(), Some("agent"));
        assert_eq!(
            tabs.tabs[0].root,
            Node::Split {
                direction: SplitDirection::Right,
                ratio: 0.6,
                children: Box::new([
                    leaf("agent", Some("claude")),
                    Node::Split {
                        direction: SplitDirection::Down,
                        ratio: 0.5,
                        children: Box::new([leaf("tests", None), leaf("shell", None)]),
                    },
                ]),
            }
        );
    }

    #[test]
    fn a_split_from_the_first_pane_nests_inside_the_earlier_split() {
        let l = parse_layout(
            r#"{"panes":[{"name":"a"},
                {"name":"b","split":{"direction":"right"}},
                {"name":"c","split":{"from":"a","direction":"down","ratio":0.3}}]}"#,
        )
        .unwrap();
        let Node::Split { children, .. } = &l.to_tabs().tabs[0].root else {
            panic!("split");
        };
        assert_eq!(
            children[0],
            Node::Split {
                direction: SplitDirection::Down,
                ratio: 0.3,
                children: Box::new([leaf("a", None), leaf("c", None)]),
            }
        );
        assert_eq!(children[1], leaf("b", None));
    }

    #[test]
    fn one_pane_is_a_flat_tab() {
        let (l, w) =
            parse_tabs_value(&serde_json::json!({"panes":[{"name":"solo","command":"vim"}]}))
                .unwrap();
        assert!(w.is_empty());
        assert_eq!(l.tabs[0].root, leaf("solo", Some("vim")));
    }

    #[test]
    fn tabs_parse_flat_split_and_nested_with_place() {
        let (l, w) = parse_tabs_value(&serde_json::json!({"tabs":[
            {"name":"claude","command":"claude","children":[
                {"name":"Terminal","place":{"direction":"right","ratio":0.6}},
                {"name":"Logs","place":{"direction":"down","of":"Terminal"}}]},
            {"name":"review","root":{"kind":"split","direction":"down","children":[
                {"kind":"terminal","command":"copilot"},{"kind":"terminal","cwd":"sub"}]}}
        ]}))
        .unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(l.tabs.len(), 2);
        assert_eq!(l.tabs[0].root, leaf("claude", Some("claude")));
        let kids = &l.tabs[0].children;
        assert_eq!(kids[0].place.as_ref().unwrap().ratio, 0.6);
        assert_eq!(
            kids[1].place.as_ref().unwrap().of.as_deref(),
            Some("Terminal")
        );
        let Node::Split {
            ratio, children, ..
        } = &l.tabs[1].root
        else {
            panic!("split");
        };
        assert_eq!(*ratio, 0.5);
        assert_eq!(
            children[1],
            Node::Terminal {
                name: None,
                command: None,
                cwd: Some("sub".into())
            }
        );
    }

    #[test]
    fn invalid_tabs_are_errors_naming_the_tab() {
        let err = |v: Value| parse_tabs_value(&v).unwrap_err();
        assert!(err(serde_json::json!({"tabs":[]})).contains("non-empty"));
        let both = err(serde_json::json!({"tabs":[{"name":"x","command":"a",
            "root":{"kind":"terminal"}}]}));
        assert!(
            both.contains("\"x\"") && both.contains("not both"),
            "{both}"
        );
        assert!(
            err(
                serde_json::json!({"tabs":[{"root":{"kind":"split","direction":"up",
            "children":[]}}]})
            )
            .contains("direction")
        );
        assert!(
            err(
                serde_json::json!({"tabs":[{"root":{"kind":"split","direction":"right",
            "ratio":1.0,"children":[{"kind":"terminal"},{"kind":"terminal"}]}}]})
            )
            .contains("ratio")
        );
        assert!(
            err(
                serde_json::json!({"tabs":[{"root":{"kind":"split","direction":"right",
            "children":[{"kind":"terminal"}]}}]})
            )
            .contains("two children")
        );
    }

    #[test]
    fn a_bad_place_warns_and_is_ignored_while_the_tab_stays() {
        let (l, w) = parse_tabs_value(&serde_json::json!({"tabs":[
            {"name":"a","children":[
                {"name":"b","place":{"direction":"right","of":"nowhere"}},
                {"name":"c","place":{"direction":"sideways"}},
                {"name":"d","place":{"direction":"right","of":"e"}},
                {"name":"e","place":{"direction":"down","of":"d"}}]}]}))
        .unwrap();
        assert_eq!(w.len(), 4, "{w:?}");
        assert!(l.tabs[0].children[0].place.is_none());
        assert!(l.tabs[0].children[1].place.is_none());
        assert_eq!(l.tabs[0].children.len(), 4);
    }

    #[test]
    fn resolve_tabs_converts_panes_of_project_file_and_prefers_it_to_config() {
        let project = r#"{"layout":{"panes":[{"name":"p"}]}}"#;
        let config = parse_tabs_value(&serde_json::json!({"tabs":[{"name":"cfg"}]}))
            .unwrap()
            .0;
        let (l, _, from_project) =
            resolve_origin_tabs(Some(("p.json", project)), None, Some(&config)).unwrap();
        assert!(from_project);
        assert_eq!(l.tabs[0].name.as_deref(), Some("p"));
        let (l, _, from_project) = resolve_origin_tabs(None, None, Some(&config)).unwrap();
        assert!(!from_project);
        assert_eq!(l, config);
        assert_eq!(resolve_tabs(None, None, None).unwrap().0, default_tabs());
        let err =
            resolve_tabs(Some(("p.json", r#"{"layout":{"tabs":[]}}"#)), None, None).unwrap_err();
        assert!(err.starts_with("p.json: layout:"), "{err}");
    }

    #[test]
    fn root_command_and_stripping_work_on_tabs() {
        let t = default_tabs().with_root_command("copilot");
        let Node::Split { children, .. } = &t.tabs[0].root else {
            panic!()
        };
        assert_eq!(children[0], leaf("agent", Some("copilot")));
        let t = t.without_commands();
        let Node::Split { children, .. } = &t.tabs[0].root else {
            panic!()
        };
        assert_eq!(children[0], leaf("agent", None));
    }
}
