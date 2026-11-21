//! Open pull requests per repository for the GitHub view (ADR 0005, #231): one GraphQL query per
//! repository through the shared `github::Client`, plus the viewer's login once. Read-only.

use serde::Serialize;
use serde_json::{Value, json};

use crate::git;
use crate::github::{Client, GhError, Repo, Runner, failed_logs, failed_runs, repo_of};

pub(crate) const QUERY: &str = "query($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { \
pullRequests(states: OPEN, first: 50, orderBy: {field: CREATED_AT, direction: DESC}) { nodes { \
number title url author { login } isDraft additions deletions createdAt headRefName isCrossRepository baseRefName mergeable \
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
    /// `MERGEABLE`, `CONFLICTING` or `UNKNOWN`.
    pub mergeable: String,
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
        mergeable: node["mergeable"].as_str().unwrap_or("UNKNOWN").to_owned(),
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
mergeable mergeStateStatus viewerCanUpdate viewerDidAuthor viewerCanMergeAsAdmin headRefOid \
reviewRequests(first: 20) { nodes { requestedReviewer { __typename ... on User { login } } } } \
labels(first: 50) { nodes { name } } assignees(first: 20) { nodes { login } } \
commits(last: 1) { nodes { commit { statusCheckRollup { contexts(first: 100) { nodes { __typename \
... on CheckRun { name status conclusion detailsUrl } ... on StatusContext { context state targetUrl } } } } } } } \
reviews(last: 50) { nodes { author { login } state submittedAt } } \
closingIssuesReferences(first: 20) { nodes { number title url state repository { nameWithOwner } } } } \
squashMergeAllowed mergeCommitAllowed rebaseMergeAllowed \
assignableUsers(first: 100) { nodes { login } } } }";

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
    /// Commit the checks and runs belong to.
    pub head_sha: String,
    /// The merge methods the repository allows, of `squash`, `merge`, `rebase`.
    pub merge_methods: Vec<String>,
    /// Why the pull request cannot be merged now; `None` when it can.
    pub merge_block: Option<String>,
    /// Logins with a pending review request.
    pub reviewers: Vec<String>,
    pub labels: Vec<String>,
    pub assignees: Vec<String>,
    /// People that can be asked to review or be assigned.
    pub assignable_users: Vec<String>,
}

