//! The pull request of each space's branch for the sidebar (ADR 0005, #250): branches from
//! `git::infos`, grouped per repository, one GraphQL query per repository with one aliased
//! `pullRequests(headRefName: …)` per branch. Branch names travel as variables, never inside the
//! query text. Read-only.

use std::collections::{BTreeSet, HashMap};
use std::sync::Mutex;

use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::git;
use crate::github::{Client, GhError, Repo, Runner, repo_of};
use crate::github_pulls::{Checks, parse_checks};

/// The newest open or merged pull request of a branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchPull {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// `open` or `merged`.
    pub state: String,
    pub is_draft: bool,
    pub checks: Checks,
}

/// Branches that never get a lookup: the trunk and release branches.
pub fn skipped(branch: &str) -> bool {
    matches!(branch, "main" | "master") || branch.starts_with("release/")
}

/// The query for `count` branches: alias `b<i>` takes the variable `$b<i>`.
pub fn build_query(count: usize) -> String {
    let decls: String = (0..count).map(|i| format!(", $b{i}: String!")).collect();
    let aliases: String = (0..count)
        .map(|i| {
            format!(
                " b{i}: pullRequests(headRefName: $b{i}, states: [OPEN, MERGED], first: 1, \
orderBy: {{field: UPDATED_AT, direction: DESC}}) {{ nodes {{ number title url state isDraft \
commits(last: 1) {{ nodes {{ commit {{ statusCheckRollup {{ state contexts(first: 1) {{ \
checkRunCountsByState {{ state count }} statusContextCountsByState {{ state count }} }} }} }} }} }} }} }} }}"
            )
        })
        .collect();
    format!(
        "query($owner: String!, $name: String!{decls}) {{ repository(owner: $owner, name: $name) {{{aliases} }} }}"
    )
}

/// Variables of the query: owner, name and one `b<i>` per branch.
pub fn build_vars(repo: &Repo, branches: &[String]) -> Value {
    let mut vars = Map::new();
    vars.insert("owner".into(), json!(repo.owner));
    vars.insert("name".into(), json!(repo.name));
    for (i, branch) in branches.iter().enumerate() {
        vars.insert(format!("b{i}"), json!(branch));
    }
    Value::Object(vars)
}

fn parse_node(node: &Value) -> Option<BranchPull> {
    Some(BranchPull {
        number: node["number"].as_u64()?,
        title: node["title"].as_str().unwrap_or_default().to_owned(),
        url: node["url"].as_str()?.to_owned(),
        state: if node["state"].as_str() == Some("MERGED") {
            "merged".into()
        } else {
            "open".into()
        },
        is_draft: node["isDraft"].as_bool().unwrap_or(false),
        checks: parse_checks(node),
    })
}

/// Pull request per branch (by position in `branches`) from one query answer; branches without a
/// pull request are left out.
pub fn parse_answer(
    answer: &Value,
    branches: &[String],
) -> Result<HashMap<String, BranchPull>, GhError> {
    let repository = &answer["data"]["repository"];
    if !repository.is_object() {
        let message = answer["errors"][0]["message"]
            .as_str()
            .unwrap_or("the repository is not readable")
            .to_owned();
        return Err(GhError::Other { message });
    }
    Ok(branches
        .iter()
        .enumerate()
        .filter_map(|(i, branch)| {
            let node = repository[format!("b{i}")]["nodes"].get(0)?;
            Some((branch.clone(), parse_node(node)?))
        })
        .collect())
}

fn load_repo<R: Runner>(
    client: &Client<R>,
    repo: &Repo,
    branches: &[String],
) -> Result<HashMap<String, BranchPull>, GhError> {
    let answer = client.graphql(&build_query(branches.len()), &build_vars(repo, branches))?;
    parse_answer(&answer, branches)
}

/// The last error that was logged, so a failing `gh` is reported once, not per refresh.
static LAST_LOGGED: Mutex<Option<String>> = Mutex::new(None);

fn log_once(error: Option<&GhError>) {
    let text = error.map(|e| format!("{e:?}"));
    if let Ok(mut last) = LAST_LOGGED.lock()
        && *last != text
    {
        if let Some(text) = &text {
            eprintln!("branch pull requests unavailable: {text}");
        }
        *last = text;
    }
}

type Group = (Repo, HashMap<String, Vec<String>>);

