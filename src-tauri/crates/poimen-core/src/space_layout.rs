//! The layout a new space gets, shared by the app and `poimen-server`: what the user config says
//! about it (`trustedProjects`, `spaceTemplates`, `spaces.layout`, `harnesses`), the template and
//! its agent for a directory, and the plan (project file > deprecated layout file > template >
//! `spaces.layout` > default, a task's root command over all). Building the panes is the caller's.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::harness::{self, Harness};
use crate::layout::{self, Layout, NamedLayout, TabLayout};
use crate::remote::{self, Remote};
use crate::settings::{config_path, parse_layouts, parse_space_templates, parse_trusted_projects};
use crate::templates::{self, Template};
use crate::trust::Sources;

/// The parts of the user config a new space's layout depends on.
#[derive(Debug, Clone, PartialEq)]
pub struct UserSpaces {
    pub trusted: BTreeMap<String, String>,
    pub templates: Vec<Template>,
    pub spaces_layout: Option<Layout>,
    /// `layouts[]` (ADR 0011).
    pub layouts: Vec<NamedLayout>,
    pub harnesses: Vec<Harness>,
    pub warnings: Vec<String>,
}

impl Default for UserSpaces {
    fn default() -> Self {
        Self {
            trusted: BTreeMap::new(),
            templates: Vec::new(),
            spaces_layout: None,
            layouts: Vec::new(),
            harnesses: harness::default_harnesses(),
            warnings: Vec::new(),
        }
    }
}

impl UserSpaces {
    /// Parses the text of `config.json`; anything invalid warns and keeps the default.
    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        let root = match serde_json::from_str::<Value>(text) {
            Ok(v @ Value::Object(_)) => v,
            _ => {
                s.warnings
                    .push("config.json is not a valid JSON object; using defaults".into());
                return s;
            }
        };
        if let Some(v) = root.get("trustedProjects") {
            s.trusted = parse_trusted_projects(v, &mut s.warnings);
        }
        if let Some(layout) = root.get("spaces").and_then(|v| v.get("layout")) {
            match layout::parse_layout_value(layout) {
                Ok(l) => s.spaces_layout = Some(l),
                Err(e) => s
                    .warnings
                    .push(format!("spaces.layout: {e}; using the default layout")),
            }
        }
        if let Some(v) = root.get("layouts") {
            s.layouts = parse_layouts(v, &mut s.warnings);
            if s.spaces_layout.is_some() && s.layouts.iter().any(|l| l.name == layout::DEFAULT_NAME)
            {
                s.warnings.push(
                    "spaces.layout is ignored: layouts[] has a layout named \"default\"".into(),
                );
            }
        }
        if let Some(v) = root.get("spaceTemplates") {
            s.templates = parse_space_templates(v, &mut s.warnings);
        }
        if let Some(v) = root.get("harnesses") {
            s.harnesses = harness::resolve(v, &mut s.warnings);
        }
        s
    }

    /// The layout named `name`: `layouts[]`, else `spaces.layout` for `default` (read on the fly,
    /// the file is never migrated).
    pub fn layout_named(&self, name: &str) -> Option<TabLayout> {
        self.layouts
            .iter()
            .find(|l| l.name == name)
            .map(|l| l.layout.clone())
            .or_else(|| {
                (name == layout::DEFAULT_NAME)
                    .then(|| self.spaces_layout.as_ref().map(Layout::to_tabs))
                    .flatten()
            })
    }

    /// The user layout for a space whose matching template is `template`: its inline layout, else
    /// the layout it names (an unknown name warns and falls through), else the template's absent
    /// layout or none: the layout `default`. `None` is the built-in default.
    pub fn layout_for(
        &self,
        template: Option<&Template>,
        warnings: &mut Vec<String>,
    ) -> Option<TabLayout> {
        if let Some(t) = template {
            if let Some(l) = &t.layout {
                return Some(l.to_tabs());
            }
            if let Some(n) = &t.layout_name {
                match self.layout_named(n) {
                    Some(l) => return Some(l),
                    None => warnings.push(format!(
                        "template \"{}\": unknown layout \"{n}\"; using the default",
                        t.name
                    )),
                }
            }
        }
        self.layout_named(layout::DEFAULT_NAME)
    }

    /// Reads `config.json` under `home` now (it is the only source, so every new space reads it
    /// again); a missing file gives the defaults.
    pub fn load(home: &Path) -> Self {
        match std::fs::read_to_string(config_path(home)) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => Self {
                warnings: vec![format!("cannot read config.json: {e}")],
                ..Self::default()
            },
        }
    }
}

