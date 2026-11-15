//! Open milestones per repository for the GitHub view (ADR 0005, #233): REST through the shared
//! `github::Client` (ETag cache), ordered by version; the current one is the lowest versioned
//! milestone that still has open issues. Versioned are the legacy `x.y.z` titles and `M<n>` /
//! `M<n> (x.y.z)` (sprint number, version added when released); legacy ones sort first. Writes (#343:
//! create, edit, close) go through `Client::mutate` as `gh api` calls, argv only.

use serde::Serialize;
use serde_json::Value;

use crate::github::{Client, GhError, Repo, Runner, repo_of};
use crate::github_issues::unique_repos;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Milestone {
    pub number: u64,
    pub title: String,
    pub url: String,
    /// ISO 8601 timestamp, when the milestone has a due date.
    pub due_on: Option<String>,
    pub description: String,
    pub open: u64,
    pub closed: u64,
    /// The lowest versioned milestone (`x.y.z`, `M<n>`, `M<n>.<m>`, `M<n> (x.y.z)`) with open issues.
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

/// `M<n>`, `M<n>.<m>` (inserted between two sprints) or either with ` (x.y.z)` or a name
/// (`M015.1 Sidebar organization`) as the sprint number and the insert number (0 without one); anything else is not a sprint milestone.
pub fn parse_sprint(title: &str) -> Option<(u64, u64)> {
    let t = title.trim();
    let rest = t.strip_prefix('M').or_else(|| t.strip_prefix('m'))?;
    let (digits, tail) = match rest.split_once(' ') {
        Some((d, tail)) => (d, Some(tail.trim())),
        None => (rest, None),
    };
    let (major, minor) = match digits.split_once('.') {
        Some((major, minor)) => (major, Some(minor)),
        None => (digits, None),
    };
    let numeric = |d: &str| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit());
    if !numeric(major) || !minor.is_none_or(numeric) {
        return None;
    }
    if let Some(tail) = tail {
        if tail.starts_with('(') {
            parse_version(tail.strip_prefix('(')?.strip_suffix(')')?)?;
        } else if !tail.chars().next().is_some_and(char::is_alphabetic) {
            // A name (`M015.1 Sidebar organization`) is fine; a bare number or version is not.
            return None;
        }
    }
    let minor = match minor {
        Some(m) => m.parse().ok()?,
        None => 0,
    };
    Some((major.parse().ok()?, minor))
}

/// Sort key of a versioned milestone: legacy `x.y.z` before `M<n>`, each numerically.
fn rank(title: &str) -> Option<(u8, u64, u64, u64)> {
    if let Some((x, y, z)) = parse_version(title) {
        return Some((0, x, y, z));
    }
    parse_sprint(title).map(|(n, m)| (1, n, m, 0))
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
                description: m["description"].as_str().unwrap_or_default().to_owned(),
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

/// A write on a milestone. `due_on` is a `YYYY-MM-DD` date; empty or `None` means no due date.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum MilestoneWrite {
    Create {
        title: String,
        description: String,
        due_on: Option<String>,
    },
    Update {
        number: u64,
        title: String,
        description: String,
        due_on: Option<String>,
    },
    Close {
        number: u64,
    },
}

fn invalid(message: &str) -> GhError {
    GhError::Other {
        message: message.into(),
    }
}

/// `YYYY-MM-DD` as the GitHub timestamp at the start of that day (UTC); `None` for an empty value.
fn due_timestamp(due_on: Option<&str>) -> Result<Option<String>, GhError> {
    let Some(d) = due_on.map(str::trim).filter(|d| !d.is_empty()) else {
        return Ok(None);
    };
    let b = d.as_bytes();
    let shape = b.len() == 10
        && b.iter().enumerate().all(|(i, c)| {
            if i == 4 || i == 7 {
                *c == b'-'
            } else {
                c.is_ascii_digit()
            }
        });
    let (month, day) = if shape {
        (
            d[5..7].parse::<u32>().unwrap_or(0),
            d[8..10].parse::<u32>().unwrap_or(0),
        )
    } else {
        (0, 0)
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return Err(invalid("the due date must be YYYY-MM-DD"));
    }
    Ok(Some(format!("{d}T00:00:00Z")))
}

