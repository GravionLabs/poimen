//! Tests of `agent_actions.rs` (kept in a file of their own: the module is long).

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
        log: String::new(),
    }
}

#[test]
fn defaults_are_neutral_and_complete() {
    let d = defaults();
    let ids: Vec<_> = d.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "plan",
            "refine",
            "review",
            "fix-checks",
            "resolve-conflicts",
            "address-review"
        ]
    );
    assert_eq!(d[0].when.level, ["epic", "feature"]);
    assert!(d[0].when.no_children);
    assert_eq!(d[1].when.label, ["needs-refinement"]);
    assert_eq!(d[3].when.checks.as_deref(), Some("failed"));
    assert!(d[4].when.conflicting);
    assert_eq!(d[5].when.review.as_deref(), Some("CHANGES_REQUESTED"));
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
            ("resolve-conflicts", Source::Default),
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
    assert_eq!(got.actions.len(), 6);
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
    let conflicting = Target {
        conflicting: true,
        ..target(On::Pr)
    };
    assert_eq!(offered(&conflicting), ["review", "resolve-conflicts"]);
}

#[test]
fn resolve_conflicts_is_a_worktree_action_naming_the_base() {
    let d = defaults();
    let a = d.iter().find(|s| s.id == "resolve-conflicts").unwrap();
    assert_eq!(a.run, Run::Worktree);
    let hs = harness::default_harnesses();
    let mut it = item(9, "T");
    it.base = "release/next".into();
    let l = plan(a, &hs, "o/r", &[it], None, None, None).unwrap();
    assert!(
        l[0].args
            .last()
            .unwrap()
            .contains("Merge release/next into")
    );
}

#[test]
fn when_conflicting_is_parsed() {
    let (s, warn) = parsed(json!([
        {"id": "c", "on": "pr", "prompt": "x", "when": {"conflicting": true}},
        {"id": "d", "on": "pr", "prompt": "x", "when": {"conflicting": "yes"}}
    ]));
    assert!(s[0].when.conflicting);
    assert_eq!(s.len(), 1);
    assert_eq!(warn.len(), 1);
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
    let each = plan(review, &hs, "o/r", &items, None, None, None).unwrap();
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
        None,
    )
    .unwrap();
    assert_eq!(chosen[0].kind, "copilot");
    assert_eq!(&chosen[0].args[..2], ["--model", "auto"]);
    assert!(plan(review, &hs, "o/r", &items, Some("nope"), None, None).is_err());
}

#[test]
fn plan_for_once_is_one_launch_with_all_numbers() {
    let mut once = defaults().remove(0);
    once.batch = Batch::Once;
    once.prompt = "do {nrs}".into();
    let hs = harness::default_harnesses();
    let l = plan(
        &once,
        &hs,
        "o/r",
        &[item(1, "a"), item(2, "b")],
        None,
        None,
        None,
    )
    .unwrap();
    assert_eq!(l.len(), 1);
    assert_eq!(l[0].args.last().unwrap(), "do 1 2");
}

#[test]
fn review_is_ask_and_names_no_skill() {
    let d = defaults();
    let review = d.iter().find(|s| s.id == "review").unwrap();
    assert_eq!(review.run, Run::Ask);
    assert_eq!(review.on, On::Pr);
    assert!(!review.prompt.starts_with('/'));
    assert!(review.prompt.contains("against its base {base}"));
}

#[test]
fn plan_appends_the_suffix_with_placeholders_expanded() {
    let d = defaults();
    let review = d.iter().find(|s| s.id == "review").unwrap();
    let hs = harness::default_harnesses();
    let l = plan(
        review,
        &hs,
        "o/r",
        &[item(7, "A")],
        None,
        None,
        Some("Run `gh pr diff {nr}`."),
    )
    .unwrap();
    assert!(
        l[0].args
            .last()
            .unwrap()
            .ends_with("\n\nRun `gh pr diff 7`.")
    );
    let none = plan(review, &hs, "o/r", &[item(7, "A")], None, None, Some("  ")).unwrap();
    assert!(!none[0].args.last().unwrap().contains("gh pr"));
}

#[test]
fn log_is_valid_only_on_pull_requests() {
    let (s, w) = parsed(json!([
        {"id": "ok", "on": "pr", "prompt": "see {log}"},
        {"id": "bad", "on": "issue", "prompt": "see {log}"}
    ]));
    assert_eq!(s.iter().filter(|x| x.id == "ok").count(), 1);
    assert!(
        w.iter()
            .any(|m| m.contains("bad") && m.contains("{log} is only valid"))
    );
}

#[test]
fn fix_checks_is_a_worktree_action_pointing_to_the_log() {
    let d = defaults();
    let fix = d.iter().find(|s| s.id == "fix-checks").unwrap();
    assert_eq!(fix.run, Run::Worktree);
    let hs = harness::default_harnesses();
    let mut it = item(9, "T");
    it.log = "/w/.agents/state/ci-failed-9.log".into();
    let l = plan(fix, &hs, "o/r", &[it], None, None, None).unwrap();
    assert!(
        l[0].args
            .last()
            .unwrap()
            .contains("are in /w/.agents/state/ci-failed-9.log.")
    );
    // Without a log yet the placeholder stays for the second planning.
    let l = plan(fix, &hs, "o/r", &[item(9, "T")], None, None, None).unwrap();
    assert!(l[0].args.last().unwrap().contains("{log}"));
}
