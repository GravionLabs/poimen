//! Agent actions for issues, pull requests and milestones (`github.agentActions[]` in
//! `config.json`, ADR 0005): a prompt template per action, so poimen starts agents without knowing
//! any skill. Entries merge by `id` (defaults, user, project); the prompt expands into one argv
//! element through the harness registry (`harness.rs`), never through a shell.

// The expansion is consumed by the GitHub views (a later PBI); until then only tests call it.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::harness::{self, Harness};

/// Placeholders allowed in a `prompt`. `log` (the file with the failed check logs, #238) is valid
/// only for `on: "pr"`.
pub const PLACEHOLDERS: &[&str] = &["nr", "nrs", "title", "url", "repo", "branch", "base", "log"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum On {
    Issue,
    Pr,
    Milestone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Run {
    RepoRoot,
    Worktree,
    Ask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Batch {
    Each,
    Once,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Default,
    User,
    Project,
}

/// When an action is offered; every given condition must hold.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct When {
    /// Any of these planning levels (`epic`, `feature`, `pbi`, `bug`).
    pub level: Vec<String>,
    /// Any of these labels.
    pub label: Vec<String>,
    /// The issue has no sub-issues.
    pub no_children: bool,
    /// `failed`: the checks of the pull request failed.
    pub checks: Option<String>,
    /// The pull request conflicts with its base.
    pub conflicting: bool,
    /// Any of these states (`open`, `closed`, `merged`).
    pub state: Vec<String>,
    /// The review decision of the pull request (`CHANGES_REQUESTED`, ...).
    pub review: Option<String>,
}

/// One entry of `github.agentActions[]` as configured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    pub id: String,
    pub label: String,
    pub on: On,
    pub when: When,
    pub prompt: String,
    pub run: Run,
    pub harness: Option<String>,
    pub model: Option<String>,
    pub batch: Batch,
    /// Removes the entry of the same id from the layer below.
    #[serde(skip)]
    pub disabled: bool,
}

/// A resolved action with where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Action {
    #[serde(flatten)]
    pub spec: Spec,
    pub source: Source,
}

/// The answer of `get_agent_actions`.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Resolved {
    pub actions: Vec<Action>,
    pub note: Option<String>,
}

fn spec(id: &str, label: &str, on: On, when: When, prompt: &str) -> Spec {
    Spec {
        id: id.into(),
        label: label.into(),
        on,
        when,
        prompt: prompt.into(),
        run: Run::RepoRoot,
        harness: None,
        model: None,
        batch: Batch::Each,
        disabled: false,
    }
}

fn strs(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// The built-in actions: neutral prompts that name no skill.
pub fn defaults() -> Vec<Spec> {
    vec![
        spec(
            "plan",
            "Plan",
            On::Issue,
            When {
                level: strs(&["epic", "feature"]),
                no_children: true,
                ..When::default()
            },
            "Plan GitHub issue #{nr} in {repo}: {title} ({url}). Break it into the next level of \
             issues with sizes and blockers, and ask me about open decisions before you create anything.",
        ),
        spec(
            "refine",
            "Refine",
            On::Issue,
            When {
                label: strs(&["needs-refinement"]),
                ..When::default()
            },
            "Refine GitHub issue #{nr} in {repo}: {title} ({url}). Check that its acceptance \
             criteria are complete and testable, and list what is still open.",
        ),
        Spec {
            run: Run::Ask,
            ..spec(
                "review",
                "Review",
                On::Pr,
                When::default(),
                "Review pull request #{nr} in {repo}: {title} ({url}), branch {branch} against its \
                 base {base}. Report your findings, blocking ones separately from notes.",
            )
        },
        Spec {
            run: Run::Worktree,
            ..spec(
                "fix-checks",
                "Fix checks",
                On::Pr,
                When {
                    checks: Some("failed".into()),
                    ..When::default()
                },
                "Fix the failing checks of pull request #{nr} in {repo} ({url}, branch {branch}). \
                 The logs of the failed runs are in {log}.",
            )
        },
        Spec {
            run: Run::Worktree,
            ..spec(
                "resolve-conflicts",
                "Resolve conflicts",
                On::Pr,
                When {
                    conflicting: true,
                    ..When::default()
                },
                "Pull request #{nr} in {repo} ({url}, branch {branch}) conflicts with its base \
                 branch {base}. Merge {base} into {branch}, resolve the conflicts so that the \
                 intent of both sides stays, run the tests and commit the merge. Do not rebase \
                 and do not force-push.",
            )
        },
        spec(
            "address-review",
            "Address review comments",
            On::Pr,
            When {
                review: Some("CHANGES_REQUESTED".into()),
                ..When::default()
            },
            "Address the review comments on pull request #{nr} in {repo} ({url}, branch {branch}): \
             change the code where they are right and answer where they are not.",
        ),
    ]
}

fn text(obj: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match obj.get(key) {
        None => Ok(None),
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(Some(s.clone())),
        Some(_) => Err(format!("{key} must be a non-empty string")),
    }
}

fn list(obj: &Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    match obj.get(key) {
        None => Ok(Vec::new()),
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| format!("when.{key} must be an array of strings")),
        Some(_) => Err(format!("when.{key} must be an array of strings")),
    }
}

