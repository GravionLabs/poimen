//! Task modes (`tasks` in `config.json`): how an issue is turned into work. This module holds the
//! types and the template expansion; `settings.rs` resolves and validates the config.

// The expansion is consumed by the task start (a later PBI); until then only tests call it.
#![allow(dead_code)]

use serde::Serialize;

pub const DEFAULT_BRANCH: &str = "{type}/{nr}-{slug}";
pub const DEFAULT_WORKTREE_DIR: &str = "../{repo}-{nr}";
pub const DEFAULT_PROMPT: &str = "Work on GitHub issue #{nr}: {title} ({url})";

/// Placeholders allowed in `branch` and `worktreeDir`.
pub const PATH_PLACEHOLDERS: &[&str] = &["nr", "title", "url", "repo", "slug", "type"];
/// Placeholders allowed in the `prompt` of a harness mode: those of `command` except `prompt`.
pub const PROMPT_PLACEHOLDERS: &[&str] = &["nr", "title", "url", "branch", "repo", "slug", "type"];
/// Placeholders allowed in `command`.
pub const COMMAND_PLACEHOLDERS: &[&str] = &[
    "nr", "title", "url", "branch", "repo", "slug", "type", "prompt",
];
/// Placeholders allowed in `cleanup`: those of `command` plus `pr`, the merged pull request.
/// A cleanup only knows the branch, the issue number and the pull request; `title`, `url` and
/// `prompt` are empty there (`slug` is empty, `type` is `issue`).
pub const CLEANUP_PLACEHOLDERS: &[&str] = &[
    "nr", "title", "url", "branch", "repo", "slug", "type", "prompt", "pr",
];

/// A command template: an argv list (each element expanded on its own, no shell) or, deprecated
/// for one release, a shell string (values are single-quoted when it is expanded).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum CommandSpec {
    Argv(Vec<String>),
    Shell(String),
}

impl CommandSpec {
    /// The template as one line for display (the trust question).
    pub fn display(&self) -> String {
        match self {
            CommandSpec::Shell(line) => line.clone(),
            CommandSpec::Argv(argv) => Cmd::Argv(argv.clone()).line(),
        }
    }
}

impl TaskMode {
    /// What the mode runs, for display: its command, or the agent it starts.
    pub fn describe(&self) -> String {
        if let Some(c) = &self.command {
            return c.display();
        }
        let mut out = format!(
            "agent {}",
            self.harness.as_deref().unwrap_or("(default harness)")
        );
        if let Some(m) = &self.model {
            out.push_str(&format!(" --model {m}"));
        }
        out.push_str(&format!(
            " {}",
            shell_quote(self.prompt.as_deref().unwrap_or(DEFAULT_PROMPT))
        ));
        out
    }
}

/// A command spec expanded for one issue.
#[derive(Debug, Clone, PartialEq)]
pub enum Cmd {
    /// The program and its arguments, run without a shell.
    Argv(Vec<String>),
    /// A line for a shell.
    Shell(String),
}