/// Pull request per directory for the branches among `dirs`. Failing repositories leave their
/// directories out (the error is logged once).
pub fn load<R: Runner + Sync>(
    client: &Client<R>,
    dirs: Vec<String>,
) -> HashMap<String, BranchPull> {
    // repository -> (repository, branch -> directories on it)
    let mut groups: HashMap<(String, String), Group> = HashMap::new();
    for (dir, info) in git::infos(dirs) {
        let Some(branch) = info.branch.filter(|b| !skipped(b)) else {
            continue;
        };
        let Some(repo) = repo_of(std::path::Path::new(&dir)) else {
            continue;
        };
        let key = (repo.owner.to_lowercase(), repo.name.to_lowercase());
        groups
            .entry(key)
            .or_insert_with(|| (repo, HashMap::new()))
            .1
            .entry(branch)
            .or_default()
            .push(dir);
    }
    let groups: Vec<Group> = groups.into_values().collect();
    let results: Vec<_> = std::thread::scope(|scope| {
        let handles: Vec<_> = groups
            .iter()
            .map(|(repo, by_branch)| {
                let branches: Vec<String> = by_branch
                    .keys()
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                scope.spawn(move || load_repo(client, repo, &branches))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join().unwrap_or_else(|_| {
                    Err(GhError::Other {
                        message: "the GitHub reader stopped unexpectedly".into(),
                    })
                })
            })
            .collect()
    });
    let mut out = HashMap::new();
    let mut failure = None;
    for ((_, by_branch), found) in groups.iter().zip(results) {
        match found {
            Ok(found) => {
                for (branch, pull) in found {
                    for dir in by_branch.get(&branch).into_iter().flatten() {
                        out.insert(dir.clone(), pull.clone());
                    }
                }
            }
            Err(error) => failure = Some(error),
        }
    }
    log_once(failure.as_ref());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> Repo {
        Repo {
            owner: "o".into(),
            name: "r".into(),
        }
    }

    fn fixture() -> Value {
        serde_json::from_str(include_str!("../fixtures/github-branch-pulls.json")).unwrap()
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn trunk_and_release_branches_are_skipped() {
        assert!(skipped("main") && skipped("master") && skipped("release/next"));
        assert!(!skipped("feat/x") && !skipped("releases") && !skipped("mainline"));
    }

    #[test]
    fn the_query_has_one_alias_and_variable_per_branch() {
        let query = build_query(2);
        assert!(
            query.starts_with("query($owner: String!, $name: String!, $b0: String!, $b1: String!)")
        );
        assert!(
            query.contains("b0: pullRequests(headRefName: $b0, states: [OPEN, MERGED], first: 1")
        );
        assert!(query.contains("b1: pullRequests(headRefName: $b1"));
        assert!(!query.contains("b2"));
        assert!(query.contains("orderBy: {field: UPDATED_AT, direction: DESC}"));
    }

    #[test]
    fn branch_names_needing_escaping_travel_as_variables() {
        let branches = names(&["fix/\"quoted\"", "feat/ünï\\code", "a'b$c"]);
        let vars = build_vars(&repo(), &branches);
        assert_eq!(vars["owner"], "o");
        assert_eq!(vars["b0"], "fix/\"quoted\"");
        assert_eq!(vars["b1"], "feat/ünï\\code");
        assert_eq!(vars["b2"], "a'b$c");
        let query = build_query(branches.len());
        assert!(!query.contains("quoted") && !query.contains("code"));
    }

    #[test]
    fn the_answer_maps_to_branches_with_state_draft_and_checks() {
        let found = parse_answer(&fixture(), &names(&["feat/a", "feat/none", "old"])).unwrap();
        assert_eq!(found.len(), 2);
        let a = &found["feat/a"];
        assert_eq!((a.number, a.state.as_str(), a.is_draft), (12, "open", true));
        assert_eq!(a.title, "Add the thing");
        assert_eq!(a.url, "https://github.com/o/r/pull/12");
        assert_eq!(
            (
                a.checks.state.as_str(),
                a.checks.passed,
                a.checks.failed,
                a.checks.pending
            ),
            ("failing", 3, 1, 2)
        );
        let old = &found["old"];
        assert_eq!((old.number, old.state.as_str()), (7, "merged"));
        assert_eq!(old.checks.state, "none");
        assert!(!found.contains_key("feat/none"));
    }

    #[test]
    fn an_answer_without_a_repository_is_an_error() {
        let answer =
            json!({"data": {"repository": null}, "errors": [{"message": "Could not resolve"}]});
        assert_eq!(
            parse_answer(&answer, &names(&["x"])),
            Err(GhError::Other {
                message: "Could not resolve".into()
            })
        );
    }

    #[test]
    fn a_missing_gh_is_an_error_not_a_panic() {
        struct Broken;
        impl Runner for &Broken {
            fn gh(&self, _: &[String]) -> std::io::Result<crate::github::Output> {
                Err(std::io::ErrorKind::NotFound.into())
            }
            fn now(&self) -> std::time::Duration {
                std::time::Duration::from_secs(1)
            }
        }
        let client = Client::new(&Broken, std::env::temp_dir().join("poimen-branches-test"));
        assert_eq!(
            load_repo(&client, &repo(), &names(&["feat/a"])),
            Err(GhError::GhMissing)
        );
    }
}