fn parse_when(value: &Value) -> Result<When, String> {
    let obj = value.as_object().ok_or("when must be an object")?;
    let checks = match obj.get("checks") {
        None => None,
        Some(Value::String(s)) if s == "failed" => Some(s.clone()),
        Some(_) => return Err("when.checks must be \"failed\"".into()),
    };
    let no_children = match obj.get("noChildren") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("when.noChildren must be a boolean".into()),
    };
    let conflicting = match obj.get("conflicting") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err("when.conflicting must be a boolean".into()),
    };
    let review = match obj.get("review") {
        None => None,
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        Some(_) => return Err("when.review must be a non-empty string".into()),
    };
    Ok(When {
        level: list(obj, "level")?,
        label: list(obj, "label")?,
        no_children,
        checks,
        conflicting,
        state: list(obj, "state")?,
        review,
    })
}

fn choice<T: Copy>(
    obj: &Map<String, Value>,
    key: &str,
    options: &[(&str, T)],
    default: Option<T>,
) -> Result<T, String> {
    let names = || {
        options
            .iter()
            .map(|(n, _)| format!("\"{n}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    match obj.get(key) {
        None => default.ok_or_else(|| format!("{key} is missing (one of {})", names())),
        Some(Value::String(s)) => options
            .iter()
            .find(|(n, _)| n == s)
            .map(|(_, v)| *v)
            .ok_or_else(|| format!("{key} must be one of {}", names())),
        Some(_) => Err(format!("{key} must be one of {}", names())),
    }
}

fn parse_one(obj: &Map<String, Value>, id: &str) -> Result<Spec, String> {
    let on = choice(
        obj,
        "on",
        &[
            ("issue", On::Issue),
            ("pr", On::Pr),
            ("milestone", On::Milestone),
        ],
        None,
    )?;
    let prompt = text(obj, "prompt")?.ok_or("prompt is missing")?;
    if let Some(p) = crate::tasks::placeholders(&prompt)
        .into_iter()
        .find(|p| !PLACEHOLDERS.contains(&p.as_str()))
    {
        return Err(format!("unknown placeholder {{{p}}}"));
    }
    if on != On::Pr
        && crate::tasks::placeholders(&prompt)
            .iter()
            .any(|p| p == "log")
    {
        return Err("{log} is only valid for actions on pull requests".into());
    }
    let when = match obj.get("when") {
        None => When::default(),
        Some(v) => parse_when(v)?,
    };
    Ok(Spec {
        id: id.to_owned(),
        label: text(obj, "label")?.unwrap_or_else(|| id.to_owned()),
        on,
        when,
        prompt,
        run: choice(
            obj,
            "run",
            &[
                ("repoRoot", Run::RepoRoot),
                ("worktree", Run::Worktree),
                ("ask", Run::Ask),
            ],
            Some(Run::RepoRoot),
        )?,
        harness: text(obj, "harness")?,
        model: text(obj, "model")?,
        batch: choice(
            obj,
            "batch",
            &[("each", Batch::Each), ("once", Batch::Once)],
            Some(Batch::Each),
        )?,
        disabled: false,
    })
}

/// Parses a `github.agentActions` array. Invalid entries warn and are skipped (`at` prefixes the
/// warnings); a repeated id keeps the first entry. `disabled: true` entries only need an `id`.
pub fn parse(value: &Value, at: &str, warnings: &mut Vec<String>) -> Vec<Spec> {
    let Some(items) = value.as_array() else {
        warnings.push(format!("{at} must be an array"));
        return Vec::new();
    };
    let mut out: Vec<Spec> = Vec::new();
    for (i, raw) in items.iter().enumerate() {
        let Some(obj) = raw.as_object() else {
            warnings.push(format!("{at}[{i}] must be an object; ignored"));
            continue;
        };
        let id = match obj.get("id") {
            Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_owned(),
            _ => {
                warnings.push(format!("{at}[{i}].id must be a non-empty string; ignored"));
                continue;
            }
        };
        if out.iter().any(|s| s.id == id) {
            warnings.push(format!("{at}[{id}]: duplicate id; the first one wins"));
            continue;
        }
        if obj.get("disabled") == Some(&Value::Bool(true)) {
            out.push(Spec {
                disabled: true,
                ..spec(&id, &id, On::Issue, When::default(), "")
            });
            continue;
        }
        match parse_one(obj, &id) {
            Ok(s) => out.push(s),
            Err(e) => warnings.push(format!("{at}[{id}]: {e}; ignored")),
        }
    }
    out
}

/// Drops entries that name a harness missing from the registry, with a warning.
pub fn check_harnesses(
    specs: Vec<Spec>,
    harnesses: &[Harness],
    at: &str,
    warnings: &mut Vec<String>,
) -> Vec<Spec> {
    specs
        .into_iter()
        .filter(|s| match &s.harness {
            Some(h) if !harnesses.iter().any(|k| &k.id == h) => {
                warnings.push(format!("{at}[{}]: unknown harness {h:?}; ignored", s.id));
                false
            }
            _ => true,
        })
        .collect()
}

/// Layers `over` on `base`: the same id replaces in place, `disabled` removes, a new id is appended.
pub fn merge(base: Vec<Action>, over: &[Spec], source: Source) -> Vec<Action> {
    let mut out = base;
    for s in over {
        let pos = out.iter().position(|a| a.spec.id == s.id);
        let action = Action {
            spec: s.clone(),
            source,
        };
        match (pos, s.disabled) {
            (Some(p), true) => {
                out.remove(p);
            }
            (None, true) => {}
            (Some(p), false) => out[p] = action,
            (None, false) => out.push(action),
        }
    }
    out
}

/// Defaults, then the user's entries, then the project's.
pub fn resolve(user: &[Spec], project: &[Spec]) -> Vec<Action> {
    let base = merge(Vec::new(), &defaults(), Source::Default);
    let base = merge(base, user, Source::User);
    merge(base, project, Source::Project)
}

/// The actions for one repository: the project file's `github.agentActions` apply only when the
/// project is trusted (its prompts are listed in the trust dialog); otherwise a note says so.
pub fn for_project(
    user: &[Spec],
    harnesses: &[Harness],
    sources: &crate::trust::Sources,
    trusted: &std::collections::BTreeMap<String, String>,
) -> Resolved {
    let project = sources
        .project
        .as_ref()
        .map(|(path, text)| crate::project::parse(path, text, crate::project::platform()))
        .map(|l| l.config.agent_actions)
        .unwrap_or_default();
    if project.is_empty() {
        return Resolved {
            actions: resolve(user, &[]),
            note: None,
        };
    }
    match crate::trust::gate(sources, trusted, None) {
        crate::trust::Gate::Run { .. } => {
            let mut ignored = Vec::new();
            let project = check_harnesses(project, harnesses, "github.agentActions", &mut ignored);
            Resolved {
                actions: resolve(user, &project),
                note: None,
            }
        }
        _ => Resolved {
            actions: resolve(user, &[]),
            note: Some(
                "Agent actions of the project file are not used until you trust the project \
                 (creating a space for it asks)."
                    .into(),
            ),
        },
    }
}

/// What an action is offered for.
#[derive(Debug, Clone, Default)]
pub struct Target {
    pub on: Option<On>,
    pub level: Option<String>,
    pub labels: Vec<String>,
    pub has_children: bool,
    pub checks: Option<String>,
    pub conflicting: bool,
    pub state: Option<String>,
    pub review: Option<String>,
}

fn any_of(wanted: &[String], have: &[String]) -> bool {
    wanted.is_empty()
        || wanted
            .iter()
            .any(|w| have.iter().any(|h| h.eq_ignore_ascii_case(w)))
}

/// Whether `action` applies to `target`: the kind and every condition of `when`.
pub fn matches(action: &Spec, t: &Target) -> bool {
    let w = &action.when;
    let level: Vec<String> = t.level.iter().cloned().collect();
    let state: Vec<String> = t.state.iter().cloned().collect();
    t.on == Some(action.on)
        && any_of(&w.level, &level)
        && any_of(&w.label, &t.labels)
        && !(w.no_children && t.has_children)
        && w.checks
            .as_ref()
            .is_none_or(|c| t.checks.as_ref() == Some(c))
        && (!w.conflicting || t.conflicting)
        && any_of(&w.state, &state)
        && w.review
            .as_ref()
            .is_none_or(|r| t.review.as_ref() == Some(r))
}

/// One issue, pull request or milestone an action runs on.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Item {
    pub nr: u64,
    pub title: String,
    pub url: String,
    pub branch: String,
    pub base: String,
    /// Absolute path of the failed check logs of a pull request (`{log}`); empty until the app
    /// has written them, then `{log}` stays as it is.
    #[serde(default)]
    pub log: String,
}

/// Replaces `{name}` placeholders in one pass, so a value that itself contains `{url}` stays text.
fn fill(template: &str, values: &[(&str, String)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let found = after.find('}').and_then(|end| {
            values
                .iter()
                .find(|(k, _)| *k == &after[..end])
                .map(|v| (end, v))
        });
        match found {
            Some((end, (_, v))) => {
                out.push_str(v);
                rest = &after[end + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn prompt_for(template: &str, repo: &str, items: &[&Item]) -> String {
    let first = items.first().copied().cloned().unwrap_or_default();
    let nrs = items
        .iter()
        .map(|i| i.nr.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    let mut values = vec![
        ("nr", first.nr.to_string()),
        ("nrs", nrs),
        ("title", first.title),
        ("url", first.url),
        ("repo", repo.to_owned()),
        ("branch", first.branch),
        ("base", first.base),
    ];
    if !first.log.is_empty() {
        values.push(("log", first.log));
    }
    fill(template, &values)
}

/// The argv to start for `action` on `items`: one per item for `each`, one for `once` (where `{nr}`
/// and the other single values are those of the first item and `{nrs}` joins all numbers with
/// spaces). The prompt is one argv element; the harness is the action's, else the first of the
/// registry.
pub fn expand(
    action: &Spec,
    harnesses: &[Harness],
    repo: &str,
    items: &[Item],
) -> Result<Vec<Vec<String>>, String> {
    let h = match &action.harness {
        Some(id) => harnesses
            .iter()
            .find(|h| &h.id == id)
            .ok_or_else(|| format!("unknown harness {id:?}"))?,
        None => harnesses.first().ok_or("no agent harness is configured")?,
    };
    let argv = |group: &[&Item]| {
        let prompt = prompt_for(&action.prompt, repo, group);
        harness::build_argv(h, action.model.as_deref(), Some(&prompt))
    };
    Ok(match action.batch {
        Batch::Each => items.iter().map(|i| argv(&[i])).collect(),
        Batch::Once if items.is_empty() => Vec::new(),
        Batch::Once => vec![argv(&items.iter().collect::<Vec<_>>())],
    })
}

/// A program and its arguments, the `kind` and `args` of `agent_start`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Launch {
    pub kind: String,
    pub args: Vec<String>,
}

/// What to start for `action` on `items`, with an optional harness or model chosen by the user
/// instead of the action's own (the Shift+click dialog). One launch per item for `each`, one for
/// `once`. `suffix` (fixed text of the app, placeholders allowed) is appended to the prompt.
pub fn plan(
    action: &Spec,
    harnesses: &[Harness],
    repo: &str,
    items: &[Item],
    harness: Option<&str>,
    model: Option<&str>,
    suffix: Option<&str>,
) -> Result<Vec<Launch>, String> {
    let mut spec = action.clone();
    if let Some(sfx) = suffix.map(str::trim).filter(|s| !s.is_empty()) {
        spec.prompt = format!("{}\n\n{sfx}", spec.prompt);
    }
    if let Some(h) = harness.filter(|h| !h.is_empty()) {
        spec.harness = Some(h.to_owned());
    }
    if let Some(m) = model.filter(|m| !m.is_empty()) {
        spec.model = Some(m.to_owned());
    }
    expand(&spec, harnesses, repo, items)?
        .into_iter()
        .map(|mut argv| {
            if argv.is_empty() {
                return Err("the harness has no command".to_owned());
            }
            let kind = argv.remove(0);
            Ok(Launch { kind, args: argv })
        })
        .collect()
}

#[cfg(test)]
#[path = "agent_actions_tests.rs"]
mod tests;