impl Cmd {
    /// The command as one line for a shell (typed into a terminal): argv elements are quoted where
    /// they contain more than plain word characters.
    pub fn line(&self) -> String {
        match self {
            Cmd::Shell(line) => line.clone(),
            Cmd::Argv(argv) => argv
                .iter()
                .map(|a| quote_if_needed(a))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }

    /// The argv; empty for a shell line.
    pub fn argv(&self) -> Vec<String> {
        match self {
            Cmd::Argv(argv) => argv.clone(),
            Cmd::Shell(_) => Vec::new(),
        }
    }
}

fn quote_if_needed(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./:=@%+,".contains(c));
    if plain {
        value.to_owned()
    } else {
        shell_quote(value)
    }
}

/// A task mode. Without `command` it starts an agent: `harness` (default: the first of the
/// registry) with `model` and `prompt` (default `DEFAULT_PROMPT`). With `command` it runs that
/// instead and `harness`, `model` and `prompt` are not set.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskMode {
    pub name: String,
    pub worktree: bool,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub prompt: Option<String>,
    pub command: Option<CommandSpec>,
    pub branch: String,
    pub cleanup: Option<CommandSpec>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskModes {
    pub modes: Vec<TaskMode>,
    pub worktree_dir: String,
}

impl Default for TaskModes {
    fn default() -> Self {
        Self {
            modes: vec![TaskMode {
                name: "agent".into(),
                worktree: true,
                harness: None,
                model: None,
                prompt: None,
                command: None,
                branch: DEFAULT_BRANCH.into(),
                cleanup: None,
            }],
            worktree_dir: DEFAULT_WORKTREE_DIR.into(),
        }
    }
}

/// `base` with the project's override: a mode of the same `name` is replaced in place, new ones
/// are appended, `worktreeDir` is replaced.
pub fn merge(base: &TaskModes, over: &crate::settings::TaskOverride) -> TaskModes {
    let mut merged = base.clone();
    for mode in over.modes.iter().flatten() {
        match merged.modes.iter_mut().find(|m| m.name == mode.name) {
            Some(existing) => *existing = mode.clone(),
            None => merged.modes.push(mode.clone()),
        }
    }
    if let Some(dir) = &over.worktree_dir {
        merged.worktree_dir = dir.clone();
    }
    merged
}

/// The task modes for a repository: the user's, with the project file's `tasks` merged over them
/// when the project is trusted. An untrusted project contributes nothing and `note` says why.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolved {
    #[serde(flatten)]
    pub modes: TaskModes,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

pub fn for_project(
    user: &TaskModes,
    sources: &crate::trust::Sources,
    trusted: &std::collections::BTreeMap<String, String>,
) -> Resolved {
    let over = sources.project.as_ref().and_then(|(path, text)| {
        crate::project::parse(path, text, crate::project::platform())
            .config
            .tasks
    });
    let Some(over) = over else {
        return Resolved {
            modes: user.clone(),
            note: None,
        };
    };
    match crate::trust::gate(sources, trusted, None) {
        crate::trust::Gate::Run { .. } => Resolved {
            modes: merge(user, &over),
            note: None,
        },
        _ => Resolved {
            modes: user.clone(),
            note: Some(
                "Task modes of the project file are not used until you trust the project                  (creating a space for it asks)."
                    .into(),
            ),
        },
    }
}

/// The names of the `{placeholder}`s in `template`, in order.
pub fn placeholders(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) => {
                out.push(after[..end].to_string());
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

/// The first placeholder of `template` that is not in `allowed`.
pub fn unknown_placeholder(template: &str, allowed: &[&str]) -> Option<String> {
    placeholders(template)
        .into_iter()
        .find(|p| !allowed.contains(&p.as_str()))
}

/// Single-quotes `value` for a POSIX shell (embedded `'` becomes `'\''`).
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn strip_level(title: &str) -> &str {
    let t = title.trim_start();
    if t.starts_with('[')
        && let Some(i) = t.find(']')
    {
        return t[i + 1..].trim_start();
    }
    t
}

/// The conventional commit type of an issue title (`feat(tasks): x`, `[Level] fix!: x`), else `issue`.
pub fn title_type(title: &str) -> &'static str {
    const TYPES: &[&str] = &[
        "feat", "fix", "refactor", "docs", "build", "chore", "test", "perf", "ci",
    ];
    let rest = strip_level(title);
    let head: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    let after = &rest[head.len()..];
    let after = match after.strip_prefix('(') {
        Some(a) => match a.find(')') {
            Some(i) => &a[i + 1..],
            None => return "issue",
        },
        None => after,
    };
    let after = after.strip_prefix('!').unwrap_or(after);
    if after.starts_with(':')
        && let Some(t) = TYPES.iter().find(|t| **t == head.to_lowercase())
    {
        return t;
    }
    "issue"
}

/// Title without the `[Level] ` prefix and the conventional type/scope, lowercase, runs of
/// non-alphanumerics as one `-`, at most 40 characters, no leading or trailing `-`.
pub fn slug(title: &str) -> String {
    let mut rest = strip_level(title);
    if title_type(title) != "issue"
        && let Some(i) = rest.find(':')
    {
        rest = &rest[i + 1..];
    }
    let mut out = String::new();
    for c in rest.chars().flat_map(|c| c.to_lowercase()) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.truncate(40);
    out.trim_end_matches('-').to_string()
}

/// The values behind the placeholders of one issue.
#[derive(Debug, Clone, Default)]
pub struct TaskVars {
    pub nr: String,
    pub title: String,
    pub url: String,
    pub repo: String,
    pub branch: String,
    pub prompt: String,
    /// The merged pull request of a cleanup; empty otherwise.
    pub pr: String,
}

impl TaskVars {
    /// Variables for an issue; `branch` and `prompt` are filled by `expand_mode`.
    pub fn new(nr: u64, title: &str, url: &str, repo: &str) -> Self {
        Self {
            nr: nr.to_string(),
            title: title.into(),
            url: url.into(),
            repo: repo.into(),
            ..Self::default()
        }
    }

