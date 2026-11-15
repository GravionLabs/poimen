//! Issues per repository for the GitHub view (ADR 0005, #232): paginated GraphQL through the shared
//! `github::Client` with native parent links (sub-issues) and the pull requests linked to each issue
//! (closing references plus open pull requests whose branch is `<type>/<nr>-...`). The tree and the
//! levels are built by the frontend. Read-only.

use serde::{Deserialize, Serialize};
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
blockedBy(first: 10) {{ nodes {{ number state }} }} \
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
    /// Numbers of the open issues that block this one (native dependencies, #585).
    pub blocked_by: Vec<u64>,
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
        blocked_by: node["blockedBy"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|b| b["state"].as_str() != Some("CLOSED"))
            .filter_map(|b| b["number"].as_u64())
            .collect(),
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
repository { nameWithOwner } } } } assignableUsers(first: 100) { nodes { login } } } }";

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
    /// Logins the chip picker offers as assignees.
    pub assignable_users: Vec<String>,
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
        assignable_users: names(&answer["data"]["repository"]["assignableUsers"], "login"),
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

/// What `github_issue_edit` changes. `body` is the new description; `expected_body` is the one the
/// pane loaded, which a save checks against GitHub unless `overwrite`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct IssueEdit {
    pub title: Option<String>,
    pub body: Option<String>,
    pub expected_body: Option<String>,
    pub overwrite: bool,
    pub add_labels: Vec<String>,
    pub remove_labels: Vec<String>,
    pub add_assignees: Vec<String>,
    pub remove_assignees: Vec<String>,
    /// Title of the milestone to set.
    pub milestone: Option<String>,
    pub remove_milestone: bool,
}

/// Answer of `github_issue_edit`: written, or the body changed on GitHub since the pane loaded it
/// (nothing was written; `current` is the body on GitHub now).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum EditOutcome {
    Saved,
    Conflict { current: String },
}

fn other(message: &str) -> GhError {
    GhError::Other {
        message: message.to_owned(),
    }
}

/// `gh issue edit` argv: the body is `--body-file -` (the text goes on stdin, never in argv), one
/// flag per label or assignee; a blank title, a value that is blank or has a comma, and nothing to
/// change are refused.
pub fn edit_argv(repo: &Repo, number: u64, edit: &IssueEdit) -> Result<Vec<String>, GhError> {
    let mut argv: Vec<String> = vec!["issue".into(), "edit".into(), number.to_string()];
    let mut changes = 0;
    if let Some(title) = &edit.title {
        let title = title.trim();
        if title.is_empty() || title.contains('\n') {
            return Err(other("the title must be one non-blank line"));
        }
        argv.extend(["--title".into(), title.to_owned()]);
        changes += 1;
    }
    if edit.body.is_some() {
        argv.extend(["--body-file".into(), "-".into()]);
        changes += 1;
    }
    let groups = [
        ("--add-label", &edit.add_labels),
        ("--remove-label", &edit.remove_labels),
        ("--add-assignee", &edit.add_assignees),
        ("--remove-assignee", &edit.remove_assignees),
    ];
    for (flag, values) in groups {
        for value in values {
            let value = value.trim();
            if value.is_empty() || value.contains(',') {
                return Err(other(&format!("invalid value for {flag}: {value:?}")));
            }
            argv.extend([flag.to_owned(), value.to_owned()]);
            changes += 1;
        }
    }
    match (&edit.milestone, edit.remove_milestone) {
        (Some(_), true) => return Err(other("set or remove the milestone, not both")),
        (Some(title), false) => {
            let title = title.trim();
            if title.is_empty() {
                return Err(other("the milestone must not be blank"));
            }
            argv.extend(["--milestone".into(), title.to_owned()]);
            changes += 1;
        }
        (None, true) => {
            argv.push("--remove-milestone".into());
            changes += 1;
        }
        (None, false) => {}
    }
    if changes == 0 {
        return Err(other("nothing to change"));
    }
    argv.extend(["-R".into(), format!("{}/{}", repo.owner, repo.name)]);
    Ok(argv)
}

/// The body GitHub holds now (`gh issue view --json body`, never cached).
fn current_body<R: Runner>(
    client: &Client<R>,
    repo: &Repo,
    number: u64,
) -> Result<String, GhError> {
    let argv: Vec<String> = [
        "issue",
        "view",
        &number.to_string(),
        "--json",
        "body",
        "-R",
        &format!("{}/{}", repo.owner, repo.name),
    ]
    .map(String::from)
    .to_vec();
    let v: Value = serde_json::from_str(&client.read_fresh(&argv)?)
        .map_err(|_| other("unexpected GitHub answer"))?;
    Ok(v["body"].as_str().unwrap_or_default().to_owned())
}

