//! Open pull requests per repository for the GitHub view (ADR 0005, #231): one GraphQL query per
//! repository through the shared `github::Client`, plus the viewer's login once. Read-only.

use serde::Serialize;
use serde_json::{Value, json};

use crate::git;
use crate::github::{Client, GhError, Repo, Runner, repo_of};

pub(crate) const QUERY: &str = "query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { \
pullRequests(states: OPEN, first: 50, orderBy: {field: CREATED_AT, direction: DESC}) { nodes { \
number title url author { login } isDraft additions deletions createdAt headRefName isCrossRepository baseRefName \
reviewDecision reviewRequests(first: 20) { nodes { requestedReviewer { __typename \
... on User { login } ... on Team { slug } } } } \
commits(last: 1) { nodes { commit { statusCheckRollup { state contexts(first: 1) { \
checkRunCountsByState { state count } statusContextCountsByState { state count } } } } } } } } } }";

const VIEWER_QUERY: &str = "query { viewer { login } }";

/// Check counts of the last commit; `state` is `none`, `passing`, `failing` or `pending`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Checks {
    pub state: String,
    pub passed: u32,
    pub failed: u32,
    pub pending: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Pull {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub author: Option<String>,
    pub is_draft: bool,
    pub additions: u64,
    pub deletions: u64,
    pub created_at: String,
    pub head_ref: String,
    /// The head branch lives in a fork (a missing answer counts as a fork).
    pub is_cross_repository: bool,
    pub base_ref: String,
    /// `APPROVED`, `CHANGES_REQUESTED`, `REVIEW_REQUIRED` or `None` (no review rule).
    pub review_decision: Option<String>,
    /// Logins of requested users and slugs of requested teams.
    pub review_requests: Vec<String>,
    pub checks: Checks,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    Ok { pulls: Vec<Pull> },
    Error { error: GhError },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepoPulls {
    pub root: String,
    /// `owner/name`.
    pub repo: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pulls {
    /// Login of the signed-in user; `None` when it could not be read.
    pub viewer: Option<String>,
    pub repos: Vec<RepoPulls>,
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn count(v: &Value) -> u64 {
    v.as_u64().unwrap_or(0)
}

/// Adds the counts of one `…CountsByState` list to passed/failed/pending; skipped and unknown
/// states are not counted.
fn add_counts(list: &Value, checks: &mut Checks) {
    for item in list.as_array().into_iter().flatten() {
        let n = count(&item["count"]) as u32;
        match item["state"].as_str().unwrap_or_default() {
            "SUCCESS" | "NEUTRAL" => checks.passed += n,
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELED" | "STARTUP_FAILURE"
            | "ACTION_REQUIRED" | "STALE" => checks.failed += n,
            "PENDING" | "QUEUED" | "IN_PROGRESS" | "WAITING" | "REQUESTED" | "EXPECTED" => {
                checks.pending += n
            }
            _ => {}
        }
    }
}

pub(crate) fn parse_checks(node: &Value) -> Checks {
    let rollup = &node["commits"]["nodes"][0]["commit"]["statusCheckRollup"];
    let mut checks = Checks {
        state: "none".into(),
        passed: 0,
        failed: 0,
        pending: 0,
    };
    if rollup.is_null() {
        return checks;
    }
    add_counts(&rollup["contexts"]["checkRunCountsByState"], &mut checks);
    add_counts(
        &rollup["contexts"]["statusContextCountsByState"],
        &mut checks,
    );
    checks.state = if checks.failed > 0 {
        "failing"
    } else if checks.pending > 0 {
        "pending"
    } else if checks.passed > 0 {
        "passing"
    } else {
        // Nothing counted: fall back to the overall state of the rollup.
        match rollup["state"].as_str().unwrap_or_default() {
            "SUCCESS" => "passing",
            "FAILURE" | "ERROR" => "failing",
            "PENDING" | "EXPECTED" => "pending",
            _ => "none",
        }
    }
    .into();
    checks
}

fn parse_pull(node: &Value) -> Option<Pull> {
    let number = node["number"].as_u64()?;
    let review_requests = node["reviewRequests"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| {
            let r = &n["requestedReviewer"];
            r["login"]
                .as_str()
                .or_else(|| r["slug"].as_str())
                .map(str::to_owned)
        })
        .collect();
    Some(Pull {
        number,
        title: text(&node["title"]),
        url: text(&node["url"]),
        author: node["author"]["login"].as_str().map(str::to_owned),
        is_draft: node["isDraft"].as_bool().unwrap_or(false),
        additions: count(&node["additions"]),
        deletions: count(&node["deletions"]),
        created_at: text(&node["createdAt"]),
        head_ref: text(&node["headRefName"]),
        is_cross_repository: node["isCrossRepository"].as_bool().unwrap_or(true),
        base_ref: text(&node["baseRefName"]),
        review_decision: node["reviewDecision"].as_str().map(str::to_owned),
        review_requests,
        checks: parse_checks(node),
    })
}

/// The pull requests of a `repository { pullRequests }` GraphQL answer; entries without a number
/// are skipped. A missing repository (or GraphQL `errors` without data) is an error.
pub fn parse_pulls(answer: &Value) -> Result<Vec<Pull>, GhError> {
    let nodes = &answer["data"]["repository"]["pullRequests"]["nodes"];
    let Some(nodes) = nodes.as_array() else {
        let message = answer["errors"][0]["message"]
            .as_str()
            .unwrap_or("unexpected GitHub answer");
        return Err(GhError::Other {
            message: message.to_owned(),
        });
    };
    Ok(nodes.iter().filter_map(parse_pull).collect())
}

fn load_repo<R: Runner>(client: &Client<R>, repo: &Repo) -> Outcome {
    let vars = json!({ "owner": repo.owner, "name": repo.name });
    match client
        .graphql(QUERY, &vars)
        .and_then(|answer| parse_pulls(&answer))
    {
        Ok(pulls) => Outcome::Ok { pulls },
        Err(error) => Outcome::Error { error },
    }
}

fn load_viewer<R: Runner>(client: &Client<R>) -> Option<String> {
    let answer = client.graphql(VIEWER_QUERY, &json!({})).ok()?;
    answer["data"]["viewer"]["login"]
        .as_str()
        .map(str::to_owned)
}

/// `~` and `~/x` of the explorer roots resolved against `home`; other paths stay as they are.
pub fn expand_home(dir: &str, home: Option<&std::path::Path>) -> String {
    let Some(home) = home else {
        return dir.to_owned();
    };
    if dir == "~" {
        return home.to_string_lossy().into_owned();
    }
    match dir.strip_prefix("~/") {
        Some(rest) => home.join(rest).to_string_lossy().into_owned(),
        None => dir.to_owned(),
    }
}

/// Open pull requests of the GitHub repositories among `dirs` (worktrees count as their main
/// repository; directories without a GitHub `origin` are left out).
pub fn load<R: Runner + Sync>(client: &Client<R>, dirs: &[String]) -> Pulls {
    let mut seen = std::collections::HashSet::new();
    let repos: Vec<(String, Repo)> = git::repo_roots(dirs)
        .into_iter()
        .filter_map(|root| {
            let repo = repo_of(std::path::Path::new(&root))?;
            seen.insert((repo.owner.to_lowercase(), repo.name.to_lowercase()))
                .then_some((root, repo))
        })
        .collect();
    std::thread::scope(|scope| {
        let viewer = scope.spawn(|| load_viewer(client));
        let handles: Vec<_> = repos
            .iter()
            .map(|(_, repo)| scope.spawn(|| load_repo(client, repo)))
            .collect();
        let repos = repos
            .iter()
            .zip(handles)
            .map(|((root, repo), handle)| RepoPulls {
                root: root.clone(),
                repo: format!("{}/{}", repo.owner, repo.name),
                outcome: handle.join().unwrap_or_else(|_| Outcome::Error {
                    error: GhError::Other {
                        message: "the GitHub reader stopped unexpectedly".into(),
                    },
                }),
            })
            .collect();
        Pulls {
            viewer: viewer.join().ok().flatten(),
            repos,
        }
    })
}

const DETAIL_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) { repository(owner: $owner, name: $name) { \
pullRequest(number: $number) { number title url body state isDraft author { login } headRefName baseRefName \
mergeable mergeStateStatus viewerCanUpdate viewerDidAuthor viewerCanMergeAsAdmin \
commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes { __typename \
... on CheckRun { name status conclusion detailsUrl } ... on StatusContext { context state targetUrl } } } } } } } \
reviews(last: 50) { nodes { author { login } state submittedAt } } \
closingIssuesReferences(first: 20) { nodes { number title url state repository { nameWithOwner } } } } } }";