    fn value(&self, name: &str) -> Option<String> {
        Some(match name {
            "nr" => self.nr.clone(),
            "title" => self.title.clone(),
            "url" => self.url.clone(),
            "repo" => self.repo.clone(),
            "branch" => self.branch.clone(),
            "prompt" => self.prompt.clone(),
            "pr" => self.pr.clone(),
            "slug" => slug(&self.title),
            "type" => title_type(&self.title).into(),
            _ => return None,
        })
    }
}

/// Expands `template`; values are shell-quoted when `quote` is set. Unknown placeholders stay.
pub fn expand(template: &str, vars: &TaskVars, quote: bool) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return out;
        };
        match vars.value(&after[..end]) {
            Some(v) if quote => out.push_str(&shell_quote(&v)),
            Some(v) => out.push_str(&v),
            None => out.push_str(&rest[start..start + end + 2]),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Expands a command spec: argv elements one by one, never quoted; a shell string with quoted values.
pub fn expand_command(spec: &CommandSpec, vars: &TaskVars) -> Cmd {
    match spec {
        CommandSpec::Argv(argv) => Cmd::Argv(argv.iter().map(|a| expand(a, vars, false)).collect()),
        CommandSpec::Shell(line) => Cmd::Shell(expand(line, vars, true)),
    }
}

/// A mode expanded for one issue.
#[derive(Debug, Clone, PartialEq)]
pub struct Expanded {
    pub branch: String,
    pub worktree_dir: String,
    pub command: Cmd,
    pub cleanup: Option<Cmd>,
}

/// Expands branch, worktree directory, command and cleanup of `mode` for the issue in `vars`.
/// An agent mode (no `command`) becomes the argv of its harness from `harnesses`; an unknown
/// harness is an error.
pub fn expand_mode(
    mode: &TaskMode,
    worktree_dir: &str,
    mut vars: TaskVars,
    harnesses: &[crate::harness::Harness],
) -> Result<Expanded, String> {
    vars.branch = expand(&mode.branch, &vars, false);
    vars.prompt = expand(DEFAULT_PROMPT, &vars, false);
    let command = match &mode.command {
        Some(spec) => expand_command(spec, &vars),
        None => {
            let harness = match &mode.harness {
                Some(id) => harnesses.iter().find(|h| h.id == *id),
                None => harnesses.first(),
            }
            .ok_or_else(|| match &mode.harness {
                Some(id) => format!("task mode {:?}: unknown harness {id:?}", mode.name),
                None => format!("task mode {:?}: no agent harness configured", mode.name),
            })?;
            let prompt = expand(
                mode.prompt.as_deref().unwrap_or(DEFAULT_PROMPT),
                &vars,
                false,
            );
            Cmd::Argv(crate::harness::build_argv(
                harness,
                mode.model.as_deref(),
                Some(&prompt),
            ))
        }
    };
    Ok(Expanded {
        branch: vars.branch.clone(),
        worktree_dir: expand(worktree_dir, &vars, false),
        command,
        cleanup: mode.cleanup.as_ref().map(|c| expand_command(c, &vars)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::default_harnesses;
    use crate::trust::Sources;
    use std::collections::BTreeMap;

    const PROJECT: &str = r#"{"tasks":{
        "worktreeDir":"../wt/{repo}-{nr}",
        "modes":[
            {"name":"agent","harness":"claude","model":"opus"},
            {"name":"deliver","command":["/deliver","{nr}"],"worktree":false}
        ]}}"#;

    fn sources(text: &str) -> Sources {
        Sources {
            root: "/work/app".into(),
            project: Some(("/work/app/.poimen/config.json".into(), text.into())),
            legacy: None,
        }
    }

    fn trusted_for(sources: &Sources) -> BTreeMap<String, String> {
        BTreeMap::from([(sources.root.clone(), sources.hash().unwrap())])
    }

    #[test]
    fn project_modes_merge_by_name_over_the_users_and_new_ones_are_appended() {
        let user = TaskModes::default();
        let s = sources(PROJECT);
        let got = for_project(&user, &s, &trusted_for(&s));
        let names: Vec<_> = got.modes.modes.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["agent", "deliver"]);
        // The same name replaces the user's mode (project wins), in place.
        assert_eq!(got.modes.modes[0].model.as_deref(), Some("opus"));
        assert!(!got.modes.modes[1].worktree);
        assert_eq!(got.note, None);
    }

    #[test]
    fn the_project_overrides_the_worktree_dir_and_leaves_it_alone_otherwise() {
        let user = TaskModes::default();
        let s = sources(PROJECT);
        let got = for_project(&user, &s, &trusted_for(&s));
        assert_eq!(got.modes.worktree_dir, "../wt/{repo}-{nr}");
        let only_modes = sources(r#"{"tasks":{"modes":[{"name":"x","command":["echo","{nr}"]}]}}"#);
        let got = for_project(&user, &only_modes, &trusted_for(&only_modes));
        assert_eq!(got.modes.worktree_dir, user.worktree_dir);
        assert_eq!(got.modes.modes.len(), 2);
    }

    #[test]
    fn an_untrusted_project_leaves_only_the_users_modes_with_a_note() {
        let user = TaskModes::default();
        let got = for_project(&user, &sources(PROJECT), &BTreeMap::new());
        assert_eq!(got.modes, user);
        assert!(got.note.as_deref().unwrap().contains("trust"));
        // A changed file asks again: the stored hash no longer matches.
        let s = sources(PROJECT);
        let mut stale = trusted_for(&s);
        stale.insert(s.root.clone(), "0".repeat(64));
        assert_eq!(for_project(&user, &s, &stale).modes, user);
    }

    #[test]
    fn no_project_tasks_is_just_the_users_modes_without_a_note() {
        let user = TaskModes::default();
        let got = for_project(&user, &sources(r#"{"setup":"make"}"#), &BTreeMap::new());
        assert_eq!((got.modes, got.note), (user.clone(), None));
        let got = for_project(&user, &Sources::default(), &BTreeMap::new());
        assert_eq!((got.modes, got.note), (user, None));
    }

    #[test]
    fn the_modes_of_a_project_are_listed_for_the_trust_question() {
        let listed = sources(PROJECT).commands();
        let whats: Vec<_> = listed.iter().map(|c| c.what.as_str()).collect();
        assert_eq!(
            whats,
            [
                "task worktreeDir",
                "task mode \"agent\"",
                "task mode \"deliver\""
            ]
        );
    }

    #[test]
    fn invalid_project_modes_warn_with_the_file_and_are_skipped() {
        let loaded = crate::project::parse(
            "/p/.poimen/config.json",
            r#"{"tasks":{"modes":[{"name":"bad","command":["x","{nope}"]},{"name":"ok","command":["y"]}]}}"#,
            "linux",
        );
        assert_eq!(loaded.config.tasks.unwrap().modes.unwrap().len(), 1);
        assert!(
            loaded
                .warnings
                .iter()
                .any(|w| w.starts_with("/p/.poimen/config.json: tasks.modes[bad].command"))
        );
    }

    #[test]
    fn type_comes_from_the_conventional_title() {
        assert_eq!(title_type("[PBI] feat(tasks): task modes"), "feat");
        assert_eq!(title_type("fix!: boom"), "fix");
        assert_eq!(title_type("ci: pin"), "ci");
        assert_eq!(title_type("Add a thing"), "issue");
        assert_eq!(title_type("style: nope"), "issue");
        assert_eq!(title_type("feat(open: x"), "issue");
    }

    #[test]
    fn slug_drops_prefix_type_and_scope() {
        assert_eq!(
            slug("[PBI] feat(tasks): Task modes in the settings"),
            "task-modes-in-the-settings"
        );
        assert_eq!(slug("Fix the  thing!!"), "fix-the-thing");
        assert_eq!(
            slug("[Epic] Plain title: with colon"),
            "plain-title-with-colon"
        );
        assert_eq!(slug("???"), "");
    }

    #[test]
    fn slug_is_at_most_40_chars_without_trailing_dash() {
        let s = slug("feat: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbb");
        assert_eq!(s, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        let t = slug("feat: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbb");
        assert_eq!(t.len(), 40);
        assert_eq!(slug("feat: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa b"), s);
    }

    #[test]
    fn shell_quote_escapes_single_quotes() {
        assert_eq!(shell_quote("a b"), "'a b'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    const TITLE: &str = "fix: it's $(rm -rf ~); echo `x` a b";
    const PROMPT: &str =
        "Work on GitHub issue #7: fix: it's $(rm -rf ~); echo `x` a b (https://x/7)";

    fn agent_mode() -> TaskMode {
        TaskModes::default().modes.remove(0)
    }

    fn expand_for(mode: &TaskMode) -> Expanded {
        let vars = TaskVars::new(7, TITLE, "https://x/7", "poimen");
        expand_mode(mode, "../{repo}-{nr}", vars, &default_harnesses()).unwrap()
    }

    #[test]
    fn the_default_mode_starts_the_first_harness_with_the_default_prompt_as_one_element() {
        let e = expand_for(&agent_mode());
        assert_eq!(e.command, Cmd::Argv(vec!["claude".into(), PROMPT.into()]));
        assert_eq!(e.branch, "fix/7-it-s-rm-rf-echo-x-a-b");
        assert_eq!(e.worktree_dir, "../poimen-7");
        assert_eq!(e.cleanup, None);
    }

    #[test]
    fn a_harness_mode_uses_its_harness_model_and_prompt_template() {
        let mut mode = agent_mode();
        mode.harness = Some("copilot".into());
        mode.model = Some("gpt-5.4".into());
        mode.prompt = Some("Fix {title} in {repo} #{nr}".into());
        let e = expand_for(&mode);
        assert_eq!(
            e.command,
            Cmd::Argv(
                [
                    "copilot",
                    "--model",
                    "gpt-5.4",
                    "-i",
                    &format!("Fix {TITLE} in poimen #7")
                ]
                .map(String::from)
                .to_vec()
            )
        );
    }

    #[test]
    fn an_unknown_harness_is_an_error_naming_the_mode() {
        let mut mode = agent_mode();
        mode.harness = Some("nope".into());
        let vars = TaskVars::new(7, "t", "u", "r");
        let err = expand_mode(&mode, "d", vars, &default_harnesses()).unwrap_err();
        assert!(err.contains("agent") && err.contains("nope"), "{err}");
    }

    #[test]
    fn an_argv_command_expands_per_element_and_a_title_stays_one_element() {
        let mut mode = agent_mode();
        mode.command = Some(CommandSpec::Argv(
            [
                "pwsh",
                "Invoke-DeliverLoop.ps1",
                "-Target",
                "{nr}",
                "-Title",
                "{title}",
                "x-{slug}",
            ]
            .map(String::from)
            .to_vec(),
        ));
        mode.cleanup = Some(CommandSpec::Argv(
            ["git", "branch", "-D", "{branch}"]
                .map(String::from)
                .to_vec(),
        ));
        let e = expand_for(&mode);
        assert_eq!(
            e.command,
            Cmd::Argv(
                [
                    "pwsh",
                    "Invoke-DeliverLoop.ps1",
                    "-Target",
                    "7",
                    "-Title",
                    TITLE,
                    "x-it-s-rm-rf-echo-x-a-b"
                ]
                .map(String::from)
                .to_vec()
            )
        );
        assert_eq!(
            e.cleanup,
            Some(Cmd::Argv(
                ["git", "branch", "-D", "fix/7-it-s-rm-rf-echo-x-a-b"]
                    .map(String::from)
                    .to_vec()
            ))
        );
    }

    #[test]
    fn a_string_command_is_still_expanded_with_shell_quoting() {
        let mut mode = agent_mode();
        mode.command = Some(CommandSpec::Shell("claude {prompt}".into()));
        mode.cleanup = Some(CommandSpec::Shell("git branch -D {branch}".into()));
        let e = expand_for(&mode);
        assert_eq!(
            e.command,
            Cmd::Shell(
                "claude 'Work on GitHub issue #7: fix: it'\\''s $(rm -rf ~); echo `x` a b (https://x/7)'"
                    .into()
            )
        );
        assert_eq!(
            e.cleanup,
            Some(Cmd::Shell(
                "git branch -D 'fix/7-it-s-rm-rf-echo-x-a-b'".into()
            ))
        );
        assert!(e.command.argv().is_empty());
    }

    #[test]
    fn each_placeholder_is_quoted_separately_in_a_string_command() {
        let mut mode = agent_mode();
        mode.command = Some(CommandSpec::Shell(
            "run {nr} {title} {url} {repo} {slug} {type}".into(),
        ));
        let vars = TaskVars::new(3, "Hello; world", "u", "r");
        let e = expand_mode(&mode, "{slug}", vars, &default_harnesses()).unwrap();
        assert_eq!(
            e.command,
            Cmd::Shell("run '3' 'Hello; world' 'u' 'r' 'hello-world' 'issue'".into())
        );
        assert_eq!(e.worktree_dir, "hello-world");
    }

    #[test]
    fn an_argv_is_typed_as_a_line_with_quotes_only_where_needed() {
        let line = Cmd::Argv(
            ["claude", "--model", "a b", "it's", ""]
                .map(String::from)
                .to_vec(),
        )
        .line();
        assert_eq!(line, "claude --model 'a b' 'it'\\''s' ''");
    }

    #[test]
    fn a_mode_is_described_by_its_command_or_its_agent_for_the_trust_question() {
        let mut mode = agent_mode();
        assert_eq!(
            mode.describe(),
            "agent (default harness) 'Work on GitHub issue #{nr}: {title} ({url})'"
        );
        mode.harness = Some("claude".into());
        mode.model = Some("opus".into());
        assert!(mode.describe().starts_with("agent claude --model opus "));
        mode.command = Some(CommandSpec::Argv(vec!["pwsh".into(), "a b".into()]));
        assert_eq!(mode.describe(), "pwsh 'a b'");
    }

    #[test]
    fn unknown_placeholders_are_found_and_kept() {
        assert_eq!(
            unknown_placeholder("a {nr} {x}", COMMAND_PLACEHOLDERS),
            Some("x".into())
        );
        assert_eq!(
            unknown_placeholder("{prompt}", PATH_PLACEHOLDERS),
            Some("prompt".into())
        );
        assert_eq!(unknown_placeholder("no braces {", PATH_PLACEHOLDERS), None);
        assert_eq!(
            expand("{x}-{nr}", &TaskVars::new(1, "t", "u", "r"), false),
            "{x}-1"
        );
    }
}