/// The `gh` arguments (without the program name) of a milestone write.
pub fn write_argv(repo: &Repo, write: &MilestoneWrite) -> Result<Vec<String>, GhError> {
    let base = format!("repos/{}/{}/milestones", repo.owner, repo.name);
    let s = |v: &str| v.to_owned();
    let fields = |title: &str, description: &str, due_on: &Option<String>| {
        let title = title.trim();
        if title.is_empty() {
            return Err(invalid("a milestone needs a title"));
        }
        let mut f = vec![s("-f"), format!("title={title}")];
        f.extend([s("-f"), format!("description={description}")]);
        match due_timestamp(due_on.as_deref())? {
            Some(t) => f.extend([s("-f"), format!("due_on={t}")]),
            None => f.extend([s("-F"), s("due_on=null")]),
        }
        Ok(f)
    };
    let (method, path, rest) = match write {
        MilestoneWrite::Create {
            title,
            description,
            due_on,
        } => {
            let mut f = fields(title, description, due_on)?;
            // A new milestone without a date simply has none.
            if due_on.as_deref().is_none_or(|d| d.trim().is_empty()) {
                f.truncate(f.len() - 2);
            }
            ("POST", base, f)
        }
        MilestoneWrite::Update {
            number,
            title,
            description,
            due_on,
        } => (
            "PATCH",
            format!("{base}/{number}"),
            fields(title, description, due_on)?,
        ),
        MilestoneWrite::Close { number } => (
            "PATCH",
            format!("{base}/{number}"),
            vec![s("-f"), s("state=closed")],
        ),
    };
    let mut argv = vec![s("api"), s("-X"), s(method), path];
    argv.extend(rest);
    Ok(argv)
}

/// Runs `write` in the repository of `root` through `Client::mutate`.
pub fn run_write<R: Runner>(
    client: &Client<R>,
    root: &str,
    write: &MilestoneWrite,
) -> Result<(), GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    let argv = write_argv(&repo, write)?;
    client.mutate(&repo, &argv).map(|_| ())
}

/// `M<n+1>` after the highest sprint number among `titles`, zero padded like that title
/// (`M015` gives `M016`); `M1` without one. Inserted sprints (`M15.1`) count as `M15`.
pub fn next_title<'a>(titles: impl IntoIterator<Item = &'a str>) -> String {
    let highest = titles
        .into_iter()
        .filter_map(|t| {
            let (n, _) = parse_sprint(t)?;
            let digits = t.trim()[1..]
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .unwrap_or("")
                .len();
            Some((n, digits))
        })
        .max();
    match highest {
        Some((n, width)) => format!("M{:0width$}", n + 1),
        None => "M1".into(),
    }
}