/// Line endings are not a change: GitHub stores what the web form sent.
fn same_body(a: &str, b: &str) -> bool {
    a.replace("\r\n", "\n") == b.replace("\r\n", "\n")
}

/// Edits issue `number` in the repository of `root`. A new body is written only when the body on
/// GitHub still equals `expected_body` (read right before), or `overwrite` is set; else nothing is
/// written and the answer is `Conflict`.
pub fn run_edit<R: Runner>(
    client: &Client<R>,
    root: &str,
    number: u64,
    edit: &IssueEdit,
) -> Result<EditOutcome, GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    edit_issue(client, &repo, number, edit)
}

fn edit_issue<R: Runner>(
    client: &Client<R>,
    repo: &Repo,
    number: u64,
    edit: &IssueEdit,
) -> Result<EditOutcome, GhError> {
    let argv = edit_argv(repo, number, edit)?;
    if let Some(body) = &edit.body {
        if !edit.overwrite {
            let expected = edit
                .expected_body
                .as_deref()
                .ok_or_else(|| other("the body the pane loaded is missing"))?;
            let current = current_body(client, repo, number)?;
            if !same_body(&current, expected) {
                return Ok(EditOutcome::Conflict { current });
            }
        }
        client.mutate_input(repo, &argv, body)?;
    } else {
        client.mutate(repo, &argv)?;
    }
    Ok(EditOutcome::Saved)
}

/// A state change of an issue (`github_issue_action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueAction {
    Completed,
    NotPlanned,
    Reopen,
}

/// `gh issue close --reason` or `gh issue reopen` argv.
pub fn action_argv(repo: &Repo, number: u64, action: IssueAction) -> Vec<String> {
    let mut argv: Vec<String> = vec!["issue".into()];
    match action {
        IssueAction::Completed => argv.extend([
            "close".into(),
            number.to_string(),
            "--reason".into(),
            "completed".into(),
        ]),
        IssueAction::NotPlanned => argv.extend([
            "close".into(),
            number.to_string(),
            "--reason".into(),
            "not planned".into(),
        ]),
        IssueAction::Reopen => argv.extend(["reopen".into(), number.to_string()]),
    }
    argv.extend(["-R".into(), format!("{}/{}", repo.owner, repo.name)]);
    argv
}

pub fn run_action<R: Runner>(
    client: &Client<R>,
    root: &str,
    number: u64,
    action: IssueAction,
) -> Result<(), GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    client
        .mutate(&repo, &action_argv(&repo, number, action))
        .map(|_| ())
}

/// A new issue (`github_issue_create`); `parent` links it as a sub-issue of that issue.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewIssue {
    pub title: String,
    pub body: String,
    pub labels: Vec<String>,
    /// Title of the milestone.
    pub milestone: Option<String>,
    pub parent: Option<u64>,
}

/// The issue `github_issue_create` made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CreatedIssue {
    pub number: u64,
    pub url: String,
}

fn slug(repo: &Repo) -> String {
    format!("{}/{}", repo.owner, repo.name)
}

/// `gh issue create` argv: the body is `--body-file -` (stdin), one `--label` per label; a blank or
/// multi-line title and a blank value or one with a comma are refused.
pub fn create_argv(repo: &Repo, new: &NewIssue) -> Result<Vec<String>, GhError> {
    let title = new.title.trim();
    if title.is_empty() || title.contains('\n') {
        return Err(other("the title must be one non-blank line"));
    }
    let mut argv: Vec<String> = ["issue", "create", "--title", title, "--body-file", "-"]
        .map(String::from)
        .to_vec();
    for label in &new.labels {
        let label = label.trim();
        if label.is_empty() || label.contains(',') {
            return Err(other(&format!("invalid label: {label:?}")));
        }
        argv.extend(["--label".into(), label.to_owned()]);
    }
    if let Some(m) = &new.milestone {
        let m = m.trim();
        if m.is_empty() {
            return Err(other("the milestone must not be blank"));
        }
        argv.extend(["--milestone".into(), m.to_owned()]);
    }
    argv.extend(["-R".into(), slug(repo)]);
    Ok(argv)
}

