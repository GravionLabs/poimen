//! Tests of `github_pulls.rs` (kept in a file of their own: the module is long).

use super::*;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/github-pulls.json")).unwrap()
}

#[test]
fn parses_a_pull_with_checks_and_review_state() {
    let pulls = parse_pulls(&fixture()).unwrap();
    assert_eq!(pulls.len(), 3);
    let p = &pulls[0];
    assert_eq!(p.number, 231);
    assert_eq!(p.author.as_deref(), Some("jkaufmann"));
    assert_eq!((p.additions, p.deletions), (420, 12));
    assert_eq!(p.review_decision.as_deref(), Some("REVIEW_REQUIRED"));
    assert_eq!(p.review_requests, vec!["octocat", "maintainers"]);
    assert_eq!(p.head_ref, "feat/231-github-pulls-view");
    assert!(!p.is_cross_repository);
    // 5 + 1 + 1 passed, 1 timed out, 1 in progress + 1 pending, 2 skipped not counted.
    assert_eq!(
        p.checks,
        Checks {
            state: "failing".into(),
            passed: 7,
            failed: 1,
            pending: 2
        }
    );
}

#[test]
fn a_draft_without_author_review_or_checks_is_complete() {
    let p = &parse_pulls(&fixture()).unwrap()[1];
    assert!(p.is_draft);
    assert_eq!(p.author, None);
    assert_eq!(p.review_decision, None);
    assert!(p.review_requests.is_empty());
    assert_eq!(p.checks.state, "none");
}

#[test]
fn the_home_shortcut_is_expanded() {
    let home = std::path::Path::new("/home/jk");
    assert_eq!(expand_home("~", Some(home)), "/home/jk");
    assert_eq!(expand_home("~/src", Some(home)), "/home/jk/src");
    assert_eq!(expand_home("/work/~x", Some(home)), "/work/~x");
    assert_eq!(expand_home("~/src", None), "~/src");
}

#[test]
fn green_checks_are_passing() {
    let p = &parse_pulls(&fixture()).unwrap()[2];
    assert_eq!(p.checks.state, "passing");
    assert_eq!(p.review_decision.as_deref(), Some("APPROVED"));
}

#[test]
fn an_empty_list_is_ok_and_a_missing_repository_is_an_error() {
    let empty = json!({"data":{"repository":{"pullRequests":{"nodes":[]}}}});
    assert_eq!(parse_pulls(&empty).unwrap(), vec![]);
    let missing = json!({"data":{"repository":null},"errors":[{"message":"Could not resolve"}]});
    assert_eq!(
        parse_pulls(&missing),
        Err(GhError::Other {
            message: "Could not resolve".into()
        })
    );
}

#[test]
fn the_rollup_state_decides_when_no_counts_exist() {
    let node = json!({"commits":{"nodes":[{"commit":{"statusCheckRollup":
        {"state":"FAILURE","contexts":{"checkRunCountsByState":[],"statusContextCountsByState":[]}}}}]}});
    assert_eq!(parse_checks(&node).state, "failing");
}

#[test]
fn the_wire_shape_is_camel_case_with_a_status_tag() {
    let repo = RepoPulls {
        root: "/r".into(),
        repo: "o/n".into(),
        outcome: Outcome::Error {
            error: GhError::NotLoggedIn,
        },
    };
    assert_eq!(
        serde_json::to_value(&repo).unwrap(),
        json!({"root":"/r","repo":"o/n","status":"error","error":{"kind":"not-logged-in"}})
    );
    let pull = serde_json::to_value(&parse_pulls(&fixture()).unwrap()[0]).unwrap();
    assert_eq!(pull["isDraft"], false);
    assert_eq!(pull["reviewDecision"], "REVIEW_REQUIRED");
}

fn detail_fixture() -> Value {
    serde_json::from_str(include_str!("../fixtures/github-pull-detail.json")).unwrap()
}