/// The suggested title of a new milestone of the repository of `root`, from all its milestones
/// (open and closed).
pub fn suggest_title<R: Runner>(client: &Client<R>, root: &str) -> Result<String, GhError> {
    let repo = repo_of(std::path::Path::new(root)).ok_or(GhError::NotFound)?;
    let path = format!(
        "repos/{}/{}/milestones?state=all&per_page=100",
        repo.owner, repo.name
    );
    let milestones = parse_milestones(&client.rest(&path)?)?;
    Ok(next_title(milestones.iter().map(|m| m.title.as_str())))
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
        assert_eq!(parse_sprint("M2"), Some((2, 0)));
        assert_eq!(parse_sprint("M10 (0.9.0)"), Some((10, 0)));
        assert_eq!(parse_sprint("M015.1"), Some((15, 1)));
        assert_eq!(parse_sprint("M15.2 (1.4.0)"), Some((15, 2)));
        assert_eq!(parse_sprint("M015.1 Sidebar organization"), Some((15, 1)));
        assert_eq!(parse_sprint("M001 Dependencies and security"), Some((1, 0)));
        for bad in [
            "M",
            "M2x",
            "M2.",
            "M2.x",
            "M.1",
            "M2.1.1",
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
    fn an_inserted_sprint_sorts_between_its_neighbours_and_can_be_current() {
        let m = parse_milestones(
            r#"[{"number":3,"title":"M016","open_issues":1,"closed_issues":0},
                {"number":2,"title":"M015.1","open_issues":2,"closed_issues":0},
                {"number":1,"title":"M015","open_issues":0,"closed_issues":4}]"#,
        )
        .unwrap();
        assert_eq!(titles(&m), ["M015", "M015.1", "M016"]);
        let current: Vec<&str> = m
            .iter()
            .filter(|m| m.current)
            .map(|m| m.title.as_str())
            .collect();
        assert_eq!(current, ["M015.1"]);
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
        assert_eq!(m[0].description, "");
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

    fn repo() -> Repo {
        Repo {
            owner: "o".into(),
            name: "n".into(),
        }
    }

    #[test]
    fn a_write_arrives_in_the_camel_case_of_the_frontend() {
        let w: MilestoneWrite = serde_json::from_value(
            json!({"kind":"update","number":3,"title":"M4","description":"d","dueOn":"2026-12-01"}),
        )
        .unwrap();
        assert_eq!(
            w,
            MilestoneWrite::Update {
                number: 3,
                title: "M4".into(),
                description: "d".into(),
                due_on: Some("2026-12-01".into())
            }
        );
    }

    #[test]
    fn create_posts_title_description_and_due_date() {
        let argv = write_argv(
            &repo(),
            &MilestoneWrite::Create {
                title: " M4 ".into(),
                description: "Sprint".into(),
                due_on: Some("2026-12-01".into()),
            },
        )
        .unwrap();
        assert_eq!(
            argv,
            [
                "api",
                "-X",
                "POST",
                "repos/o/n/milestones",
                "-f",
                "title=M4",
                "-f",
                "description=Sprint",
                "-f",
                "due_on=2026-12-01T00:00:00Z"
            ]
        );
    }

    #[test]
    fn create_without_a_date_sends_none_and_update_clears_it() {
        let create = write_argv(
            &repo(),
            &MilestoneWrite::Create {
                title: "M4".into(),
                description: String::new(),
                due_on: Some(" ".into()),
            },
        )
        .unwrap();
        assert_eq!(create.len(), 8);
        assert!(!create.iter().any(|a| a.starts_with("due_on")));
        let update = write_argv(
            &repo(),
            &MilestoneWrite::Update {
                number: 3,
                title: "M4".into(),
                description: String::new(),
                due_on: None,
            },
        )
        .unwrap();
        assert_eq!(
            &update[..4],
            ["api", "-X", "PATCH", "repos/o/n/milestones/3"]
        );
        assert_eq!(&update[update.len() - 2..], ["-F", "due_on=null"]);
    }

    #[test]
    fn close_patches_the_state_only() {
        assert_eq!(
            write_argv(&repo(), &MilestoneWrite::Close { number: 7 }).unwrap(),
            [
                "api",
                "-X",
                "PATCH",
                "repos/o/n/milestones/7",
                "-f",
                "state=closed"
            ]
        );
    }

    #[test]
    fn an_empty_title_or_a_bad_date_is_refused() {
        let w = |title: &str, due: &str| MilestoneWrite::Create {
            title: title.into(),
            description: String::new(),
            due_on: Some(due.into()),
        };
        assert!(write_argv(&repo(), &w("  ", "")).is_err());
        for bad in ["2026-13-01", "2026-1-01", "tomorrow", "2026-12-32"] {
            assert!(write_argv(&repo(), &w("M1", bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_next_title_follows_the_highest_sprint() {
        assert_eq!(next_title(["M2", "M10 (1.3.0)", "Backlog", "1.2.0"]), "M11");
        assert_eq!(next_title(["M015", "M015.1", "M014"]), "M016");
        assert_eq!(next_title(["Backlog", "1.0.0"]), "M1");
        assert_eq!(next_title([]), "M1");
    }

    struct Fake {
        calls: std::cell::RefCell<Vec<Vec<String>>>,
        code: i32,
        stdout: String,
    }

    impl Runner for &Fake {
        fn gh(&self, args: &[String]) -> std::io::Result<crate::github::Output> {
            self.calls.borrow_mut().push(args.to_vec());
            Ok(crate::github::Output {
                code: Some(self.code),
                stdout: self.stdout.clone(),
                stderr: "boom".into(),
            })
        }
        fn now(&self) -> std::time::Duration {
            std::time::Duration::from_secs(1_000_000)
        }
    }

    #[test]
    fn a_write_runs_through_the_client_and_a_failure_is_an_error() {
        let dir = std::env::temp_dir().join(format!("poimen-ms-{}", std::process::id()));
        let ok = Fake {
            calls: Default::default(),
            code: 0,
            stdout: "{}".into(),
        };
        let client = Client::new(&ok, dir.clone());
        // `run_write` resolves the repository from a git directory; use the argv path directly.
        let argv = write_argv(&repo(), &MilestoneWrite::Close { number: 2 }).unwrap();
        client.mutate(&repo(), &argv).unwrap();
        assert_eq!(ok.calls.borrow()[0], argv);
        let bad = Fake {
            calls: Default::default(),
            code: 1,
            stdout: String::new(),
        };
        let client = Client::new(&bad, dir);
        assert!(client.mutate(&repo(), &argv).is_err());
        assert!(
            run_write(
                &client,
                "/nonexistent-dir",
                &MilestoneWrite::Close { number: 1 }
            )
            .is_err()
        );
    }
}