/// One check context of the last commit; `state` is `passing`, `failing`, `pending` or `skipped`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CheckContext {
    pub name: String,
    pub state: String,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub author: Option<String>,
    pub state: String,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedIssue {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    /// `owner/name` of the issue's repository.
    pub repo: String,
}

/// A pull request in detail (`github_pull_detail`); the `viewer_can_*` flags are for the actions
/// of later pull requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullDetail {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub body: String,
    pub state: String,
    pub is_draft: bool,
    pub author: Option<String>,
    pub head_ref: String,
    pub base_ref: String,
    /// `MERGEABLE`, `CONFLICTING` or `UNKNOWN`.
    pub mergeable: String,
    pub merge_state_status: String,
    pub checks: Vec<CheckContext>,
    pub reviews: Vec<Review>,
    pub linked_issues: Vec<LinkedIssue>,
    pub viewer_can_update: bool,
    pub viewer_did_author: bool,
    pub viewer_can_merge_as_admin: bool,
}

fn opt_text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_owned)
}

fn parse_context(node: &Value) -> Option<CheckContext> {
    if node["__typename"] == "StatusContext" {
        let state = match node["state"].as_str().unwrap_or_default() {
            "SUCCESS" => "passing",
            "FAILURE" | "ERROR" => "failing",
            _ => "pending",
        };
        return Some(CheckContext {
            name: node["context"].as_str()?.to_owned(),
            state: state.into(),
            url: opt_text(&node["targetUrl"]),
        });
    }
    let state = if node["status"] != "COMPLETED" {
        "pending"
    } else {
        match node["conclusion"].as_str().unwrap_or_default() {
            "SUCCESS" => "passing",
            "NEUTRAL" | "SKIPPED" => "skipped",
            _ => "failing",
        }
    };
    Some(CheckContext {
        name: node["name"].as_str()?.to_owned(),
        state: state.into(),
        url: opt_text(&node["detailsUrl"]),
    })
}