#[test]
fn parses_the_detail_fixture() {
    let d = parse_detail(&detail_fixture()).unwrap();
    assert_eq!(d.number, 336);
    assert_eq!(d.author.as_deref(), Some("jkaufmann"));
    assert_eq!(d.head_ref, "feat/336-pull-request-detail-pane");
    assert_eq!(d.base_ref, "release/next");
    assert_eq!(
        (d.mergeable.as_str(), d.merge_state_status.as_str()),
        ("MERGEABLE", "BLOCKED")
    );
    assert!(d.body.starts_with("Adds the **detail pane**"));
    assert!(d.viewer_can_update && d.viewer_did_author && !d.viewer_can_merge_as_admin);
    let checks: Vec<_> = d
        .checks
        .iter()
        .map(|c| (c.name.as_str(), c.state.as_str()))
        .collect();
    assert_eq!(
        checks,
        vec![
            ("frontend", "passing"),
            ("rust", "failing"),
            ("windows", "pending"),
            ("docs", "skipped"),
            ("ci/legacy", "pending"),
            ("ci/lint", "passing"),
        ]
    );
    assert_eq!(
        d.checks[0].url.as_deref(),
        Some("https://github.com/GravionLabs/poimen/actions/runs/1")
    );
    assert_eq!(d.checks[3].url, None);
    assert_eq!(d.reviews.len(), 3);
    assert_eq!(d.reviews[0].author.as_deref(), Some("octocat"));
    assert_eq!(d.reviews[0].state, "CHANGES_REQUESTED");
    assert_eq!(d.reviews[1].author, None);
    assert_eq!(d.reviews[2].submitted_at, None);
    assert_eq!(d.linked_issues.len(), 2);
    assert_eq!(d.linked_issues[1].repo, "GravionLabs/other");
    assert_eq!(d.linked_issues[1].state, "CLOSED");
}

#[test]
fn a_bare_pull_request_parses_with_defaults_and_a_missing_one_is_an_error() {
    let bare = json!({"data":{"repository":{"pullRequest":{"number":5}}}});
    let d = parse_detail(&bare).unwrap();
    assert!(d.checks.is_empty() && d.reviews.is_empty() && d.linked_issues.is_empty());
    assert_eq!(d.mergeable, "UNKNOWN");
    assert!(!d.viewer_can_update);
    let missing = json!({"data":{"repository":{"pullRequest":null}},"errors":[{"message":"Could not resolve to a PullRequest"}]});
    assert_eq!(
        parse_detail(&missing),
        Err(GhError::Other {
            message: "Could not resolve to a PullRequest".into()
        })
    );
}

#[test]
fn the_detail_wire_shape_is_camel_case() {
    let v = serde_json::to_value(parse_detail(&detail_fixture()).unwrap()).unwrap();
    assert_eq!(v["viewerCanMergeAsAdmin"], false);
    assert_eq!(v["linkedIssues"][0]["repo"], "GravionLabs/poimen");
    assert_eq!(v["reviews"][0]["submittedAt"], "2026-11-05T10:00:00Z");
}

#[test]
fn action_argv_per_action() {
    let r = Repo {
        owner: "o".into(),
        name: "n".into(),
    };
    let a = |act, body| action_argv(&r, 7, act, body).unwrap();
    assert_eq!(
        a(PrAction::Approve, None),
        ["pr", "review", "7", "--approve", "-R", "o/n"]
    );
    assert_eq!(
        a(PrAction::RequestChanges, Some(" fix it ")),
        [
            "pr",
            "review",
            "7",
            "--request-changes",
            "-b",
            "fix it",
            "-R",
            "o/n"
        ]
    );
    assert_eq!(
        a(PrAction::Comment, Some("hi")),
        ["pr", "review", "7", "--comment", "-b", "hi", "-R", "o/n"]
    );
    assert_eq!(a(PrAction::Ready, None), ["pr", "ready", "7", "-R", "o/n"]);
    assert_eq!(
        a(PrAction::Draft, None),
        ["pr", "ready", "7", "--undo", "-R", "o/n"]
    );
    assert_eq!(a(PrAction::Close, None), ["pr", "close", "7", "-R", "o/n"]);
    assert_eq!(
        a(PrAction::Reopen, None),
        ["pr", "reopen", "7", "-R", "o/n"]
    );
}

