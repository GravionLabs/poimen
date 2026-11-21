//! Creating a pull request for the branch of a space from the Changes view (#996): the state that
//! decides whether to offer it, a draft of title and body from the commits and the diff through a
//! headless call of an agent harness (shared with the commit message, `commit_message`), and
//! `gh pr create` with the body on stdin. Nothing is forced; the branch is never pushed here.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use poimen_core::harness::{self, Harness};
use serde::Serialize;
use serde_json::Value;

use crate::changes::validate;
use crate::commit_message::{DIFF_LIMIT, TIMEOUT, clean, run, shorten, tracked};
use crate::git::{base_ref, run_bytes};
use crate::github::{Client, GhError, Repo, Runner, repo_of};

/// Bytes of the commit list given to the harness.
const LOG_LIMIT: usize = 8_000;

/// An open pull request of the branch.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OpenPull {
    pub number: u64,
    pub url: String,
}

/// What the Changes view needs to offer "Create pull request...".
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PullState {
    /// The checked out branch; `None` on a detached HEAD.
    pub branch: Option<String>,
    /// The base branch by the rule of `git::base_ref`, without the `origin/` prefix.
    pub base: Option<String>,
    /// `origin/<branch>` exists.
    pub pushed: bool,
    /// An open pull request has this branch as head.
    pub existing: Option<OpenPull>,
}

/// A drafted title and body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Draft {
    pub title: String,
    pub body: String,
}

/// A created pull request.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Created {
    pub number: u64,
    pub url: String,
}

fn text(bytes: Vec<u8>) -> String {
    String::from_utf8_lossy(&bytes).trim().to_owned()
}

fn branch_of(top: &str) -> Option<String> {
    let name = text(run_bytes(top, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()?);
    (!name.is_empty() && name != "HEAD").then_some(name)
}

fn base_name(top: &str) -> Option<String> {
    base_ref(top).map(|r| r.strip_prefix("origin/").unwrap_or(&r).to_owned())
}

/// `gh pr list` argv for the open pull requests with head `branch`.
fn list_argv(repo: &Repo, branch: &str) -> Vec<String> {
    [
        "pr",
        "list",
        "--head",
        branch,
        "--state",
        "open",
        "--json",
        "number,url",
        "--limit",
        "1",
        "-R",
        &format!("{}/{}", repo.owner, repo.name),
    ]
    .map(String::from)
    .to_vec()
}

fn parse_open(body: &str) -> Result<Option<OpenPull>, GhError> {
    let v: Value = serde_json::from_str(body).map_err(|e| GhError::Other {
        message: format!("unexpected GitHub answer: {e}"),
    })?;
    Ok(v.as_array().and_then(|a| a.first()).and_then(|p| {
        Some(OpenPull {
            number: p.get("number")?.as_u64()?,
            url: p.get("url")?.as_str()?.to_owned(),
        })
    }))
}

/// The pull request state of the branch of `dir`. The open-pull-request lookup goes straight to
/// GitHub and is skipped (None) for a detached HEAD or an unpushed branch.
pub fn state<R: Runner>(
    client: &Client<R>,
    dir: &str,
    home: Option<&Path>,
) -> Result<PullState, String> {
    let top = validate(dir, home)?;
    let branch = branch_of(&top);
    let base = base_name(&top);
    let pushed = branch.as_deref().is_some_and(|b| {
        run_bytes(
            &top,
            &[
                "rev-parse",
                "--verify",
                "--quiet",
                &format!("refs/remotes/origin/{b}"),
            ],
        )
        .is_ok()
    });
    let mut existing = None;
    if let (true, Some(b)) = (pushed, branch.as_deref()) {
        let repo = repo_of(Path::new(&top)).ok_or("This repository has no GitHub remote")?;
        let out = client
            .read_fresh(&list_argv(&repo, b))
            .map_err(|e| format!("{e:?}"))?;
        existing = parse_open(&out).map_err(|e| format!("{e:?}"))?;
    }
    Ok(PullState {
        branch,
        base,
        pushed,
        existing,
    })
}

/// A branch name usable as a ref argument: not empty, no leading `-`, no whitespace or `..`.
fn valid_ref(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.contains("..")
        && !name.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn prompt(branch: &str, base: &str, log: &str, diff: &str) -> String {
    format!(
        "Write the title and the description of a GitHub pull request for the branch `{branch}` \
         into `{base}`.\nRules: the first line is the title, a Conventional Commit style \
         `type(scope): summary` in the imperative, at most 72 characters; then a blank line and \
         the description in Markdown: what changed and why, short. Reply with the text only: no \
         code fence around it, no explanation.\n\nCommits:\n{log}\n\nDiff:\n{diff}"
    )
}

/// Splits a reply into title (first non-blank line, without a leading `#` or backticks) and body.
pub(crate) fn split_draft(reply: &str) -> Option<Draft> {
    let reply = clean(reply);
    let (first, rest) = reply.split_once('\n').unwrap_or((&reply, ""));
    let title = first
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches('`')
        .trim();
    (!title.is_empty()).then(|| Draft {
        title: title.to_owned(),
        body: rest.trim().to_owned(),
    })
}

fn draft_in(
    dir: &str,
    harness: &Harness,
    base: &str,
    flag: &AtomicBool,
    home: Option<&Path>,
) -> Result<Draft, String> {
    let top = validate(dir, home)?;
    if !valid_ref(base) {
        return Err(format!("{base:?} is not a valid base branch"));
    }
    let base_ref = format!("origin/{base}");
    if run_bytes(&top, &["rev-parse", "--verify", "--quiet", &base_ref]).is_err() {
        return Err(format!("{base_ref} does not exist; fetch first"));
    }
    let branch = branch_of(&top).ok_or("HEAD is detached")?;
    let range = format!("{base_ref}..HEAD");
    let log = text(run_bytes(
        &top,
        &["log", "--no-color", "--format=%h %s%n%b", &range],
    )?);
    let diff = run_bytes(
        &top,
        &[
            "diff",
            "--no-color",
            "--no-ext-diff",
            "--no-textconv",
            &format!("{base_ref}...HEAD"),
        ],
    )?;
    if log.is_empty() && diff.is_empty() {
        return Err(format!("The branch has no commits beyond {base_ref}"));
    }
    let diff = String::from_utf8_lossy(&diff);
    let log = crate::commit_message::cut(&log, LOG_LIMIT);
    let message = prompt(&branch, base, log, &shorten(&diff, DIFF_LIMIT));
    let argv = harness::build_print_argv(harness, &message)
        .ok_or_else(|| format!("{} has no printArgs", harness.id))?;
    if flag.load(Ordering::SeqCst) {
        return Err("Cancelled".into());
    }
    let reply = run(&argv, &top, flag, TIMEOUT)?;
    split_draft(&reply).ok_or_else(|| format!("{} returned no title", harness.id))
}

/// `gh pr create` argv; the body comes on stdin (`--body-file -`), argv only, never a shell.
pub fn create_argv(
    repo: &Repo,
    branch: &str,
    base: &str,
    title: &str,
    draft: bool,
) -> Result<Vec<String>, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("A title is required".into());
    }
    if !valid_ref(base) || !valid_ref(branch) {
        return Err("The base or head branch is not valid".into());
    }
    let mut argv: Vec<String> = [
        "pr",
        "create",
        "--base",
        base,
        "--head",
        branch,
        "--title",
        title,
        "--body-file",
        "-",
    ]
    .map(String::from)
    .to_vec();
    if draft {
        argv.push("--draft".into());
    }
    argv.extend(["-R".into(), format!("{}/{}", repo.owner, repo.name)]);
    Ok(argv)
}