/// The `repository { pullRequest }` answer of [`DETAIL_QUERY`]; a missing pull request is an error.
pub fn parse_detail(answer: &Value) -> Result<PullDetail, GhError> {
    let node = &answer["data"]["repository"]["pullRequest"];
    let Some(number) = node["number"].as_u64() else {
        let message = answer["errors"][0]["message"]
            .as_str()
            .unwrap_or("unexpected GitHub answer");
        return Err(GhError::Other {
            message: message.to_owned(),
        });
    };
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let flag = |k: &str| node[k].as_bool().unwrap_or(false);
    Ok(PullDetail {
        number,
        title: text(&node["title"]),
        url: text(&node["url"]),
        body: text(&node["body"]),
        state: text(&node["state"]),
        is_draft: flag("isDraft"),
        author: opt_text(&node["author"]["login"]),
        head_ref: text(&node["headRefName"]),
        base_ref: text(&node["baseRefName"]),
        mergeable: node["mergeable"].as_str().unwrap_or("UNKNOWN").to_owned(),
        merge_state_status: node["mergeStateStatus"]
            .as_str()
            .unwrap_or("UNKNOWN")
            .to_owned(),
        checks: list(
            &node["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"],
        )
        .iter()
        .filter_map(parse_context)
        .collect(),
        reviews: list(&node["reviews"]["nodes"])
            .iter()
            .map(|r| Review {
                author: opt_text(&r["author"]["login"]),
                state: text(&r["state"]),
                submitted_at: opt_text(&r["submittedAt"]),
            })
            .collect(),
        linked_issues: list(&node["closingIssuesReferences"]["nodes"])
            .iter()
            .filter_map(|i| {
                Some(LinkedIssue {
                    number: i["number"].as_u64()?,
                    title: text(&i["title"]),
                    url: text(&i["url"]),
                    state: text(&i["state"]),
                    repo: text(&i["repository"]["nameWithOwner"]),
                })
            })
            .collect(),
        viewer_can_update: flag("viewerCanUpdate"),
        viewer_did_author: flag("viewerDidAuthor"),
        viewer_can_merge_as_admin: flag("viewerCanMergeAsAdmin"),
    })
}

/// Detail of pull request `number` in the GitHub repository of `root` (one GraphQL query, cached
/// 30 s by the client).
pub fn load_detail<R: Runner>(
    client: &Client<R>,
    root: &str,
    number: u64,
) -> Result<PullDetail, GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    let vars = json!({ "owner": repo.owner, "name": repo.name, "number": number });
    parse_detail(&client.graphql(DETAIL_QUERY, &vars)?)
}

#[cfg(test)]
mod tests {
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
        let missing =
            json!({"data":{"repository":null},"errors":[{"message":"Could not resolve"}]});
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
}
