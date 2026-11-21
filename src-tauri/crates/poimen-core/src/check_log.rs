//! The failed log of one check of a pull request (`gh run view --log-failed`), for the detail pane.

use crate::github::{Client, GhError, LOG_LIMIT_PER_RUN, Repo, Runner, run_log_argv, trim_log};

/// The run and job ids of a check's details URL
/// `https://github.com/<owner>/<name>/actions/runs/<run>[/job/<job>]` of `repo`; `None` for any
/// other URL (an external status, another repository).
pub fn check_ids(repo: &Repo, url: &str) -> Option<(u64, Option<u64>)> {
    let rest = url.strip_prefix("https://github.com/")?;
    let rest = rest.split(['?', '#']).next()?;
    let mut parts = rest.split('/');
    if parts.next()? != repo.owner || parts.next()? != repo.name {
        return None;
    }
    if (parts.next()?, parts.next()?) != ("actions", "runs") {
        return None;
    }
    let digits = |s: &str| {
        (!s.is_empty() && s.len() <= 18 && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse::<u64>().ok())
            .flatten()
    };
    let run = digits(parts.next()?)?;
    let job = match (parts.next(), parts.next()) {
        (None, _) => None,
        (Some("job"), Some(j)) => Some(digits(j)?),
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some((run, job))
}

/// `gh run view <run> --log-failed [--job <job>]` argv.
pub fn check_log_argv(repo: &Repo, run_id: u64, job_id: Option<u64>) -> Vec<String> {
    let mut argv = run_log_argv(repo, run_id);
    if let Some(job) = job_id {
        argv.push("--job".into());
        argv.push(job.to_string());
    }
    argv
}

/// The failed log of one check (its job, else its run), trimmed to its last `LOG_LIMIT_PER_RUN`
/// bytes. A URL that is not a run of `repo` is an error.
pub fn check_log<R: Runner>(client: &Client<R>, repo: &Repo, url: &str) -> Result<String, GhError> {
    let (run, job) = check_ids(repo, url).ok_or(GhError::Other {
        message: "this check has no GitHub Actions log".into(),
    })?;
    let log = client.read(&check_log_argv(repo, run, job))?;
    Ok(trim_log(&log, LOG_LIMIT_PER_RUN))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::Output;
    use std::cell::RefCell;
    use std::io;
    use std::time::Duration;

    struct Fake {
        stdout: String,
        calls: RefCell<Vec<Vec<String>>>,
    }

    impl Runner for &Fake {
        fn gh(&self, args: &[String]) -> io::Result<Output> {
            self.calls.borrow_mut().push(args.to_vec());
            Ok(Output {
                code: Some(0),
                stdout: self.stdout.clone(),
                stderr: String::new(),
            })
        }
        fn now(&self) -> Duration {
            Duration::from_secs(1_000_000)
        }
    }

    fn repo() -> Repo {
        Repo {
            owner: "o".into(),
            name: "n".into(),
        }
    }

    #[test]
    fn check_urls_give_run_and_job_of_the_repository_only() {
        let r = repo();
        let u = "https://github.com/o/n/actions/runs/12/job/34";
        assert_eq!(check_ids(&r, u), Some((12, Some(34))));
        assert_eq!(check_ids(&r, &format!("{u}?pr=1")), Some((12, Some(34))));
        assert_eq!(
            check_ids(&r, "https://github.com/o/n/actions/runs/12"),
            Some((12, None))
        );
        for bad in [
            "https://github.com/x/n/actions/runs/12",
            "https://github.com/o/n/pull/12",
            "https://github.com/o/n/actions/runs/1x",
            "https://github.com/o/n/actions/runs/12/job/",
            "https://github.com/o/n/actions/runs/12/attempts/2",
            "https://example.com/o/n/actions/runs/12",
        ] {
            assert_eq!(check_ids(&r, bad), None, "{bad}");
        }
    }

    #[test]
    fn check_log_reads_the_job_log_trimmed() {
        let f = Fake {
            stdout: "z".repeat(LOG_LIMIT_PER_RUN + 10),
            calls: RefCell::new(vec![]),
        };
        let dir = std::env::temp_dir().join(format!("poimen-check-log-{}", std::process::id()));
        let c = Client::new(&f, dir);
        let text = check_log(&c, &repo(), "https://github.com/o/n/actions/runs/12/job/34").unwrap();
        assert!(text.starts_with("[... 10 earlier"));
        assert_eq!(
            f.calls.borrow()[0],
            [
                "run",
                "view",
                "12",
                "--log-failed",
                "-R",
                "o/n",
                "--job",
                "34"
            ]
        );
        assert!(check_log(&c, &repo(), "https://example.com/x").is_err());
    }
}