/// Why a pull request in `state` with `merge_state_status` cannot be merged, or `None` when it can
/// (`CLEAN`, `UNSTABLE` or `HAS_HOOKS`).
pub fn merge_block(state: &str, is_draft: bool, merge_state_status: &str) -> Option<String> {
    if state != "OPEN" {
        return Some("The pull request is not open.".into());
    }
    if is_draft {
        return Some("The pull request is a draft.".into());
    }
    let reason = match merge_state_status {
        "CLEAN" | "UNSTABLE" | "HAS_HOOKS" => return None,
        "BLOCKED" => "Required reviews or checks are missing.",
        "BEHIND" => "The branch is behind its base branch.",
        "DIRTY" => "The branch has merge conflicts.",
        "DRAFT" => "The pull request is a draft.",
        _ => "GitHub has not decided yet whether it can be merged.",
    };
    Some(reason.into())
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
    let repo = &answer["data"]["repository"];
    let logins = |v: &Value, key: &str| -> Vec<String> {
        list(&v["nodes"])
            .iter()
            .filter_map(|n| opt_text(&n[key]))
            .collect()
    };
    let merge_methods = [
        ("squash", "squashMergeAllowed"),
        ("merge", "mergeCommitAllowed"),
        ("rebase", "rebaseMergeAllowed"),
    ]
    .iter()
    .filter(|(_, k)| repo[*k].as_bool().unwrap_or(false))
    .map(|(m, _)| (*m).to_owned())
    .collect();
    let reviewers = list(&node["reviewRequests"]["nodes"])
        .iter()
        .filter_map(|n| opt_text(&n["requestedReviewer"]["login"]))
        .collect();
    let state = text(&node["state"]);
    let merge_state_status = node["mergeStateStatus"]
        .as_str()
        .unwrap_or("UNKNOWN")
        .to_owned();
    Ok(PullDetail {
        head_sha: text(&node["headRefOid"]),
        merge_methods,
        merge_block: merge_block(&state, flag("isDraft"), &merge_state_status),
        reviewers,
        labels: logins(&node["labels"], "name"),
        assignees: logins(&node["assignees"], "login"),
        assignable_users: logins(&repo["assignableUsers"], "login"),
        number,
        title: text(&node["title"]),
        url: text(&node["url"]),
        body: text(&node["body"]),
        state,
        is_draft: flag("isDraft"),
        author: opt_text(&node["author"]["login"]),
        head_ref: text(&node["headRefName"]),
        base_ref: text(&node["baseRefName"]),
        mergeable: node["mergeable"].as_str().unwrap_or("UNKNOWN").to_owned(),
        merge_state_status,
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

/// A write on a pull request (`github_pr_action`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrAction {
    Approve,
    RequestChanges,
    Comment,
    Ready,
    Draft,
    Close,
    Reopen,
}

/// The `gh` argv (without the program name) of `action`; argv only, never a shell. Request changes
/// and comment need a non-blank `body`; approve takes one when given.
pub fn action_argv(
    repo: &Repo,
    number: u64,
    action: PrAction,
    body: Option<&str>,
) -> Result<Vec<String>, GhError> {
    let body = body.map(str::trim).filter(|b| !b.is_empty());
    let mut argv: Vec<String> = vec!["pr".into()];
    let flag = |name: &str| name.to_owned();
    match action {
        PrAction::Approve | PrAction::RequestChanges | PrAction::Comment => {
            argv.extend(["review".into(), number.to_string()]);
            argv.push(flag(match action {
                PrAction::Approve => "--approve",
                PrAction::RequestChanges => "--request-changes",
                _ => "--comment",
            }));
            if action != PrAction::Approve && body.is_none() {
                return Err(GhError::Other {
                    message: "a comment text is required".into(),
                });
            }
            if let Some(body) = body {
                argv.extend(["-b".into(), body.to_owned()]);
            }
        }
        PrAction::Ready | PrAction::Draft => {
            argv.extend(["ready".into(), number.to_string()]);
            if action == PrAction::Draft {
                argv.push("--undo".into());
            }
        }
        PrAction::Close | PrAction::Reopen => {
            let verb = if action == PrAction::Close {
                "close"
            } else {
                "reopen"
            };
            argv.extend([verb.into(), number.to_string()]);
        }
    }
    argv.extend(["-R".into(), format!("{}/{}", repo.owner, repo.name)]);
    Ok(argv)
}

/// Runs `action` on pull request `number` in the repository of `root` through `Client::mutate`.
pub fn run_action<R: Runner>(
    client: &Client<R>,
    root: &str,
    number: u64,
    action: PrAction,
    body: Option<&str>,
) -> Result<(), GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    let argv = action_argv(&repo, number, action, body)?;
    client.mutate(&repo, &argv).map(|_| ())
}

/// How to merge (`github_pr_merge`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeMethod {
    Squash,
    Merge,
    Rebase,
}

impl MergeMethod {
    fn name(self) -> &'static str {
        match self {
            Self::Squash => "squash",
            Self::Merge => "merge",
            Self::Rebase => "rebase",
        }
    }
}

/// `gh pr merge` argv; argv only.
pub fn merge_argv(
    repo: &Repo,
    number: u64,
    method: MergeMethod,
    delete_branch: bool,
) -> Vec<String> {
    let mut argv: Vec<String> = vec![
        "pr".into(),
        "merge".into(),
        number.to_string(),
        format!("--{}", method.name()),
    ];
    if delete_branch {
        argv.push("--delete-branch".into());
    }
    argv.extend(["-R".into(), format!("{}/{}", repo.owner, repo.name)]);
    argv
}

/// Merges pull request `number` after checking against a fresh read that the repository allows
/// `method` and the merge state permits it.
pub fn run_merge<R: Runner>(
    client: &Client<R>,
    root: &str,
    number: u64,
    method: MergeMethod,
    delete_branch: bool,
) -> Result<(), GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    let detail = load_detail(client, root, number)?;
    let refuse = |message: String| Err(GhError::Other { message });
    if let Some(reason) = &detail.merge_block {
        return refuse(reason.clone());
    }
    if !detail.merge_methods.iter().any(|m| m == method.name()) {
        return refuse(format!(
            "The repository does not allow {} merges.",
            method.name()
        ));
    }
    client
        .mutate(&repo, &merge_argv(&repo, number, method, delete_branch))
        .map(|_| ())
}

/// `gh run rerun <id> --failed` argv.
pub fn rerun_argv(repo: &Repo, run_id: u64) -> Vec<String> {
    vec![
        "run".into(),
        "rerun".into(),
        run_id.to_string(),
        "--failed".into(),
        "-R".into(),
        format!("{}/{}", repo.owner, repo.name),
    ]
}