#[test]
fn request_changes_and_comment_need_a_body() {
    let r = Repo {
        owner: "o".into(),
        name: "n".into(),
    };
    for act in [PrAction::RequestChanges, PrAction::Comment] {
        assert!(action_argv(&r, 1, act, None).is_err());
        assert!(action_argv(&r, 1, act, Some("  \n")).is_err());
    }
}
fn repo() -> Repo {
    Repo {
        owner: "o".into(),
        name: "n".into(),
    }
}

#[test]
fn detail_fixture_has_merge_and_people_fields() {
    let d = parse_detail(&detail_fixture()).unwrap();
    assert_eq!(d.head_sha, "0123456789abcdef0123456789abcdef01234567");
    assert_eq!(d.merge_methods, ["squash", "rebase"]);
    assert_eq!(
        d.merge_block.as_deref(),
        Some("Required reviews or checks are missing.")
    );
    assert_eq!(d.reviewers, ["octocat"]);
    assert_eq!(d.labels, ["bug", "github"]);
    assert_eq!(d.assignees, ["jkaufmann"]);
    assert_eq!(d.assignable_users, ["jkaufmann", "octocat", "hubot"]);
}

#[test]
fn merge_gating_by_state() {
    for ok in ["CLEAN", "UNSTABLE", "HAS_HOOKS"] {
        assert_eq!(merge_block("OPEN", false, ok), None, "{ok}");
    }
    for bad in ["BLOCKED", "BEHIND", "DIRTY", "UNKNOWN", "DRAFT"] {
        assert!(merge_block("OPEN", false, bad).is_some(), "{bad}");
    }
    assert!(merge_block("MERGED", false, "CLEAN").is_some());
    assert!(merge_block("OPEN", true, "CLEAN").is_some());
}

#[test]
fn merge_argv_per_method() {
    let a = |m, d| merge_argv(&repo(), 7, m, d);
    assert_eq!(
        a(MergeMethod::Squash, true),
        [
            "pr",
            "merge",
            "7",
            "--squash",
            "--delete-branch",
            "-R",
            "o/n"
        ]
    );
    assert_eq!(
        a(MergeMethod::Merge, false),
        ["pr", "merge", "7", "--merge", "-R", "o/n"]
    );
    assert_eq!(
        a(MergeMethod::Rebase, false),
        ["pr", "merge", "7", "--rebase", "-R", "o/n"]
    );
}

#[test]
fn rerun_argv_is_failed_only() {
    assert_eq!(
        rerun_argv(&repo(), 102),
        ["run", "rerun", "102", "--failed", "-R", "o/n"]
    );
}

#[test]
fn edit_argv_has_one_flag_per_value() {
    let edit = PrEdit {
        add_reviewers: vec!["octocat".into()],
        remove_labels: vec!["good first issue".into(), "bug".into()],
        add_assignees: vec!["hubot".into()],
        ..PrEdit::default()
    };
    assert_eq!(
        edit_argv(&repo(), 7, &edit).unwrap(),
        [
            "pr",
            "edit",
            "7",
            "--add-reviewer",
            "octocat",
            "--remove-label",
            "good first issue",
            "--remove-label",
            "bug",
            "--add-assignee",
            "hubot",
            "-R",
            "o/n"
        ]
    );
    let all = PrEdit {
        remove_reviewers: vec!["a".into()],
        add_labels: vec!["b".into()],
        remove_assignees: vec!["c".into()],
        ..PrEdit::default()
    };
    assert_eq!(
        edit_argv(&repo(), 1, &all).unwrap()[3..9],
        [
            "--remove-reviewer",
            "a",
            "--add-label",
            "b",
            "--remove-assignee",
            "c"
        ]
    );
}

#[test]
fn edit_refuses_nothing_blank_and_comma_values() {
    assert!(edit_argv(&repo(), 1, &PrEdit::default()).is_err());
    for bad in ["", "  ", "a,b"] {
        let edit = PrEdit {
            add_labels: vec![bad.into()],
            ..PrEdit::default()
        };
        assert!(edit_argv(&repo(), 1, &edit).is_err(), "{bad:?}");
    }
}

#[test]
fn labels_parse_from_the_rest_answer() {
    let body = include_str!("../fixtures/github-labels.json");
    assert_eq!(
        parse_labels(body).unwrap(),
        ["bug", "github", "good first issue"]
    );
    assert!(parse_labels("{}").is_err());
}