/// The last non-blank line of what `gh issue create` prints (the issue URL).
fn printed_url(output: &str) -> &str {
    output
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim()
}

/// The issue number at the end of the URL `gh issue create` prints.
pub fn number_of_url(output: &str) -> Option<u64> {
    printed_url(output).rsplit('/').next()?.parse().ok()
}

/// The REST id of issue `number` (the sub-issue endpoints take it, not the number).
fn issue_id<R: Runner>(client: &Client<R>, repo: &Repo, number: u64) -> Result<u64, GhError> {
    let argv: Vec<String> = [
        "api",
        &format!("repos/{}/issues/{number}", slug(repo)),
        "--jq",
        ".id",
    ]
    .map(String::from)
    .to_vec();
    client
        .read_fresh(&argv)?
        .trim()
        .parse()
        .map_err(|_| other("unexpected GitHub answer"))
}

/// REST call on the sub-issues of `parent`: `POST .../sub_issues` or `DELETE .../sub_issue`.
pub fn sub_issue_argv(repo: &Repo, parent: u64, sub_id: u64, add: bool) -> Vec<String> {
    let (method, path) = if add {
        ("POST", "sub_issues")
    } else {
        ("DELETE", "sub_issue")
    };
    vec![
        "api".into(),
        "-X".into(),
        method.into(),
        format!("repos/{}/issues/{parent}/{path}", slug(repo)),
        "-F".into(),
        format!("sub_issue_id={sub_id}"),
    ]
}

/// Creates an issue in the repository of `root` and, with `parent`, links it as its sub-issue.
pub fn run_create<R: Runner>(
    client: &Client<R>,
    root: &str,
    new: &NewIssue,
) -> Result<CreatedIssue, GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    create_issue(client, &repo, new)
}

fn create_issue<R: Runner>(
    client: &Client<R>,
    repo: &Repo,
    new: &NewIssue,
) -> Result<CreatedIssue, GhError> {
    let argv = create_argv(repo, new)?;
    let out = client.mutate_input(repo, &argv, &new.body)?;
    let number = number_of_url(&out).ok_or_else(|| other("unexpected gh answer"))?;
    if let Some(parent) = new.parent {
        let linked = issue_id(client, repo, number)
            .and_then(|id| client.mutate(repo, &sub_issue_argv(repo, parent, id, true)));
        if let Err(e) = linked {
            let why = match e {
                GhError::Other { message } => message,
                e => format!("{e:?}"),
            };
            return Err(other(&format!(
                "created #{number} but could not link it to #{parent}: {why}"
            )));
        }
    }
    Ok(CreatedIssue {
        number,
        url: printed_url(&out).to_owned(),
    })
}

/// Moves issue `number` under `new_parent`: removed from `old_parent` first when it has one.
pub fn run_change_parent<R: Runner>(
    client: &Client<R>,
    root: &str,
    number: u64,
    old_parent: Option<u64>,
    new_parent: u64,
) -> Result<(), GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    change_parent(client, &repo, number, old_parent, new_parent)
}