/// The number and URL from the output of `gh pr create` (the URL is its last line).
pub(crate) fn parse_created(out: &str) -> Option<Created> {
    let url = out
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| l.contains("/pull/"))?;
    let number = url.rsplit('/').next()?.parse().ok()?;
    Some(Created {
        number,
        url: url.to_owned(),
    })
}

/// Creates the pull request of the branch of `dir` into `base`.
pub fn create<R: Runner>(
    client: &Client<R>,
    dir: &str,
    base: &str,
    title: &str,
    body: &str,
    draft: bool,
    home: Option<&Path>,
) -> Result<Created, String> {
    let top = validate(dir, home)?;
    let branch = branch_of(&top).ok_or("HEAD is detached")?;
    let repo = repo_of(Path::new(&top)).ok_or("This repository has no GitHub remote")?;
    let argv = create_argv(&repo, &branch, base, title, draft)?;
    let out = client
        .mutate_input(&repo, &argv, body)
        .map_err(|e| match e {
            GhError::Other { message } => message,
            other => format!("{other:?}"),
        })?;
    parse_created(&out).ok_or_else(|| format!("Unexpected answer of gh: {}", out.trim()))
}

/// The pull request state of the branch of `dir`.
#[tauri::command]
pub async fn changes_pr_state(
    client: tauri::State<'_, crate::GithubClient>,
    dir: String,
) -> Result<PullState, String> {
    let client = client.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state(&client, &dir, crate::home_dir().as_deref()))
        .await
        .map_err(|e| e.to_string())?
}

