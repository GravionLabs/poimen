//! Open milestones per repository for the GitHub view (ADR 0005, #233): REST through the shared
//! `github::Client` (ETag cache), ordered by version; the current one is the lowest versioned
//! milestone that still has open issues. Versioned are the legacy `x.y.z` titles and `M<n>` /
//! `M<n> (x.y.z)` (sprint number, version added when released); legacy ones sort first. Read-only.

use serde::Serialize;
use serde_json::Value;

use crate::github::{Client, GhError, Repo, Runner};
use crate::github_issues::unique_repos;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// ISO 8601 timestamp, when the milestone has a due date.
    pub due_on: Option<String>,
    pub open: u64,
    pub closed: u64,
    /// The lowest versioned milestone (`x.y.z`, `M<n>`, `M<n> (x.y.z)`) with open issues.
    pub current: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum Outcome {
    Ok { milestones: Vec<Milestone> },
    Error { error: GhError },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepoMilestones {
    pub root: String,
    /// `owner/name`.
    pub repo: String,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// `x.y.z` (a leading `v` is allowed) as numbers; anything else is not a version.
pub fn parse_version(title: &str) -> Option<(u64, u64, u64)> {
    let t = title.trim();
    let t = t.strip_prefix('v').unwrap_or(t);
    let mut parts = t.split('.');
    let mut next = || {
        let p = parts.next()?;
        (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
            .then(|| p.parse::<u64>().ok())
            .flatten()
    };
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// `M<n>` or `M<n> (x.y.z)` as the sprint number; anything else is not a sprint milestone.
pub fn parse_sprint(title: &str) -> Option<u64> {
    let t = title.trim();
    let rest = t.strip_prefix('M').or_else(|| t.strip_prefix('m'))?;
    let (digits, tail) = match rest.split_once(' ') {
        Some((d, tail)) => (d, Some(tail.trim())),
        None => (rest, None),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if let Some(tail) = tail {
        let version = tail.strip_prefix('(')?.strip_suffix(')')?;
        parse_version(version)?;
    }
    digits.parse().ok()
}

/// Sort key of a versioned milestone: legacy `x.y.z` before `M<n>`, each numerically.
fn rank(title: &str) -> Option<(u8, u64, u64, u64)> {
    if let Some((x, y, z)) = parse_version(title) {
        return Some((0, x, y, z));
    }
    parse_sprint(title).map(|n| (1, n, 0, 0))
}

/// Milestones by version ascending (numerically, so 1.10.0 follows 1.2.0 and M10 follows M2;
/// legacy versions first), the others by title after them; marks the current one.
pub fn order(mut milestones: Vec<Milestone>) -> Vec<Milestone> {
    milestones.sort_by(|a, b| match (rank(&a.title), rank(&b.title)) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.title.cmp(&b.title)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
    });
    if let Some(m) = milestones
        .iter_mut()
        .find(|m| m.open > 0 && rank(&m.title).is_some())
    {
        m.current = true;
    }
    milestones
}

/// The milestones of a `repos/{o}/{r}/milestones` answer (entries without a title are skipped).
pub fn parse_milestones(body: &str) -> Result<Vec<Milestone>, GhError> {
    let value: Value = serde_json::from_str(body).map_err(|e| GhError::Other {
        message: format!("unexpected GitHub answer: {e}"),
    })?;
    let Some(list) = value.as_array() else {
        return Err(GhError::Other {
            message: "unexpected GitHub answer".into(),
        });
    };
    let milestones = list
        .iter()
        .filter_map(|m| {
            Some(Milestone {
                number: m["number"].as_u64()?,
                title: m["title"].as_str()?.to_owned(),
                url: m["html_url"].as_str().unwrap_or_default().to_owned(),
                due_on: m["due_on"].as_str().map(str::to_owned),
                open: m["open_issues"].as_u64().unwrap_or(0),
                closed: m["closed_issues"].as_u64().unwrap_or(0),
                current: false,
            })
        })
        .collect();
    Ok(order(milestones))
}

fn load_repo<R: Runner>(client: &Client<R>, repo: &Repo) -> Outcome {
    let path = format!(
        "repos/{}/{}/milestones?state=open&per_page=100",
        repo.owner, repo.name
    );
    match client.rest(&path).and_then(|body| parse_milestones(&body)) {
        Ok(milestones) => Outcome::Ok { milestones },
        Err(error) => Outcome::Error { error },
    }
}

/// Open milestones of the GitHub repositories among `dirs`.
pub fn load<R: Runner + Sync>(client: &Client<R>, dirs: &[String]) -> Vec<RepoMilestones> {
    let repos = unique_repos(dirs);
    std::thread::scope(|scope| {
        let handles: Vec<_> = repos
            .iter()
            .map(|(_, repo)| scope.spawn(|| load_repo(client, repo)))
            .collect();
        repos
            .iter()
            .zip(handles)
            .map(|((root, repo), handle)| RepoMilestones {
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIXTURE: &str = include_str!("../fixtures/github-milestones.json");

    fn titles(m: &[Milestone]) -> Vec<&str> {
        m.iter().map(|m| m.title.as_str()).collect()
    }

    #[test]
    fn versions_parse_with_an_optional_v_and_nothing_else() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("v1.10.0"), Some((1, 10, 0)));
        for bad in ["1.2", "1.2.3.4", "1.2.x", "Backlog", "", "1..3", "-1.2.3"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn sprint_titles_parse_with_an_optional_version() {
        assert_eq!(parse_sprint("M2"), Some(2));
        assert_eq!(parse_sprint("M10 (0.9.0)"), Some(10));
        for bad in [
            "M",
            "M2x",
            "M2 0.9.0",
            "M2 (0.9)",
            "M2 ()",
            "Milestone",
            "2",
            "M-1",
            "",
        ] {
            assert_eq!(parse_sprint(bad), None, "{bad}");
        }
    }

    #[test]
    fn versions_sort_numerically_legacy_first_and_other_names_come_last() {
        let m = parse_milestones(FIXTURE).unwrap();
        assert_eq!(
            titles(&m),
            [
                "v1.1.0",
                "1.1.5",
                "1.2.0",
                "1.10.0",
                "M2",
                "M10 (1.3.0)",
                "Backlog"
            ]
        );
    }

    #[test]
    fn the_lowest_sprint_with_open_issues_is_current_after_the_legacy_ones_are_done() {
        let m = parse_milestones(
            r#"[{"number":3,"title":"M10","open_issues":1,"closed_issues":0},
                {"number":2,"title":"M2 (0.9.0)","open_issues":0,"closed_issues":4},
                {"number":1,"title":"M3","open_issues":2,"closed_issues":0}]"#,
        )
        .unwrap();
        assert_eq!(titles(&m), ["M2 (0.9.0)", "M3", "M10"]);
        let current: Vec<&str> = m
            .iter()
            .filter(|m| m.current)
            .map(|m| m.title.as_str())
            .collect();
        assert_eq!(current, ["M3"]);
    }

    #[test]
    fn the_current_milestone_is_the_lowest_version_with_open_issues() {
        let m = parse_milestones(FIXTURE).unwrap();
        let current: Vec<&str> = m
            .iter()
            .filter(|m| m.current)
            .map(|m| m.title.as_str())
            .collect();
        // v1.1.0 has no open issue; Backlog is no version.
        assert_eq!(current, ["1.1.5"]);
    }

    #[test]
    fn without_a_versioned_milestone_with_open_issues_none_is_current() {
        let m = parse_milestones(
            r#"[{"number":1,"title":"Backlog","open_issues":3,"closed_issues":0},
                {"number":2,"title":"1.0.0","open_issues":0,"closed_issues":2}]"#,
        )
        .unwrap();
        assert!(m.iter().all(|m| !m.current));
    }

    #[test]
    fn counts_due_date_and_url_are_read() {
        let m = parse_milestones(FIXTURE).unwrap();
        let v = m.iter().find(|m| m.title == "1.2.0").unwrap();
        assert_eq!((v.open, v.closed), (3, 5));
        assert_eq!(v.due_on.as_deref(), Some("2026-11-20T08:00:00Z"));
        assert_eq!(v.url, "https://github.com/o/n/milestone/3");
        assert_eq!(m[0].due_on, None);
    }

    #[test]
    fn an_empty_list_is_fine_and_a_non_list_is_an_error() {
        assert_eq!(parse_milestones("[]").unwrap(), vec![]);
        assert!(parse_milestones(r#"{"message":"Not Found"}"#).is_err());
        assert!(parse_milestones("garbage").is_err());
    }

    #[test]
    fn the_wire_shape_is_camel_case_with_a_status_tag() {
        let m = parse_milestones(FIXTURE).unwrap();
        let value = serde_json::to_value(&m[1]).unwrap();
        assert_eq!(value["dueOn"], Value::Null);
        assert_eq!(value["current"], true);
        let repo = RepoMilestones {
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
}