/// The template for `dir`; the `origin` of its repository is read only when a template matches
/// by remote.
pub fn template_for<'a>(
    dir: &str,
    templates: &'a [Template],
    home: Option<&Path>,
) -> Option<&'a Template> {
    let remote: Option<Remote> = templates
        .iter()
        .any(|t| t.remote.is_some())
        .then(|| crate::git::origin_url(dir).and_then(|u| remote::parse(&u)))
        .flatten();
    templates::resolve(dir, remote.as_ref(), templates, home)
}

/// The template's agent command and the user layout in the tab shape (ADR 0011: the template's
/// inline layout or the one it names, else `spaces.layout` as `default`) for a space of `dir`, with
/// the warnings of the template and of an unknown layout name. `available` says whether a program
/// is on the `PATH`; it is asked only for the agent of a matching template.
pub fn inputs(
    dir: &str,
    user: &UserSpaces,
    home: Option<&Path>,
    available: impl Fn(&str) -> bool,
) -> (Option<String>, Option<TabLayout>, Vec<String>) {
    let template = template_for(dir, &user.templates, home);
    let mut warnings = Vec::new();
    let agent_command = template.and_then(|t| {
        templates::agent_command(t, &user.harnesses, &available)
            .map_err(|w| warnings.push(w))
            .ok()
    });
    let layout = user.layout_for(template, &mut warnings);
    (agent_command, layout, warnings)
}