/// Drafts title and body of a pull request into `base` with `harness`; `id` is cancelled by
/// `changes_generate_cancel`. Creates nothing.
#[tauri::command]
pub async fn changes_draft_pull_request(
    dir: String,
    harness: String,
    base: String,
    id: String,
) -> Result<Draft, String> {
    let found = crate::get_settings()
        .harnesses
        .into_iter()
        .find(|h| h.id == harness)
        .ok_or_else(|| format!("unknown harness {harness:?}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        tracked(&id, |flag| {
            draft_in(&dir, &found, &base, flag, crate::home_dir().as_deref())
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Creates the pull request of the branch of `dir` (`gh pr create`, body on stdin).
#[tauri::command]
pub async fn changes_create_pull_request(
    client: tauri::State<'_, crate::GithubClient>,
    dir: String,
    base: String,
    title: String,
    body: String,
    draft: bool,
) -> Result<Created, String> {
    let client = client.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        create(
            &client,
            &dir,
            &base,
            &title,
            &body,
            draft,
            crate::home_dir().as_deref(),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Adds and removes reviewers, labels and assignees of a pull request.
#[tauri::command]
pub async fn github_pr_edit(
    client: tauri::State<'_, crate::GithubClient>,
    root: String,
    number: u64,
    edit: crate::github_pulls::PrEdit,
) -> Result<(), GhError> {
    let client = client.inner().clone();
    let root = crate::github_pulls::expand_home(&root, crate::home_dir().as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        crate::github_pulls::run_edit(&client, &root, number, &edit)
    })
    .await
    .map_err(|e| GhError::Other {
        message: e.to_string(),
    })?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> Repo {
        Repo {
            owner: "o".into(),
            name: "n".into(),
        }
    }

    #[test]
    fn create_argv_has_the_body_on_stdin_and_no_force() {
        let a = create_argv(&repo(), "feat/x", "release/next", " Title ", false).unwrap();
        assert_eq!(
            a,
            [
                "pr",
                "create",
                "--base",
                "release/next",
                "--head",
                "feat/x",
                "--title",
                "Title",
                "--body-file",
                "-",
                "-R",
                "o/n"
            ]
        );
        assert!(!a.iter().any(|s| s.contains("force")));
        let d = create_argv(&repo(), "feat/x", "main", "t", true).unwrap();
        assert!(d.contains(&"--draft".to_owned()));
    }

    #[test]
    fn create_argv_refuses_a_blank_title_and_option_like_branches() {
        assert!(create_argv(&repo(), "b", "main", "  ", false).is_err());
        assert!(create_argv(&repo(), "b", "--web", "t", false).is_err());
        assert!(create_argv(&repo(), "-x", "main", "t", false).is_err());
    }

    #[test]
    fn splits_a_reply_into_title_and_body() {
        let d = split_draft("```\n# feat(x): add y\n\nWhy.\n\n- a\n```").unwrap();
        assert_eq!(d.title, "feat(x): add y");
        assert_eq!(d.body, "Why.\n\n- a");
        let d = split_draft("fix: z").unwrap();
        assert_eq!((d.title.as_str(), d.body.as_str()), ("fix: z", ""));
        assert!(split_draft("  \n").is_none());
    }

    #[test]
    fn parses_the_url_of_a_created_pull_request() {
        let out = "\nCreating pull request for x into y\n\nhttps://github.com/o/n/pull/42\n";
        assert_eq!(
            parse_created(out),
            Some(Created {
                number: 42,
                url: "https://github.com/o/n/pull/42".into()
            })
        );
        assert_eq!(parse_created("nothing"), None);
    }

    #[test]
    fn parses_the_open_pull_request_of_a_branch() {
        let one = r#"[{"number":7,"url":"https://github.com/o/n/pull/7"}]"#;
        assert_eq!(parse_open(one).unwrap().unwrap().number, 7);
        assert_eq!(parse_open("[]").unwrap(), None);
        assert!(parse_open("nope").is_err());
    }

    #[test]
    fn list_argv_asks_for_open_pull_requests_of_the_head() {
        let a = list_argv(&repo(), "feat/x");
        assert!(a.windows(2).any(|w| w == ["--head", "feat/x"]));
        assert!(a.windows(2).any(|w| w == ["--state", "open"]));
    }

    #[test]
    fn valid_ref_rejects_options_and_spaces() {
        assert!(valid_ref("release/next"));
        assert!(!valid_ref("-x"));
        assert!(!valid_ref("a b"));
        assert!(!valid_ref("a..b"));
        assert!(!valid_ref(""));
    }
}

/// The failed log of one check of a pull request's head commit (`gh run view --log-failed`).
#[tauri::command]
pub async fn github_check_log(
    client: tauri::State<'_, crate::GithubClient>,
    root: String,
    url: String,
) -> Result<String, GhError> {
    let client = client.inner().clone();
    let root = crate::github_pulls::expand_home(&root, crate::home_dir().as_deref());
    tauri::async_runtime::spawn_blocking(move || {
        let repo =
            poimen_core::github::repo_of(std::path::Path::new(&root)).ok_or(GhError::NotFound)?;
        poimen_core::check_log::check_log(&client, &repo, &url)
    })
    .await
    .map_err(|e| GhError::Other {
        message: e.to_string(),
    })?
}
