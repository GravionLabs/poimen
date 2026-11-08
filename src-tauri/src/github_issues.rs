//! Issues per repository for the GitHub view (ADR 0005, #232): paginated GraphQL through the shared
//! `github::Client` with native parent links (sub-issues) and the pull requests linked to each issue
//! (closing references plus open pull requests whose branch is `<type>/<nr>-...`). The tree and the
//! levels are built by the frontend. Read-only.

use serde::Serialize;
use serde_json::{Value, json};

use crate::git;
use crate::github::{Client, GhError, Repo, Runner, repo_of};
use crate::github_pulls::{self, Checks};

/// Issues read per repository at most (three pages of 100).
pub const MAX_ISSUES: usize = 300;
const PAGE: usize = 100;

const PR_FIELDS: &str = "number url state headRefName \
commits(last: 1) { nodes { commit { statusCheckRollup { state contexts(first: 1) { \
checkRunCountsByState { state count } statusContextCountsByState { state count } } } } } }";

fn issues_query(closed: bool) -> String {
    let states = if closed { "CLOSED" } else { "OPEN" };
    format!(
        "query($owner: String!, $name: String!, $cursor: String) {{ repository(owner: $owner, name: $name) {{ \
issues(states: {states}, first: {PAGE}, after: $cursor, orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ \
pageInfo {{ hasNextPage endCursor }} nodes {{ number title url state labels(first: 20) {{ nodes {{ name }} }} \
assignees(first: 10) {{ nodes {{ login }} }} milestone {{ title }} \
parent {{ number repository {{ nameWithOwner }} }} subIssuesSummary {{ total completed }} \
closedByPullRequestsReferences(first: 5, includeClosedPrs: true) {{ nodes {{ {PR_FIELDS} }} }} }} }} }} }}"
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Parent {
    pub number: u64,
    /// `owner/name` of the parent.
    pub repo: String,
}

/// A pull request linked to an issue; `state` is `OPEN`, `MERGED` or `CLOSED`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkedPr {
    pub number: u64,
    pub url: String,
    pub state: String,
    pub checks: Checks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// `OPEN` or `CLOSED`.
    pub state: String,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: Option<String>,
    pub parent: Option<Parent>,
    pub sub_total: u64,
    pub sub_completed: u64,
    pub prs: Vec<LinkedPr>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    Ok { issues: Vec<Issue>, truncated: bool },
    Error { error: GhError },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepoIssues {
    pub root: String,
    /// `owner/name`.
    pub repo: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

fn names(list: &Value, field: &str) -> Vec<String> {
    list["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| n[field].as_str().map(str::to_owned))
        .collect()
}

fn linked_pr(node: &Value) -> Option<LinkedPr> {
    Some(LinkedPr {
        number: node["number"].as_u64()?,
        url: node["url"].as_str().unwrap_or_default().to_owned(),
        state: node["state"].as_str().unwrap_or("OPEN").to_owned(),
        checks: github_pulls::parse_checks(node),
    })
}

fn parse_issue(node: &Value) -> Option<Issue> {
    let number = node["number"].as_u64()?;
    let parent = node["parent"]["number"].as_u64().map(|n| Parent {
        number: n,
        repo: node["parent"]["repository"]["nameWithOwner"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
    });
    let mut prs: Vec<LinkedPr> = Vec::new();
    for pr in node["closedByPullRequestsReferences"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(linked_pr)
    {
        if !prs.iter().any(|p| p.number == pr.number) {
            prs.push(pr);
        }
    }
    Some(Issue {
        number,
        title: node["title"].as_str().unwrap_or_default().to_owned(),
        url: node["url"].as_str().unwrap_or_default().to_owned(),
        state: node["state"].as_str().unwrap_or("OPEN").to_owned(),
        labels: names(&node["labels"], "name"),
        assignees: names(&node["assignees"], "login"),
        milestone: node["milestone"]["title"].as_str().map(str::to_owned),
        parent,
        sub_total: node["subIssuesSummary"]["total"].as_u64().unwrap_or(0),
        sub_completed: node["subIssuesSummary"]["completed"].as_u64().unwrap_or(0),
        prs,
    })
}

/// One page of a `repository { issues }` answer: its issues (entries without a number are skipped)
/// and the cursor of the next page. A missing repository is an error.
pub fn parse_page(answer: &Value) -> Result<(Vec<Issue>, Option<String>), GhError> {
    let issues = &answer["data"]["repository"]["issues"];
    let Some(nodes) = issues["nodes"].as_array() else {
        let message = answer["errors"][0]["message"]
            .as_str()
            .unwrap_or("unexpected GitHub answer");
        return Err(GhError::Other {
            message: message.to_owned(),
        });
    };
    let next = issues["pageInfo"]["hasNextPage"]
        .as_bool()
        .unwrap_or(false)
        .then(|| issues["pageInfo"]["endCursor"].as_str().map(str::to_owned))
        .flatten();
    Ok((nodes.iter().filter_map(parse_issue).collect(), next))
}

/// Whether `head_ref` is `<type>/<nr>-...` for issue `number` (type: letters, e.g. `feat`).
pub fn branch_matches(head_ref: &str, number: u64) -> bool {
    let Some((kind, rest)) = head_ref.split_once('/') else {
        return false;
    };
    !kind.is_empty()
        && kind.chars().all(|c| c.is_ascii_alphabetic())
        && rest.starts_with(&format!("{number}-"))
}

/// Adds the open pull requests whose branch names the issue to `prs` (no duplicates).
pub fn add_branch_matches(issues: &mut [Issue], pulls: &[github_pulls::Pull]) {
    for issue in issues {
        for pull in pulls {
            if branch_matches(&pull.head_ref, issue.number)
                && !issue.prs.iter().any(|p| p.number == pull.number)
            {
                issue.prs.push(LinkedPr {
                    number: pull.number,
                    url: pull.url.clone(),
                    state: "OPEN".into(),
                    checks: pull.checks.clone(),
                });
            }
        }
    }
}

fn read_issues<R: Runner>(
    client: &Client<R>,
    repo: &Repo,
    closed: bool,
) -> Result<(Vec<Issue>, bool), GhError> {
    let query = issues_query(closed);
    let mut all: Vec<Issue> = Vec::new();
    let mut more = false;
    let mut cursor: Option<String> = None;
    while all.len() < MAX_ISSUES {
        let mut vars = json!({ "owner": repo.owner, "name": repo.name });
        if let Some(c) = &cursor {
            vars["cursor"] = json!(c);
        }
        let (page, next) = parse_page(&client.graphql(&query, &vars)?)?;
        all.extend(page);
        more = next.is_some();
        cursor = next;
        if cursor.is_none() {
            break;
        }
    }
    all.truncate(MAX_ISSUES);
    Ok((all, more))
}

fn load_repo<R: Runner>(client: &Client<R>, repo: &Repo, closed: bool) -> Outcome {
    match read_issues(client, repo, closed) {
        Ok((mut issues, truncated)) => {
            // Pull requests for the branch match; failing here only loses those links.
            let vars = json!({ "owner": repo.owner, "name": repo.name });
            if let Ok(answer) = client.graphql(github_pulls::QUERY, &vars)
                && let Ok(pulls) = github_pulls::parse_pulls(&answer)
            {
                add_branch_matches(&mut issues, &pulls);
            }
            Outcome::Ok { issues, truncated }
        }
        Err(error) => Outcome::Error { error },
    }
}

/// The GitHub repositories among `dirs` as (root, repository), once each (case-insensitive).
pub fn unique_repos(dirs: &[String]) -> Vec<(String, Repo)> {
    let mut seen = std::collections::HashSet::new();
    git::repo_roots(dirs)
        .into_iter()
        .filter_map(|root| {
            let repo = repo_of(std::path::Path::new(&root))?;
            seen.insert((repo.owner.to_lowercase(), repo.name.to_lowercase()))
                .then_some((root, repo))
        })
        .collect()
}

/// Issues of the GitHub repositories among `dirs` (worktrees count as their main repository;
/// directories without a GitHub `origin` are left out).
pub fn load<R: Runner + Sync>(
    client: &Client<R>,
    dirs: &[String],
    closed: bool,
) -> Vec<RepoIssues> {
    let repos = unique_repos(dirs);
    std::thread::scope(|scope| {
        let handles: Vec<_> = repos
            .iter()
            .map(|(_, repo)| scope.spawn(|| load_repo(client, repo, closed)))
            .collect();
        repos
            .iter()
            .zip(handles)
            .map(|((root, repo), handle)| RepoIssues {
                root: root.clone(),
                repo: format!("{}/{}", repo.owner, repo.name),
                outcome: handle.join().unwrap_or_else(|_| Outcome::Error {
                    error: GhError::Other {
                        message: "the GitHub reader stopped unexpectedly".into(),
                    },
                }),
            })
            .collect()
    })
}

const DETAIL_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!) { repository(owner: $owner, name: $name) { \
issue(number: $number) { number title url body state stateReason author { login } viewerCanUpdate \
labels(first: 30) { nodes { name } } assignees(first: 20) { nodes { login } } milestone { title } \
parent { number title repository { nameWithOwner } } \
subIssues(first: 50) { nodes { number title state repository { nameWithOwner } } } \
closedByPullRequestsReferences(first: 20, includeClosedPrs: true) { nodes { number title url state \
repository { nameWithOwner } } } } } }";

/// A sub-issue or the parent of an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelatedIssue {
    pub number: u64,
    pub title: String,
    /// `OPEN` or `CLOSED` (empty for a parent, which is not asked for its state).
    pub state: String,
    /// `owner/name`.
    pub repo: String,
}

/// A pull request that closes the issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClosingPr {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub state: String,
    /// `owner/name`.
    pub repo: String,
}

/// An issue in detail (`github_issue_detail`); `viewer_can_update` is for later actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueDetail {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub body: String,
    pub state: String,
    /// `COMPLETED`, `NOT_PLANNED`, `REOPENED` or `DUPLICATE` once closed.
    pub state_reason: Option<String>,
    pub author: Option<String>,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    pub milestone: Option<String>,
    pub parent: Option<RelatedIssue>,
    pub sub_issues: Vec<RelatedIssue>,
    pub pull_requests: Vec<ClosingPr>,
    pub viewer_can_update: bool,
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

fn related(node: &Value) -> Option<RelatedIssue> {
    Some(RelatedIssue {
        number: node["number"].as_u64()?,
        title: str_of(&node["title"]),
        state: str_of(&node["state"]),
        repo: str_of(&node["repository"]["nameWithOwner"]),
    })
}

/// The `repository { issue }` answer of [`DETAIL_QUERY`]; a missing issue is an error.
pub fn parse_detail(answer: &Value) -> Result<IssueDetail, GhError> {
    let node = &answer["data"]["repository"]["issue"];
    let Some(number) = node["number"].as_u64() else {
        let message = answer["errors"][0]["message"]
            .as_str()
            .unwrap_or("unexpected GitHub answer");
        return Err(GhError::Other {
            message: message.to_owned(),
        });
    };
    let list = |v: &Value| v.as_array().cloned().unwrap_or_default();
    Ok(IssueDetail {
        number,
        title: str_of(&node["title"]),
        url: str_of(&node["url"]),
        body: str_of(&node["body"]),
        state: node["state"].as_str().unwrap_or("OPEN").to_owned(),
        state_reason: node["stateReason"].as_str().map(str::to_owned),
        author: node["author"]["login"].as_str().map(str::to_owned),
        labels: names(&node["labels"], "name"),
        assignees: names(&node["assignees"], "login"),
        milestone: node["milestone"]["title"].as_str().map(str::to_owned),
        parent: related(&node["parent"]),
        sub_issues: list(&node["subIssues"]["nodes"])
            .iter()
            .filter_map(related)
            .collect(),
        pull_requests: list(&node["closedByPullRequestsReferences"]["nodes"])
            .iter()
            .filter_map(|p| {
                Some(ClosingPr {
                    number: p["number"].as_u64()?,
                    title: str_of(&p["title"]),
                    url: str_of(&p["url"]),
                    state: p["state"].as_str().unwrap_or("OPEN").to_owned(),
                    repo: str_of(&p["repository"]["nameWithOwner"]),
                })
            })
            .collect(),
        viewer_can_update: node["viewerCanUpdate"].as_bool().unwrap_or(false),
    })
}

/// Detail of issue `number` in the GitHub repository of `root` (one GraphQL query, cached 30 s by
/// the client).
pub fn load_detail<R: Runner>(
    client: &Client<R>,
    root: &str,
    number: u64,
) -> Result<IssueDetail, GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    let vars = json!({ "owner": repo.owner, "name": repo.name, "number": number });
    parse_detail(&client.graphql(DETAIL_QUERY, &vars)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../fixtures/github-issues.json")).unwrap()
    }

    #[test]
    fn parses_issues_with_labels_parent_and_linked_prs() {
        let (issues, next) = parse_page(&fixture()).unwrap();
        assert_eq!(next, None);
        assert_eq!(issues.len(), 3);
        let epic = &issues[0];
        assert_eq!(epic.milestone.as_deref(), Some("1.0.0"));
        assert_eq!((epic.sub_total, epic.sub_completed), (2, 1));
        let pbi = &issues[1];
        assert_eq!(pbi.labels, vec!["pbi", "needs-refinement"]);
        assert_eq!(pbi.assignees, vec!["jkaufmann"]);
        assert_eq!(
            pbi.parent,
            Some(Parent {
                number: 10,
                repo: "o/n".into()
            })
        );
        assert_eq!(pbi.prs.len(), 2);
        assert_eq!(pbi.prs[0].state, "MERGED");
        assert_eq!(pbi.prs[0].checks.state, "passing");
        assert_eq!(pbi.prs[1].checks.state, "none");
    }

    #[test]
    fn a_parent_in_another_repository_keeps_its_repository() {
        let (issues, _) = parse_page(&fixture()).unwrap();
        assert_eq!(issues[2].parent.as_ref().unwrap().repo, "o/other");
        assert_eq!(issues[2].sub_total, 0);
    }

    #[test]
    fn the_next_page_cursor_is_read_only_when_there_is_a_next_page() {
        let page = json!({"data":{"repository":{"issues":{
            "pageInfo":{"hasNextPage":true,"endCursor":"abc"},"nodes":[]}}}});
        assert_eq!(parse_page(&page).unwrap().1.as_deref(), Some("abc"));
    }

    #[test]
    fn a_missing_repository_is_an_error() {
        let missing =
            json!({"data":{"repository":null},"errors":[{"message":"Could not resolve"}]});
        assert_eq!(
            parse_page(&missing).unwrap_err(),
            GhError::Other {
                message: "Could not resolve".into()
            }
        );
    }

    #[test]
    fn branches_match_type_slash_number_dash() {
        assert!(branch_matches("feat/232-github-issues-tree", 232));
        assert!(branch_matches("fix/232-x", 232));
        assert!(!branch_matches("feat/2320-x", 232));
        assert!(!branch_matches("feat/232", 232));
        assert!(!branch_matches("232-x", 232));
        assert!(!branch_matches("a1/232-x", 232));
    }

    #[test]
    fn branch_matches_add_open_pulls_once() {
        let mut issues = parse_page(&fixture()).unwrap().0;
        let pulls_fixture: Value =
            serde_json::from_str(include_str!("../fixtures/github-pulls.json")).unwrap();
        let mut pulls = github_pulls::parse_pulls(&pulls_fixture).unwrap();
        pulls[0].head_ref = "feat/233-other".into();
        pulls[0].number = 500;
        add_branch_matches(&mut issues, &pulls);
        assert_eq!(issues[2].prs.len(), 1);
        assert_eq!(issues[2].prs[0].number, 500);
        assert_eq!(issues[2].prs[0].state, "OPEN");
        add_branch_matches(&mut issues, &pulls);
        assert_eq!(issues[2].prs.len(), 1);
    }

    #[test]
    fn the_query_selects_the_requested_state() {
        assert!(issues_query(false).contains("states: OPEN"));
        assert!(issues_query(true).contains("states: CLOSED"));
    }

    #[test]
    fn the_wire_shape_is_camel_case_with_a_status_tag() {
        let issue = serde_json::to_value(&parse_page(&fixture()).unwrap().0[0]).unwrap();
        assert_eq!(issue["subTotal"], 2);
        let repo = RepoIssues {
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
    }

    fn detail_fixture() -> Value {
        serde_json::from_str(include_str!("../fixtures/github-issue-detail.json")).unwrap()
    }

    #[test]
    fn parses_the_issue_detail_fixture() {
        let d = parse_detail(&detail_fixture()).unwrap();
        assert_eq!(d.number, 337);
        assert_eq!(d.author.as_deref(), Some("jkaufmann"));
        assert!(d.body.starts_with("Selecting an issue"));
        assert_eq!(
            (d.state.as_str(), d.state_reason.as_deref()),
            ("CLOSED", Some("COMPLETED"))
        );
        assert_eq!(d.labels, vec!["pbi", "frontend"]);
        assert_eq!(d.assignees, vec!["jkaufmann"]);
        assert_eq!(d.milestone.as_deref(), Some("M2"));
        let parent = d.parent.unwrap();
        assert_eq!(
            (parent.number, parent.repo.as_str()),
            (300, "GravionLabs/poimen")
        );
        let subs: Vec<_> = d
            .sub_issues
            .iter()
            .map(|s| (s.number, s.state.as_str()))
            .collect();
        assert_eq!(subs, vec![(338, "OPEN"), (339, "CLOSED")]);
        assert_eq!(d.pull_requests.len(), 1);
        assert_eq!(d.pull_requests[0].state, "MERGED");
        assert_eq!(d.pull_requests[0].repo, "GravionLabs/poimen");
        assert!(d.viewer_can_update);
    }

    #[test]
    fn a_bare_issue_parses_with_defaults_and_a_missing_one_is_an_error() {
        let bare = json!({"data":{"repository":{"issue":{"number":5}}}});
        let d = parse_detail(&bare).unwrap();
        assert!(d.parent.is_none() && d.sub_issues.is_empty() && d.pull_requests.is_empty());
        assert_eq!(
            (d.state.as_str(), d.state_reason, d.author),
            ("OPEN", None, None)
        );
        assert!(!d.viewer_can_update);
        let missing = json!({"data":{"repository":{"issue":null}},"errors":[{"message":"Could not resolve to an Issue"}]});
        assert_eq!(
            parse_detail(&missing),
            Err(GhError::Other {
                message: "Could not resolve to an Issue".into()
            })
        );
    }

    #[test]
    fn the_issue_detail_wire_shape_is_camel_case() {
        let v = serde_json::to_value(parse_detail(&detail_fixture()).unwrap()).unwrap();
        assert_eq!(v["stateReason"], "COMPLETED");
        assert_eq!(v["subIssues"][0]["repo"], "GravionLabs/poimen");
        assert_eq!(v["pullRequests"][0]["number"], 540);
        assert_eq!(v["viewerCanUpdate"], true);
    }
}