/// The layout a new space gets from the already read project `sources` with the resolution
/// warnings; `Err` (a warning naming the file) when the project layout is invalid. With
/// `allow_project_commands` false a layout from a project file keeps its panes but types nothing.
/// A task's `root_command` wins; then the template's `agent_command`, which fills a root pane
/// without a command of its own (the built-in default's `claude` is not one).
pub fn plan(
    sources: &Sources,
    user_layout: Option<&TabLayout>,
    allow_project_commands: bool,
    root_command: Option<&str>,
    agent_command: Option<&str>,
) -> Result<(TabLayout, Vec<String>), String> {
    let (project, legacy) = sources.refs();
    let (layout, warnings, from_project) = layout::resolve_origin(project, legacy, user_layout)?;
    let layout = if from_project && !allow_project_commands {
        layout.without_commands()
    } else {
        layout
    };
    let explicit = from_project || user_layout.is_some();
    let root_free = layout
        .root_terminal()
        .is_none_or(|(_, command)| command.is_none() || !explicit);
    let layout = match (root_command, agent_command) {
        (Some(command), _) => layout.with_root_command(command),
        (None, Some(agent)) if root_free => layout.with_root_command(agent),
        _ => layout,
    };
    Ok((layout, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/work/app";

    fn sources(project: Option<&str>) -> Sources {
        Sources {
            root: ROOT.into(),
            project: project.map(|t| (format!("{ROOT}/.poimen/config.json"), t.to_owned())),
            legacy: None,
        }
    }

    fn names(l: &TabLayout) -> Vec<(String, Option<String>)> {
        l.panes().into_iter().map(|p| (p.name, p.command)).collect()
    }

    fn pair(n: &str, c: Option<&str>) -> (String, Option<String>) {
        (n.to_owned(), c.map(str::to_owned))
    }

    fn tabs_of(json: &str) -> TabLayout {
        layout::parse_layout_value(&serde_json::from_str(json).unwrap())
            .unwrap()
            .to_tabs()
    }

    #[test]
    fn parse_reads_trust_templates_layout_and_harnesses() {
        let s = UserSpaces::parse(
            r#"{
              "theme": "dark",
              "trustedProjects": {"/work/app": "ab12", "/bad": 3},
              "spaces": {"layout": {"panes": [{"name": "solo"}]}},
              "spaceTemplates": [{"name": "work", "match": {"dir": "/work"}, "agent": "copilot"}],
              "harnesses": {"aider": {"command": ["aider"]}}
            }"#,
        );
        assert_eq!(s.trusted.get("/work/app").map(String::as_str), Some("ab12"));
        assert_eq!(s.trusted.len(), 1);
        assert_eq!(s.spaces_layout.unwrap().panes[0].name, "solo");
        assert_eq!(s.templates[0].name, "work");
        assert!(s.harnesses.iter().any(|h| h.id == "aider"));
        assert!(s.harnesses.iter().any(|h| h.id == "claude"));
        assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
    }

    #[test]
    fn parse_of_an_invalid_file_or_layout_warns_and_keeps_the_defaults() {
        let s = UserSpaces::parse("not json");
        assert_eq!(
            s,
            UserSpaces {
                warnings: s.warnings.clone(),
                ..UserSpaces::default()
            }
        );
        assert_eq!(s.warnings.len(), 1);
        let s = UserSpaces::parse(r#"{"spaces": {"layout": {"panes": []}}}"#);
        assert_eq!(s.spaces_layout, None);
        assert!(
            s.warnings[0].starts_with("spaces.layout"),
            "{:?}",
            s.warnings
        );
    }

    #[test]
    fn load_of_a_missing_file_is_the_default() {
        let home = std::env::temp_dir().join(format!("poimen-space-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        assert_eq!(UserSpaces::load(&home), UserSpaces::default());
        std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
        std::fs::write(config_path(&home), r#"{"trustedProjects": {"/r": "h"}}"#).unwrap();
        assert_eq!(UserSpaces::load(&home).trusted.len(), 1);
        std::fs::remove_dir_all(&home).unwrap();
    }

    fn template(agent: &str, layout: Option<Layout>) -> Template {
        Template {
            name: "work".into(),
            dir: Some("/work".into()),
            remote: None,
            agent: agent.into(),
            model: None,
            layout,
            layout_name: None,
        }
    }

    fn user(templates: Vec<Template>, spaces_layout: Option<Layout>) -> UserSpaces {
        UserSpaces {
            templates,
            spaces_layout,
            ..UserSpaces::default()
        }
    }

    #[test]
    fn inputs_take_the_template_agent_and_layout_over_spaces_layout() {
        let solo = layout::parse_layout(r#"{"panes":[{"name":"solo"}]}"#).unwrap();
        let mine = layout::parse_layout(r#"{"panes":[{"name":"mine"}]}"#).unwrap();
        let u = user(
            vec![template("claude", Some(mine.clone()))],
            Some(solo.clone()),
        );
        let (agent, layout, warnings) = inputs(ROOT, &u, None, |_| true);
        assert_eq!(agent.as_deref(), Some("claude"));
        assert_eq!(layout, Some(mine.to_tabs()));
        assert!(warnings.is_empty());
        // Outside the template's directory: `spaces.layout`, no agent.
        let (agent, layout, _) = inputs("/elsewhere", &u, None, |_| true);
        assert_eq!((agent, layout), (None, Some(solo.to_tabs())));
    }

    #[test]
    fn a_template_agent_that_is_missing_warns_and_starts_nothing() {
        let (agent, _, warnings) = inputs(
            ROOT,
            &user(vec![template("claude", None)], None),
            None,
            |_| false,
        );
        assert_eq!(agent, None);
        assert!(warnings[0].contains("not found"), "{warnings:?}");
        let (agent, _, warnings) = inputs(
            ROOT,
            &user(vec![template("nope", None)], None),
            None,
            |_| true,
        );
        assert_eq!(agent, None);
        assert!(warnings[0].contains("unknown harness"), "{warnings:?}");
    }

    const PROJECT: &str = r#"{"layout": {"panes": [{"name": "dev", "command": "make dev"},
        {"name": "sh", "split": {"direction": "down"}}]}}"#;

    #[test]
    fn plan_follows_the_precedence_and_strips_only_project_commands() {
        let user = tabs_of(r#"{"panes":[{"name":"u","command":"htop"}]}"#);
        let (l, _) = plan(&sources(Some(PROJECT)), Some(&user), true, None, None).unwrap();
        assert_eq!(names(&l), [pair("dev", Some("make dev")), pair("sh", None)]);
        let (l, _) = plan(&sources(Some(PROJECT)), Some(&user), false, None, None).unwrap();
        assert_eq!(names(&l), [pair("dev", None), pair("sh", None)]);
        // The user's own layout is never stripped.
        let (l, _) = plan(&sources(None), Some(&user), false, None, None).unwrap();
        assert_eq!(names(&l), [pair("u", Some("htop"))]);
        let (l, _) = plan(&sources(None), None, false, None, None).unwrap();
        assert_eq!(l, layout::default_tabs());
    }

    #[test]
    fn plan_puts_the_task_command_first_then_the_agent_into_a_free_root() {
        let (l, _) = plan(
            &sources(Some(PROJECT)),
            None,
            true,
            Some("fix 7"),
            Some("claude"),
        )
        .unwrap();
        assert_eq!(names(&l)[0], pair("dev", Some("fix 7")));
        // The project's root pane has a command: the agent does not replace it.
        let (l, _) = plan(&sources(Some(PROJECT)), None, true, None, Some("copilot")).unwrap();
        assert_eq!(names(&l)[0], pair("dev", Some("make dev")));
        // The built-in default's `claude` is not the user's choice: the agent replaces it.
        let (l, _) = plan(&sources(None), None, true, None, Some("copilot")).unwrap();
        assert_eq!(names(&l)[0], pair("agent", Some("copilot")));
    }

    #[test]
    fn plan_of_an_invalid_project_layout_is_a_warning_naming_the_file() {
        let err = plan(
            &sources(Some(r#"{"layout": {"panes": []}}"#)),
            None,
            true,
            None,
            None,
        )
        .unwrap_err();
        assert!(
            err.contains(".poimen/config.json") && err.contains("non-empty"),
            "{err}"
        );
    }

    #[test]
    fn layouts_load_skip_invalid_and_duplicates_and_templates_refer_by_name() {
        let u = UserSpaces::parse(
            r#"{
              "layouts": [
                {"name": "review", "tabs": [{"name": "r", "command": "copilot"}]},
                {"name": "review", "tabs": [{"name": "dup"}]},
                {"name": "bad", "tabs": []},
                {"tabs": [{"name": "nameless"}]},
                {"name": "old", "panes": [{"name": "p"}]}
              ],
              "spaceTemplates": [
                {"name": "t", "agent": "claude", "layout": "review"},
                {"name": "u", "match": {"dir": "/u"}, "agent": "claude", "layout": {"panes": [{"name": "inl"}]}}
              ]
            }"#,
        );
        let names: Vec<_> = u.layouts.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["review", "old"]);
        assert_eq!(u.warnings.len(), 3, "{:?}", u.warnings);
        assert_eq!(u.templates[0].layout_name.as_deref(), Some("review"));
        assert!(u.templates[0].layout.is_none());
        assert_eq!(u.templates[1].layout.as_ref().unwrap().panes[0].name, "inl");
        let mut w = Vec::new();
        let l = u.layout_for(Some(&u.templates[0]), &mut w).unwrap();
        assert_eq!(l.tabs[0].name.as_deref(), Some("r"));
        assert!(w.is_empty());
        let l = u.layout_for(Some(&u.templates[1]), &mut w).unwrap();
        assert_eq!(l.tabs[0].name.as_deref(), Some("inl"));
    }

    #[test]
    fn an_unknown_layout_name_warns_and_falls_back_to_spaces_layout_then_the_default() {
        let u = UserSpaces::parse(
            r#"{"spaces": {"layout": {"panes": [{"name": "legacy"}]}},
                "spaceTemplates": [{"name": "t", "agent": "claude", "layout": "nope"}]}"#,
        );
        let mut w = Vec::new();
        let l = u.layout_for(Some(&u.templates[0]), &mut w).unwrap();
        assert_eq!(l.tabs[0].name.as_deref(), Some("legacy"));
        assert!(w[0].contains("unknown layout \"nope\""), "{w:?}");
        let none = UserSpaces::parse(
            r#"{"spaceTemplates": [{"name": "t", "agent": "a", "layout": "nope"}]}"#,
        );
        assert_eq!(
            none.layout_for(Some(&none.templates[0]), &mut Vec::new()),
            None
        );
    }

    #[test]
    fn spaces_layout_is_the_layout_default_and_layouts_default_wins_with_a_warning() {
        let old = UserSpaces::parse(r#"{"spaces": {"layout": {"panes": [{"name": "legacy"}]}}}"#);
        assert!(old.warnings.is_empty());
        assert_eq!(
            old.layout_named("default").unwrap().tabs[0].name.as_deref(),
            Some("legacy")
        );
        assert_eq!(old.layout_named("other"), None);
        let both = UserSpaces::parse(
            r#"{"spaces": {"layout": {"panes": [{"name": "legacy"}]}},
                "layouts": [{"name": "default", "tabs": [{"name": "new"}]}]}"#,
        );
        assert_eq!(
            both.layout_named("default").unwrap().tabs[0]
                .name
                .as_deref(),
            Some("new")
        );
        assert!(
            both.warnings[0].contains("spaces.layout is ignored"),
            "{:?}",
            both.warnings
        );
    }

    #[test]
    fn inputs_take_the_template_layout_over_default_and_nothing_is_rewritten() {
        let home = std::env::temp_dir().join(format!("poimen-609-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
        let text = r#"{"spaces": {"layout": {"panes": [{"name": "legacy"}]}},
            "layouts": [{"name": "review", "tabs": [{"name": "r"}]}],
            "spaceTemplates": [{"name": "w", "match": {"dir": "/work"}, "agent": "claude", "layout": "review"}]}"#;
        std::fs::write(config_path(&home), text).unwrap();
        let u = UserSpaces::load(&home);
        let (_, l, w) = inputs("/work/app", &u, None, |_| true);
        assert_eq!(l.unwrap().tabs[0].name.as_deref(), Some("r"));
        assert!(w.is_empty(), "{w:?}");
        let (_, l, _) = inputs("/elsewhere", &u, None, |_| true);
        assert_eq!(l.unwrap().tabs[0].name.as_deref(), Some("legacy"));
        assert_eq!(std::fs::read_to_string(config_path(&home)).unwrap(), text);
        std::fs::remove_dir_all(&home).unwrap();
    }
}