/// Re-runs the failed jobs of every failed run of the head commit; returns how many runs. All
/// are tried; the first error is returned afterwards.
pub fn run_rerun<R: Runner>(client: &Client<R>, root: &str, number: u64) -> Result<usize, GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    let detail = load_detail(client, root, number)?;
    let runs = failed_runs(client, &repo, &detail.head_sha)?;
    if runs.is_empty() {
        return Err(GhError::Other {
            message: "no failed run found for the last commit".into(),
        });
    }
    let mut first_error = None;
    for id in &runs {
        if let Err(e) = client.mutate(&repo, &rerun_argv(&repo, *id)) {
            first_error.get_or_insert(e);
        }
    }
    first_error.map_or(Ok(runs.len()), Err)
}

/// Collects the failed logs of the head commit of pull request `number` and writes them to
/// `<worktree>/.agents/state/ci-failed-<nr>.log`; returns the path (`{log}` of the `fix-checks`
/// action). The file carries the log, never the command line.
pub fn run_ci_log<R: Runner>(
    client: &Client<R>,
    root: &str,
    number: u64,
    worktree: &str,
    home: Option<&std::path::Path>,
) -> Result<String, GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    let detail = load_detail(client, root, number)?;
    let log = failed_logs(client, &repo, &detail.head_sha)?;
    crate::changes::write_ci_log(worktree, number, &log, home)
        .map_err(|message| GhError::Other { message })
}

/// People, labels and assignees to add and remove (`github_pr_edit`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PrEdit {
    pub add_reviewers: Vec<String>,
    pub remove_reviewers: Vec<String>,
    pub add_labels: Vec<String>,
    pub remove_labels: Vec<String>,
    pub add_assignees: Vec<String>,
    pub remove_assignees: Vec<String>,
}

/// `gh pr edit` argv: one flag per value, so a name never gets split; a value that is blank or has
/// a comma is refused (`gh` would read it as a list); nothing to change is an error.
pub fn edit_argv(repo: &Repo, number: u64, edit: &PrEdit) -> Result<Vec<String>, GhError> {
    let mut argv: Vec<String> = vec!["pr".into(), "edit".into(), number.to_string()];
    let groups = [
        ("--add-reviewer", &edit.add_reviewers),
        ("--remove-reviewer", &edit.remove_reviewers),
        ("--add-label", &edit.add_labels),
        ("--remove-label", &edit.remove_labels),
        ("--add-assignee", &edit.add_assignees),
        ("--remove-assignee", &edit.remove_assignees),
    ];
    let mut changes = 0;
    for (flag, values) in groups {
        for value in values {
            let value = value.trim();
            if value.is_empty() || value.contains(',') {
                return Err(GhError::Other {
                    message: format!("invalid value for {flag}: {value:?}"),
                });
            }
            argv.extend([flag.to_owned(), value.to_owned()]);
            changes += 1;
        }
    }
    if changes == 0 {
        return Err(GhError::Other {
            message: "nothing to change".into(),
        });
    }
    argv.extend(["-R".into(), format!("{}/{}", repo.owner, repo.name)]);
    Ok(argv)
}

/// Edits reviewers, labels and assignees of pull request `number` through `Client::mutate`.
pub fn run_edit<R: Runner>(
    client: &Client<R>,
    root: &str,
    number: u64,
    edit: &PrEdit,
) -> Result<(), GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    let argv = edit_argv(&repo, number, edit)?;
    client.mutate(&repo, &argv).map(|_| ())
}

/// The label names of `repo` (first 100, REST `labels` through the cached `rest`).
pub fn parse_labels(body: &str) -> Result<Vec<String>, GhError> {
    let v: Value = serde_json::from_str(body).map_err(|e| GhError::Other {
        message: format!("unexpected GitHub answer: {e}"),
    })?;
    let list = v.as_array().ok_or(GhError::Other {
        message: "unexpected GitHub answer".into(),
    })?;
    Ok(list.iter().filter_map(|l| opt_text(&l["name"])).collect())
}

pub fn repo_labels<R: Runner>(client: &Client<R>, root: &str) -> Result<Vec<String>, GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    let path = format!("repos/{}/{}/labels?per_page=100", repo.owner, repo.name);
    parse_labels(&client.rest(&path)?)
}

#[cfg(test)]
#[path = "github_pulls_tests.rs"]
mod tests;