fn change_parent<R: Runner>(
    client: &Client<R>,
    repo: &Repo,
    number: u64,
    old_parent: Option<u64>,
    new_parent: u64,
) -> Result<(), GhError> {
    if new_parent == number || old_parent == Some(new_parent) {
        return Err(other("pick another issue as the parent"));
    }
    let id = issue_id(client, repo, number)?;
    if let Some(old) = old_parent {
        client.mutate(repo, &sub_issue_argv(repo, old, id, false))?;
    }
    client.mutate(repo, &sub_issue_argv(repo, new_parent, id, true))?;
    Ok(())
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
    fn open_blockers_are_listed_and_closed_ones_are_not() {
        let (issues, _) = parse_page(&fixture()).unwrap();
        assert_eq!(issues[1].blocked_by, vec![231]);
        assert!(issues[0].blocked_by.is_empty());
        assert!(issues[2].blocked_by.is_empty());
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
        assert_eq!(d.assignable_users, vec!["jkaufmann", "octo"]);
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

    fn repo() -> Repo {
        Repo {
            owner: "o".into(),
            name: "n".into(),
        }
    }

    fn argv(edit: &IssueEdit) -> Vec<String> {
        edit_argv(&repo(), 7, edit).unwrap()
    }

    #[test]
    fn edit_argv_has_one_flag_per_value_and_the_body_by_file() {
        let edit = IssueEdit {
            title: Some("  New title ".into()),
            body: Some("x".repeat(500_000)),
            add_labels: vec!["bug".into(), "good first issue".into()],
            remove_labels: vec!["pbi".into()],
            add_assignees: vec!["octo".into()],
            remove_assignees: vec!["jk".into()],
            milestone: Some("M3".into()),
            ..IssueEdit::default()
        };
        let a = argv(&edit);
        assert_eq!(
            a,
            [
                "issue",
                "edit",
                "7",
                "--title",
                "New title",
                "--body-file",
                "-",
                "--add-label",
                "bug",
                "--add-label",
                "good first issue",
                "--remove-label",
                "pbi",
                "--add-assignee",
                "octo",
                "--remove-assignee",
                "jk",
                "--milestone",
                "M3",
                "-R",
                "o/n"
            ]
        );
        assert!(
            a.iter().all(|x| x.len() < 100),
            "the body never goes in argv"
        );
    }

    #[test]
    fn edit_argv_removes_a_milestone_and_refuses_bad_input() {
        let rm = IssueEdit {
            remove_milestone: true,
            ..IssueEdit::default()
        };
        assert_eq!(
            argv(&rm),
            ["issue", "edit", "7", "--remove-milestone", "-R", "o/n"]
        );
        let bad = |e: IssueEdit| edit_argv(&repo(), 7, &e).is_err();
        assert!(bad(IssueEdit::default()));
        assert!(bad(IssueEdit {
            title: Some(" ".into()),
            ..IssueEdit::default()
        }));
        assert!(bad(IssueEdit {
            title: Some("a\nb".into()),
            ..IssueEdit::default()
        }));
        assert!(bad(IssueEdit {
            add_labels: vec!["a,b".into()],
            ..IssueEdit::default()
        }));
        assert!(bad(IssueEdit {
            add_assignees: vec![" ".into()],
            ..IssueEdit::default()
        }));
        assert!(bad(IssueEdit {
            milestone: Some("M1".into()),
            remove_milestone: true,
            ..IssueEdit::default()
        }));
        assert!(bad(IssueEdit {
            milestone: Some(" ".into()),
            ..IssueEdit::default()
        }));
        // An empty body is a valid edit (clears the description).
        assert!(!bad(IssueEdit {
            body: Some(String::new()),
            ..IssueEdit::default()
        }));
    }

    #[test]
    fn action_argv_per_action() {
        let a = |x| action_argv(&repo(), 7, x);
        assert_eq!(
            a(IssueAction::Completed),
            ["issue", "close", "7", "--reason", "completed", "-R", "o/n"]
        );
        assert_eq!(
            a(IssueAction::NotPlanned),
            [
                "issue",
                "close",
                "7",
                "--reason",
                "not planned",
                "-R",
                "o/n"
            ]
        );
        assert_eq!(
            a(IssueAction::Reopen),
            ["issue", "reopen", "7", "-R", "o/n"]
        );
    }

    #[test]
    fn line_endings_alone_are_not_a_conflict() {
        assert!(same_body("a\r\nb", "a\nb"));
        assert!(!same_body("a\nb", "a\nc"));
    }

    use crate::github::Output;
    use std::cell::RefCell;
    use std::io;

    /// Answers `issue view` with `current` and records every call and stdin.
    struct Gh {
        current: String,
        calls: RefCell<Vec<Vec<String>>>,
        inputs: RefCell<Vec<String>>,
    }

    fn gh(current: &str) -> Gh {
        Gh {
            current: current.into(),
            calls: RefCell::new(vec![]),
            inputs: RefCell::new(vec![]),
        }
    }

    impl Runner for &Gh {
        fn gh(&self, args: &[String]) -> io::Result<Output> {
            self.calls.borrow_mut().push(args.to_vec());
            let stdout = if args.get(1).is_some_and(|a| a == "view") {
                json!({ "body": self.current }).to_string()
            } else {
                String::new()
            };
            Ok(Output {
                code: Some(0),
                stdout,
                stderr: String::new(),
            })
        }
        fn gh_input(&self, args: &[String], input: &str) -> io::Result<Output> {
            self.inputs.borrow_mut().push(input.into());
            self.gh(args)
        }
        fn now(&self) -> std::time::Duration {
            std::time::Duration::from_secs(1)
        }
    }

    fn client(g: &Gh) -> Client<&Gh> {
        Client::new(g, std::env::temp_dir().join("poimen-issues-edit-test"))
    }

    fn body_edit(expected: Option<&str>, overwrite: bool) -> IssueEdit {
        IssueEdit {
            body: Some("mine\n".into()),
            expected_body: expected.map(String::from),
            overwrite,
            ..IssueEdit::default()
        }
    }

    #[test]
    fn a_save_with_an_unchanged_body_writes_it_on_stdin() {
        let g = gh("loaded");
        let out = edit_issue(&client(&g), &repo(), 7, &body_edit(Some("loaded"), false));
        assert_eq!(out, Ok(EditOutcome::Saved));
        assert_eq!(g.inputs.borrow().as_slice(), ["mine\n"]);
        let calls = g.calls.borrow();
        assert_eq!(calls[0][..3], ["issue", "view", "7"]);
        assert_eq!(
            calls[1],
            ["issue", "edit", "7", "--body-file", "-", "-R", "o/n"]
        );
    }

    #[test]
    fn a_body_changed_on_github_is_a_conflict_and_nothing_is_written() {
        let g = gh("changed elsewhere");
        let out = edit_issue(&client(&g), &repo(), 7, &body_edit(Some("loaded"), false));
        assert_eq!(
            out,
            Ok(EditOutcome::Conflict {
                current: "changed elsewhere".into()
            })
        );
        assert!(g.inputs.borrow().is_empty());
        assert_eq!(g.calls.borrow().len(), 1, "only the read ran");
    }

    #[test]
    fn overwrite_skips_the_check_and_a_missing_baseline_is_refused() {
        let g = gh("changed elsewhere");
        let out = edit_issue(&client(&g), &repo(), 7, &body_edit(Some("loaded"), true));
        assert_eq!(out, Ok(EditOutcome::Saved));
        assert_eq!(g.calls.borrow().len(), 1, "no read before the write");
        let g = gh("x");
        assert!(edit_issue(&client(&g), &repo(), 7, &body_edit(None, false)).is_err());
        assert!(g.calls.borrow().is_empty());
    }

    #[test]
    fn an_edit_without_a_body_does_not_read_or_use_stdin() {
        let g = gh("x");
        let edit = IssueEdit {
            add_labels: vec!["bug".into()],
            ..IssueEdit::default()
        };
        assert_eq!(
            edit_issue(&client(&g), &repo(), 7, &edit),
            Ok(EditOutcome::Saved)
        );
        assert!(g.inputs.borrow().is_empty());
        assert_eq!(g.calls.borrow().len(), 1);
    }

    #[test]
    fn crlf_in_the_stored_body_is_not_a_conflict() {
        let g = gh("a\r\nb");
        let out = edit_issue(&client(&g), &repo(), 7, &body_edit(Some("a\nb"), false));
        assert_eq!(out, Ok(EditOutcome::Saved));
    }

    /// Answers `issue create` with a URL, `api .../issues/<n> --jq .id` with `1000 + n`.
    struct Rest {
        calls: RefCell<Vec<Vec<String>>>,
        inputs: RefCell<Vec<String>>,
        fail_post: bool,
    }

    impl Runner for &Rest {
        fn gh(&self, args: &[String]) -> io::Result<Output> {
            self.calls.borrow_mut().push(args.to_vec());
            let mut stdout = String::new();
            let mut code = Some(0);
            if args[0] == "issue" {
                stdout = "https://github.com/o/n/issues/42\n".into();
            } else if args.contains(&"--jq".to_owned()) {
                let n: u64 = args[1].rsplit('/').next().unwrap().parse().unwrap();
                stdout = format!("{}\n", 1000 + n);
            } else if self.fail_post && args.contains(&"POST".to_owned()) {
                code = Some(1);
            }
            Ok(Output {
                code,
                stdout,
                stderr: if code == Some(1) {
                    "boom".into()
                } else {
                    String::new()
                },
            })
        }
        fn gh_input(&self, args: &[String], input: &str) -> io::Result<Output> {
            self.inputs.borrow_mut().push(input.into());
            self.gh(args)
        }
        fn now(&self) -> std::time::Duration {
            std::time::Duration::from_secs(1)
        }
    }

    fn rest(fail_post: bool) -> Rest {
        Rest {
            calls: RefCell::new(vec![]),
            inputs: RefCell::new(vec![]),
            fail_post,
        }
    }

    fn rclient(r: &Rest) -> Client<&Rest> {
        Client::new(r, std::env::temp_dir().join("poimen-issues-create-test"))
    }

    fn new_issue() -> NewIssue {
        NewIssue {
            title: " Add thing ".into(),
            body: "text\n".into(),
            labels: vec!["pbi".into(), "ui".into()],
            milestone: Some("M3".into()),
            parent: None,
        }
    }

    #[test]
    fn create_argv_has_body_on_stdin_labels_and_milestone() {
        assert_eq!(
            create_argv(&repo(), &new_issue()).unwrap(),
            [
                "issue",
                "create",
                "--title",
                "Add thing",
                "--body-file",
                "-",
                "--label",
                "pbi",
                "--label",
                "ui",
                "--milestone",
                "M3",
                "-R",
                "o/n"
            ]
        );
    }

    #[test]
    fn create_argv_refuses_bad_input() {
        for bad in [
            NewIssue {
                title: "  ".into(),
                ..new_issue()
            },
            NewIssue {
                title: "a\nb".into(),
                ..new_issue()
            },
            NewIssue {
                labels: vec!["a,b".into()],
                ..new_issue()
            },
            NewIssue {
                milestone: Some(" ".into()),
                ..new_issue()
            },
        ] {
            assert!(create_argv(&repo(), &bad).is_err());
        }
    }

    #[test]
    fn the_number_comes_from_the_printed_url() {
        assert_eq!(
            number_of_url("\nhttps://github.com/o/n/issues/42\n"),
            Some(42)
        );
        assert_eq!(number_of_url("nope"), None);
        assert_eq!(number_of_url(""), None);
    }

    #[test]
    fn sub_issue_calls_are_post_and_delete_with_the_id() {
        assert_eq!(
            sub_issue_argv(&repo(), 5, 1007, true),
            [
                "api",
                "-X",
                "POST",
                "repos/o/n/issues/5/sub_issues",
                "-F",
                "sub_issue_id=1007"
            ]
        );
        assert_eq!(
            sub_issue_argv(&repo(), 5, 1007, false),
            [
                "api",
                "-X",
                "DELETE",
                "repos/o/n/issues/5/sub_issue",
                "-F",
                "sub_issue_id=1007"
            ]
        );
    }

    #[test]
    fn creating_a_sub_issue_posts_the_new_issues_id_to_the_parent() {
        let r = rest(false);
        let new = NewIssue {
            parent: Some(5),
            ..new_issue()
        };
        let made = create_issue(&rclient(&r), &repo(), &new).unwrap();
        assert_eq!(made.number, 42);
        assert_eq!(made.url, "https://github.com/o/n/issues/42");
        assert_eq!(r.inputs.borrow().as_slice(), ["text\n"]);
        let calls = r.calls.borrow();
        assert_eq!(calls[1][..2], ["api", "repos/o/n/issues/42"]);
        assert_eq!(
            calls[2],
            [
                "api",
                "-X",
                "POST",
                "repos/o/n/issues/5/sub_issues",
                "-F",
                "sub_issue_id=1042"
            ]
        );
    }

    #[test]
    fn a_top_level_issue_makes_no_rest_call_and_a_link_failure_names_the_issue() {
        let r = rest(false);
        create_issue(&rclient(&r), &repo(), &new_issue()).unwrap();
        assert_eq!(r.calls.borrow().len(), 1);
        let r = rest(true);
        let new = NewIssue {
            parent: Some(5),
            ..new_issue()
        };
        let err = create_issue(&rclient(&r), &repo(), &new).unwrap_err();
        assert!(format!("{err:?}").contains("created #42"));
    }

    #[test]
    fn changing_the_parent_deletes_from_the_old_then_posts_to_the_new() {
        let r = rest(false);
        change_parent(&rclient(&r), &repo(), 9, Some(2), 3).unwrap();
        {
            let calls = r.calls.borrow();
            assert_eq!(calls[1][2..4], ["DELETE", "repos/o/n/issues/2/sub_issue"]);
            assert_eq!(calls[2][2..4], ["POST", "repos/o/n/issues/3/sub_issues"]);
            assert_eq!(calls[2][5], "sub_issue_id=1009");
        }
        let r = rest(false);
        change_parent(&rclient(&r), &repo(), 9, None, 3).unwrap();
        assert_eq!(r.calls.borrow().len(), 2, "no delete without an old parent");
    }

    #[test]
    fn changing_to_itself_or_the_same_parent_is_refused() {
        let r = rest(false);
        assert!(change_parent(&rclient(&r), &repo(), 9, Some(2), 9).is_err());
        assert!(change_parent(&rclient(&r), &repo(), 9, Some(2), 2).is_err());
        assert!(r.calls.borrow().is_empty());
    }
}
