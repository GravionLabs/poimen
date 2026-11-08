//! Agent actions for issues, pull requests and milestones (`github.agentActions[]` in
//! `config.json`, ADR 0005): a prompt template per action, so poimen starts agents without knowing
//! any skill. Entries merge by `id` (defaults, user, project); the prompt expands into one argv
//! element through the harness registry (`harness.rs`), never through a shell.

// The expansion is consumed by the GitHub views (a later PBI); until then only tests call it.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::harness::{self, Harness};

/// Placeholders allowed in a `prompt`.
pub const PLACEHOLDERS: &[&str] = &["nr", "nrs", "title", "url", "repo", "branch", "base"];

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
        spec(
            "review",
            "Review",
            On::Pr,
            When::default(),
            "Review pull request #{nr} in {repo}: {title} ({url}), branch {branch} into {base}. \
             Report blocking findings separately from notes.",
        ),
        spec(
            "fix-checks",
            "Fix checks",
            On::Pr,
            When {
                checks: Some("failed".into()),
                ..When::default()
            },
            "The checks of pull request #{nr} in {repo} ({url}, branch {branch}) failed. Find out \
             why from the logs and fix it.",
        ),
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
    fill(
        template,
        &[
            ("nr", first.nr.to_string()),
            ("nrs", nrs),
            ("title", first.title),
            ("url", first.url),
            ("repo", repo.to_owned()),
            ("branch", first.branch),
            ("base", first.base),
        ],
    )
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
/// `once`.
pub fn plan(
    action: &Spec,
    harnesses: &[Harness],
    repo: &str,
    items: &[Item],
    harness: Option<&str>,
    model: Option<&str>,
) -> Result<Vec<Launch>, String> {
    let mut spec = action.clone();
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
mod tests {
    use super::*;
    use serde_json::json;

    fn parsed(v: Value) -> (Vec<Spec>, Vec<String>) {
        let mut w = Vec::new();
        let s = parse(&v, "github.agentActions", &mut w);
        (s, w)
    }

    fn item(nr: u64, title: &str) -> Item {
        Item {
            nr,
            title: title.into(),
            url: format!("https://github.com/o/r/issues/{nr}"),
            branch: "feat/x".into(),
            base: "main".into(),
        }
    }

    #[test]
    fn defaults_are_neutral_and_complete() {
        let d = defaults();
        let ids: Vec<_> = d.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            ["plan", "refine", "review", "fix-checks", "address-review"]
        );
        assert_eq!(d[0].when.level, ["epic", "feature"]);
        assert!(d[0].when.no_children);
        assert_eq!(d[1].when.label, ["needs-refinement"]);
        assert_eq!(d[3].when.checks.as_deref(), Some("failed"));
        assert_eq!(d[4].when.review.as_deref(), Some("CHANGES_REQUESTED"));
        for s in &d {
            assert!(!s.prompt.starts_with('/'), "{} names a skill", s.id);
            assert!(s.prompt.len() > 20);
        }
    }

    #[test]
    fn parses_the_example_entry() {
        let (s, w) = parsed(json!([{
            "id": "refine", "label": "Refine", "on": "issue",
            "when": {"label": ["needs-refinement"]},
            "prompt": "/planner refine {nr}", "run": "repoRoot"
        }]));
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(s[0].run, Run::RepoRoot);
        assert_eq!(s[0].batch, Batch::Each);
        assert_eq!(s[0].prompt, "/planner refine {nr}");
    }

    #[test]
    fn parses_every_field() {
        let (s, w) = parsed(json!([{
            "id": "a", "on": "milestone", "prompt": "{nrs}", "run": "ask", "batch": "once",
            "harness": "copilot", "model": "auto",
            "when": {"level": ["pbi"], "state": ["open"], "noChildren": true, "checks": "failed"}
        }]));
        assert!(w.is_empty(), "{w:?}");
        let s = &s[0];
        assert_eq!(
            (s.on, s.run, s.batch),
            (On::Milestone, Run::Ask, Batch::Once)
        );
        assert_eq!(s.label, "a");
        assert_eq!(s.harness.as_deref(), Some("copilot"));
        assert_eq!(s.model.as_deref(), Some("auto"));
        assert!(s.when.no_children);
    }

    #[test]
    fn invalid_entries_warn_and_the_rest_loads() {
        let (s, w) = parsed(json!([
            {"id": "ok", "on": "pr", "prompt": "x"},
            {"id": "noprompt", "on": "pr"},
            {"id": "unknown", "on": "pr", "prompt": "{bogus}"},
            {"id": "ok", "on": "pr", "prompt": "dup"},
            {"on": "pr", "prompt": "x"},
            {"id": "badon", "on": "repo", "prompt": "x"},
            {"id": "badrun", "on": "pr", "prompt": "x", "run": "sudo"},
            {"id": "badwhen", "on": "pr", "prompt": "x", "when": {"checks": "passed"}},
            "text",
            {"id": "last", "on": "issue", "prompt": "y"}
        ]));
        let ids: Vec<_> = s.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["ok", "last"]);
        assert_eq!(s[0].prompt, "x");
        assert_eq!(w.len(), 8, "{w:?}");
        assert!(
            w.iter()
                .any(|m| m.contains("noprompt") && m.contains("prompt is missing"))
        );
        assert!(w.iter().any(|m| m.contains("unknown placeholder {bogus}")));
        assert!(w.iter().any(|m| m.contains("duplicate id")));
    }

    #[test]
    fn not_an_array_warns() {
        let (s, w) = parsed(json!({"id": "x"}));
        assert!(s.is_empty());
        assert_eq!(w, ["github.agentActions must be an array"]);
    }

    #[test]
    fn unknown_harness_is_skipped_with_a_warning() {
        let (s, _) = parsed(json!([
            {"id": "a", "on": "pr", "prompt": "x", "harness": "nope"},
            {"id": "b", "on": "pr", "prompt": "x", "harness": "claude"}
        ]));
        let mut w = Vec::new();
        let s = check_harnesses(
            s,
            &harness::default_harnesses(),
            "github.agentActions",
            &mut w,
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].id, "b");
        assert_eq!(w.len(), 1);
        assert!(w[0].contains("unknown harness"));
    }

    fn ids(actions: &[Action]) -> Vec<(&str, Source)> {
        actions
            .iter()
            .map(|a| (a.spec.id.as_str(), a.source))
            .collect()
    }

    #[test]
    fn user_entries_replace_disable_and_add() {
        let (user, w) = parsed(json!([
            {"id": "review", "on": "pr", "prompt": "/review {nr}"},
            {"id": "plan", "disabled": true},
            {"id": "mine", "on": "issue", "prompt": "m"},
            {"id": "ghost", "disabled": true}
        ]));
        assert!(w.is_empty(), "{w:?}");
        let all = resolve(&user, &[]);
        assert_eq!(
            ids(&all),
            [
                ("refine", Source::Default),
                ("review", Source::User),
                ("fix-checks", Source::Default),
                ("address-review", Source::Default),
                ("mine", Source::User),
            ]
        );
        assert_eq!(all[1].spec.prompt, "/review {nr}");
    }

    #[test]
    fn project_overrides_user_and_defaults() {
        let (user, _) = parsed(json!([{"id": "review", "on": "pr", "prompt": "user"}]));
        let (project, _) = parsed(json!([
            {"id": "review", "on": "pr", "prompt": "project"},
            {"id": "refine", "disabled": true}
        ]));
        let all = resolve(&user, &project);
        let review = all.iter().find(|a| a.spec.id == "review").unwrap();
        assert_eq!(
            (review.spec.prompt.as_str(), review.source),
            ("project", Source::Project)
        );
        assert!(all.iter().all(|a| a.spec.id != "refine"));
    }

    #[test]
    fn serializes_with_the_source() {
        let all = resolve(&[], &[]);
        let v = serde_json::to_value(&all[0]).unwrap();
        assert_eq!(v["id"], "plan");
        assert_eq!(v["source"], "default");
        assert_eq!(v["on"], "issue");
        assert_eq!(v["run"], "repoRoot");
        assert_eq!(v["when"]["noChildren"], true);
    }

    const PROJECT: &str = r#"{"github":{"agentActions":[
        {"id":"review","on":"pr","prompt":"project review {nr}"},
        {"id":"extra","on":"issue","prompt":"extra","harness":"nope"},
        {"id":"plan","disabled":true}]}}"#;

    fn sources() -> crate::trust::Sources {
        crate::trust::Sources {
            root: "/work/app".into(),
            project: Some(("/work/app/.poimen/config.json".into(), PROJECT.into())),
            legacy: None,
        }
    }

    #[test]
    fn trusted_project_overrides_per_repository() {
        let s = sources();
        let trusted = std::collections::BTreeMap::from([(s.root.clone(), s.hash().unwrap())]);
        let got = for_project(&[], &harness::default_harnesses(), &s, &trusted);
        assert!(got.note.is_none());
        let review = got.actions.iter().find(|a| a.spec.id == "review").unwrap();
        assert_eq!(review.source, Source::Project);
        assert_eq!(review.spec.prompt, "project review {nr}");
        assert!(got.actions.iter().all(|a| a.spec.id != "plan"));
        assert!(got.actions.iter().all(|a| a.spec.id != "extra"));
    }

    #[test]
    fn untrusted_project_actions_are_not_used_and_a_note_says_why() {
        let got = for_project(
            &[],
            &harness::default_harnesses(),
            &sources(),
            &std::collections::BTreeMap::new(),
        );
        assert!(got.note.is_some());
        assert_eq!(got.actions, resolve(&[], &[]));
    }

    #[test]
    fn no_project_file_means_user_and_defaults() {
        let s = crate::trust::Sources {
            root: "/x".into(),
            project: None,
            legacy: None,
        };
        let got = for_project(
            &[],
            &harness::default_harnesses(),
            &s,
            &std::collections::BTreeMap::new(),
        );
        assert_eq!(got.actions.len(), 5);
        assert!(got.note.is_none());
    }

    fn target(on: On) -> Target {
        Target {
            on: Some(on),
            ..Target::default()
        }
    }

    fn offered(t: &Target) -> Vec<String> {
        resolve(&[], &[])
            .into_iter()
            .filter(|a| matches(&a.spec, t))
            .map(|a| a.spec.id)
            .collect()
    }

    #[test]
    fn when_matching_for_the_defaults() {
        let epic = Target {
            level: Some("Epic".into()),
            ..target(On::Issue)
        };
        assert_eq!(offered(&epic), ["plan"]);
        let with_children = Target {
            has_children: true,
            ..epic.clone()
        };
        assert!(offered(&with_children).is_empty());
        let labeled = Target {
            labels: vec!["needs-refinement".into()],
            level: Some("pbi".into()),
            ..target(On::Issue)
        };
        assert_eq!(offered(&labeled), ["refine"]);
        assert!(offered(&target(On::Issue)).is_empty());
        assert_eq!(offered(&target(On::Pr)), ["review"]);
        let failing = Target {
            checks: Some("failed".into()),
            review: Some("CHANGES_REQUESTED".into()),
            ..target(On::Pr)
        };
        assert_eq!(
            offered(&failing),
            ["review", "fix-checks", "address-review"]
        );
        assert!(offered(&target(On::Milestone)).is_empty());
    }

    #[test]
    fn state_condition_matches_any_listed_state() {
        let (s, _) = parsed(json!([
            {"id": "a", "on": "pr", "prompt": "x", "when": {"state": ["open", "merged"]}}
        ]));
        let t = |st: Option<&str>| Target {
            state: st.map(str::to_owned),
            ..target(On::Pr)
        };
        assert!(matches(&s[0], &t(Some("MERGED"))));
        assert!(!matches(&s[0], &t(Some("closed"))));
        assert!(!matches(&s[0], &t(None)));
    }

    const NASTY: &str = "it's \"quoted\" $(touch x); `id` && echo {url}\nline two";

    #[test]
    fn expansion_keeps_the_prompt_one_element() {
        let (s, _) = parsed(json!([
            {"id": "a", "on": "issue", "prompt": "Fix #{nr}: {title} ({url})"}
        ]));
        let argv = expand(
            &s[0],
            &harness::default_harnesses(),
            "o/r",
            &[item(7, NASTY)],
        )
        .unwrap();
        assert_eq!(argv.len(), 1);
        assert_eq!(
            argv[0],
            [
                "claude".to_owned(),
                format!("Fix #7: {NASTY} (https://github.com/o/r/issues/7)")
            ]
        );
    }

    #[test]
    fn expansion_with_copilot_and_model() {
        let (s, _) = parsed(json!([
            {"id": "a", "on": "pr", "prompt": "{repo} {branch}->{base}",
             "harness": "copilot", "model": "auto"}
        ]));
        let argv = expand(&s[0], &harness::default_harnesses(), "o/r", &[item(1, "t")]).unwrap();
        assert_eq!(
            argv[0],
            ["copilot", "--model", "auto", "-i", "o/r feat/x->main"]
        );
    }

    #[test]
    fn each_gives_one_argv_per_item_and_once_joins_numbers() {
        let items = [item(1, "a"), item(2, "b"), item(3, "c")];
        let hs = harness::default_harnesses();
        let (each, _) = parsed(json!([{"id": "e", "on": "issue", "prompt": "{nr}|{nrs}"}]));
        let argv = expand(&each[0], &hs, "o/r", &items).unwrap();
        let prompts: Vec<_> = argv.iter().map(|a| a[1].as_str()).collect();
        assert_eq!(prompts, ["1|1", "2|2", "3|3"]);
        let (once, _) = parsed(json!([
            {"id": "o", "on": "issue", "batch": "once", "prompt": "do {nrs}"}
        ]));
        let argv = expand(&once[0], &hs, "o/r", &items).unwrap();
        assert_eq!(argv, [["claude", "do 1 2 3"]]);
        assert!(expand(&once[0], &hs, "o/r", &[]).unwrap().is_empty());
    }

    #[test]
    fn values_with_placeholders_are_not_expanded_again() {
        let (s, _) = parsed(json!([{"id": "a", "on": "issue", "prompt": "{title}|{nr}"}]));
        let argv = expand(
            &s[0],
            &harness::default_harnesses(),
            "r",
            &[item(4, "{nr}")],
        )
        .unwrap();
        assert_eq!(argv[0][1], "{nr}|4");
    }

    #[test]
    fn unknown_harness_at_expansion_is_an_error() {
        let (mut s, _) = parsed(json!([{"id": "a", "on": "issue", "prompt": "x"}]));
        s[0].harness = Some("gone".into());
        let e = expand(&s[0], &harness::default_harnesses(), "r", &[item(1, "t")]).unwrap_err();
        assert!(e.contains("gone"));
    }

    #[test]
    fn plan_splits_the_program_and_applies_the_chosen_harness_and_model() {
        let d = defaults();
        let review = d.iter().find(|s| s.id == "review").unwrap();
        let hs = harness::default_harnesses();
        let items = [item(7, "A"), item(8, "B")];
        let each = plan(review, &hs, "o/r", &items, None, None).unwrap();
        assert_eq!(each.len(), 2);
        assert_eq!(each[0].kind, "claude");
        assert!(each[1].args[0].contains("#8"));
        let chosen = plan(
            review,
            &hs,
            "o/r",
            &items[..1],
            Some("copilot"),
            Some("auto"),
        )
        .unwrap();
        assert_eq!(chosen[0].kind, "copilot");
        assert_eq!(&chosen[0].args[..2], ["--model", "auto"]);
        assert!(plan(review, &hs, "o/r", &items, Some("nope"), None).is_err());
    }

    #[test]
    fn plan_for_once_is_one_launch_with_all_numbers() {
        let mut once = defaults().remove(0);
        once.batch = Batch::Once;
        once.prompt = "do {nrs}".into();
        let hs = harness::default_harnesses();
        let l = plan(&once, &hs, "o/r", &[item(1, "a"), item(2, "b")], None, None).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].args.last().unwrap(), "do 1 2");
    }
}
