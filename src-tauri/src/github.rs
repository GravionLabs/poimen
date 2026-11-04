//! Shared GitHub client over the `gh` CLI (ADR 0005): repository detection, REST with ETag
//! revalidation cached on disk, GraphQL cached in memory. The pull request view uses it.
//!
//! `gh` is started with an argv (never a shell) and the login-shell `PATH`. Nothing here logs
//! arguments, headers or the token; files are only written below the cache directory given to
//! `Client::new`.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gui_path;

/// How long an answer is served without asking GitHub again.
pub const CACHE_TTL: Duration = Duration::from_secs(30);

/// How long one `gh` call may run before it is killed.
const GH_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Repo {
    pub owner: String,
    pub name: String,
}

/// What the frontend gets when a call fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum GhError {
    GhMissing,
    NotLoggedIn,
    Offline,
    /// `reset` is the epoch second at which the limit resets, when GitHub said so.
    RateLimited {
        reset: Option<u64>,
    },
    NotFound,
    Other {
        message: String,
    },
}

/// Output of a finished child process.
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// The side effects the client needs; faked in tests.
pub trait Runner {
    /// Runs `gh` with `args`. `ErrorKind::NotFound` means `gh` is not installed.
    fn gh(&self, args: &[String]) -> io::Result<Output>;
    /// Wall-clock time since the Unix epoch.
    fn now(&self) -> Duration;
}

pub struct SystemRunner;

impl Runner for SystemRunner {
    fn gh(&self, args: &[String]) -> io::Result<Output> {
        let path = gui_path::child_path();
        let program = gui_path::resolve_program("gh", &path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "gh not found"))?;
        let mut child = Command::new(program)
            .args(args)
            .env("PATH", &path)
            .env("GH_PROMPT_DISABLED", "1")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let read = |pipe: Option<Box<dyn io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_end(&mut buf);
                }
                String::from_utf8_lossy(&buf).into_owned()
            })
        };
        let stdout = read(child.stdout.take().map(|p| Box::new(p) as _));
        let stderr = read(child.stderr.take().map(|p| Box::new(p) as _));
        let deadline = Instant::now() + GH_TIMEOUT;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(io::ErrorKind::TimedOut, "gh timed out"));
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        Ok(Output {
            code: status.code(),
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }

    fn now(&self) -> Duration {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
    }
}

/// `owner/name` of a GitHub remote URL; `None` for any other host.
pub fn parse_remote(url: &str) -> Option<Repo> {
    let url = url.trim();
    let rest = if let Some(r) = url.strip_prefix("git@github.com:") {
        r
    } else {
        let after = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))
            .or_else(|| url.strip_prefix("ssh://"))
            .or_else(|| url.strip_prefix("git://"))?;
        let (authority, path) = after.split_once('/')?;
        let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
        if !host.eq_ignore_ascii_case("github.com") {
            return None;
        }
        path
    };
    let rest = rest.trim_end_matches('/');
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let (owner, name) = rest.split_once('/')?;
    let valid = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (valid(owner) && valid(name)).then(|| Repo {
        owner: owner.to_owned(),
        name: name.to_owned(),
    })
}

/// The GitHub repository behind `origin` of the git directory `dir`.
pub fn repo_of(dir: &Path) -> Option<Repo> {
    let out = Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(dir)
        .env("PATH", gui_path::child_path())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_remote(&String::from_utf8_lossy(&out.stdout))
}

/// A `gh api --include` answer: status code, headers (lowercased names) and body.
#[derive(Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Splits `HTTP/2.0 200 OK\r\nName: value\r\n...\r\n\r\nbody`.
pub fn split_response(raw: &str) -> Option<Response> {
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .unwrap_or((raw, ""));
    let mut lines = head.lines();
    let status = lines
        .next()?
        .strip_prefix("HTTP/")?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    Some(Response {
        status,
        headers,
        body: body.to_owned(),
    })
}

/// Maps a failed `gh api` call that needs no further question. `status` is the HTTP status when
/// known. Missing credentials are decided by `Client::classify` (it asks `gh auth status`).
pub fn map_error(
    status: Option<u16>,
    response: Option<&Response>,
    stderr: &str,
) -> Option<GhError> {
    let lower = stderr.to_lowercase();
    let status = status.or_else(|| {
        let at = stderr.find("(HTTP ")? + 6;
        stderr[at..].split(')').next()?.trim().parse().ok()
    });
    if lower.contains("error connecting to")
        || lower.contains("no such host")
        || lower.contains("network is unreachable")
        || lower.contains("dial tcp")
    {
        return Some(GhError::Offline);
    }
    let remaining_zero = response
        .and_then(|r| r.header("x-ratelimit-remaining"))
        .is_some_and(|v| v == "0");
    if status == Some(429)
        || lower.contains("rate limit")
        || (status == Some(403) && remaining_zero)
    {
        let reset = response
            .and_then(|r| r.header("x-ratelimit-reset"))
            .and_then(|v| v.parse().ok());
        return Some(GhError::RateLimited { reset });
    }
    if status == Some(404) {
        return Some(GhError::NotFound);
    }
    None
}

#[allow(dead_code)] // REST and its disk cache are for the next GitHub views
#[derive(Serialize, Deserialize)]
struct DiskEntry {
    path: String,
    etag: Option<String>,
    body: String,
    /// Epoch milliseconds of the last successful (re)validation.
    fetched_ms: u64,
}

#[allow(dead_code)] // REST and its disk cache are for the next GitHub views
fn file_name(path: &str) -> String {
    // FNV-1a; the entry stores the full path, so a collision is detected, not trusted.
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in path.bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}.json")
}

pub struct Client<R: Runner> {
    runner: R,
    #[allow(dead_code)] // REST and its disk cache are for the next GitHub views
    cache_dir: PathBuf,
    graphql_cache: Mutex<HashMap<String, (Duration, Value)>>,
}

impl Client<SystemRunner> {
    pub fn system(cache_dir: PathBuf) -> Self {
        Self::new(SystemRunner, cache_dir)
    }
}

impl<R: Runner> Client<R> {
    pub fn new(runner: R, cache_dir: PathBuf) -> Self {
        Self {
            runner,
            cache_dir,
            graphql_cache: Mutex::new(HashMap::new()),
        }
    }

    fn run(&self, args: &[String]) -> Result<Output, GhError> {
        self.runner.gh(args).map_err(|e| {
            if e.kind() == io::ErrorKind::NotFound {
                GhError::GhMissing
            } else {
                GhError::Other {
                    message: e.to_string(),
                }
            }
        })
    }

    /// Turns a failed call into an error; asks `gh auth status` when the failure looks like
    /// missing credentials.
    fn classify(&self, status: Option<u16>, response: Option<&Response>, stderr: &str) -> GhError {
        if let Some(err) = map_error(status, response, stderr) {
            return err;
        }
        let lower = stderr.to_lowercase();
        let suspicious = status == Some(401)
            || lower.contains("gh auth login")
            || lower.contains("not logged in");
        if suspicious {
            let logged_in = self
                .run(&["auth".into(), "status".into()])
                .is_ok_and(|o| o.code == Some(0));
            if !logged_in {
                return GhError::NotLoggedIn;
            }
        }
        let message = stderr
            .trim()
            .lines()
            .next()
            .unwrap_or("gh failed")
            .to_owned();
        GhError::Other { message }
    }

    #[allow(dead_code)] // REST and its disk cache are for the next GitHub views
    fn read_entry(&self, path: &str) -> Option<DiskEntry> {
        let text = std::fs::read_to_string(self.cache_dir.join(file_name(path))).ok()?;
        let entry: DiskEntry = serde_json::from_str(&text).ok()?;
        (entry.path == path).then_some(entry)
    }

    #[allow(dead_code)] // REST and its disk cache are for the next GitHub views
    fn write_entry(&self, entry: &DiskEntry) {
        // A cache that cannot be written is only slower, never wrong.
        if std::fs::create_dir_all(&self.cache_dir).is_err() {
            return;
        }
        let Ok(text) = serde_json::to_string(entry) else {
            return;
        };
        let name = file_name(&entry.path);
        let tmp = self.cache_dir.join(format!("{name}.tmp"));
        if std::fs::write(&tmp, text).is_ok()
            && std::fs::rename(&tmp, self.cache_dir.join(name)).is_err()
        {
            let _ = std::fs::remove_file(&tmp);
        }
    }

    /// `GET path` through `gh api`, revalidated with the cached ETag; the cached body is served
    /// without a request for `CACHE_TTL`.
    #[allow(dead_code)] // REST and its disk cache are for the next GitHub views
    pub fn rest(&self, path: &str) -> Result<String, GhError> {
        // A path starting with `-` would be read by `gh` as a flag.
        if path.starts_with('-') {
            return Err(GhError::Other {
                message: "invalid API path".into(),
            });
        }
        let now_ms = self.runner.now().as_millis() as u64;
        let cached = self.read_entry(path);
        if let Some(entry) = &cached
            && now_ms.saturating_sub(entry.fetched_ms) < CACHE_TTL.as_millis() as u64
        {
            return Ok(entry.body.clone());
        }
        let mut args: Vec<String> = vec!["api".into(), "--include".into()];
        if let Some(etag) = cached.as_ref().and_then(|e| e.etag.as_deref()) {
            args.push("-H".into());
            args.push(format!("If-None-Match: {etag}"));
        }
        args.push(path.to_owned());
        let out = self.run(&args)?;
        let response = split_response(&out.stdout);
        let status = response.as_ref().map(|r| r.status);
        match (status, response) {
            (Some(304), _) => {
                let Some(mut entry) = cached else {
                    return Err(GhError::Other {
                        message: "304 without a cached body".into(),
                    });
                };
                entry.fetched_ms = now_ms;
                self.write_entry(&entry);
                Ok(entry.body)
            }
            (Some(200..=299), Some(r)) => {
                self.write_entry(&DiskEntry {
                    path: path.to_owned(),
                    etag: r.header("etag").map(str::to_owned),
                    body: r.body.clone(),
                    fetched_ms: now_ms,
                });
                Ok(r.body)
            }
            (status, response) => Err(self.classify(status, response.as_ref(), &out.stderr)),
        }
    }

    /// A GraphQL query; answers are cached in memory per (query, variables) for `CACHE_TTL`.
    /// String variables go through `-f` (taken literally, `-F` would read `@file`), other values
    /// through `-F`.
    pub fn graphql(&self, query: &str, vars: &Value) -> Result<Value, GhError> {
        let key = format!("{query}\u{0}{vars}");
        let now = self.runner.now();
        if let Ok(cache) = self.graphql_cache.lock()
            && let Some((at, value)) = cache.get(&key)
            && now.saturating_sub(*at) < CACHE_TTL
        {
            return Ok(value.clone());
        }
        let mut args: Vec<String> = vec![
            "api".into(),
            "graphql".into(),
            "-f".into(),
            format!("query={query}"),
        ];
        if let Some(map) = vars.as_object() {
            for (name, value) in map {
                match value {
                    Value::String(s) => {
                        args.push("-f".into());
                        args.push(format!("{name}={s}"));
                    }
                    other => {
                        args.push("-F".into());
                        args.push(format!("{name}={other}"));
                    }
                }
            }
        }
        let out = self.run(&args)?;
        if out.code != Some(0) {
            return Err(self.classify(None, None, &out.stderr));
        }
        let value: Value = serde_json::from_str(&out.stdout).map_err(|e| GhError::Other {
            message: format!("unreadable GraphQL answer: {e}"),
        })?;
        if let Ok(mut cache) = self.graphql_cache.lock() {
            cache.insert(key, (now, value.clone()));
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::{Cell, RefCell};
    use std::collections::VecDeque;

    struct Fake {
        answers: RefCell<VecDeque<io::Result<Output>>>,
        calls: RefCell<Vec<Vec<String>>>,
        clock: Cell<Duration>,
    }

    impl Runner for &Fake {
        fn gh(&self, args: &[String]) -> io::Result<Output> {
            self.calls.borrow_mut().push(args.to_vec());
            self.answers
                .borrow_mut()
                .pop_front()
                .expect("unexpected gh call")
        }
        fn now(&self) -> Duration {
            self.clock.get()
        }
    }

    fn fake(answers: Vec<io::Result<Output>>) -> Fake {
        Fake {
            answers: RefCell::new(answers.into()),
            calls: RefCell::new(vec![]),
            clock: Cell::new(Duration::from_secs(1_000_000)),
        }
    }

    fn out(code: i32, stdout: &str, stderr: &str) -> io::Result<Output> {
        Ok(Output {
            code: Some(code),
            stdout: stdout.into(),
            stderr: stderr.into(),
        })
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-gh-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const OK: &str =
        "HTTP/2.0 200 OK\r\nContent-Type: application/json\r\nEtag: W/\"abc\"\r\n\r\n{\"n\":1}";
    const NOT_MODIFIED: &str = "HTTP/2.0 304 Not Modified\r\nEtag: W/\"abc\"\r\n\r\n";

    fn repo(o: &str, n: &str) -> Option<Repo> {
        Some(Repo {
            owner: o.into(),
            name: n.into(),
        })
    }

    #[test]
    fn remotes_parse_for_ssh_and_https() {
        assert_eq!(
            parse_remote("git@github.com:GravionLabs/poimen.git\n"),
            repo("GravionLabs", "poimen")
        );
        assert_eq!(parse_remote("https://github.com/o/r.git"), repo("o", "r"));
        assert_eq!(parse_remote("https://github.com/o/r"), repo("o", "r"));
        assert_eq!(parse_remote("https://github.com/o/r/"), repo("o", "r"));
        assert_eq!(
            parse_remote("https://tok@github.com/o/r.git"),
            repo("o", "r")
        );
        assert_eq!(parse_remote("ssh://git@github.com/o/r.git"), repo("o", "r"));
        assert_eq!(
            parse_remote("https://github.com/o/my.repo.git"),
            repo("o", "my.repo")
        );
    }

    #[test]
    fn other_hosts_and_malformed_remotes_are_none() {
        assert_eq!(parse_remote("git@gitlab.com:o/r.git"), None);
        assert_eq!(parse_remote("https://github.com.evil.example/o/r"), None);
        assert_eq!(parse_remote("https://example.com/github.com/o/r"), None);
        assert_eq!(parse_remote("https://github.com/o"), None);
        assert_eq!(parse_remote("/local/path/repo"), None);
        assert_eq!(parse_remote(""), None);
    }

    #[test]
    fn a_response_splits_into_status_headers_and_body() {
        let r = split_response(OK).unwrap();
        assert_eq!(r.status, 200);
        assert_eq!(r.header("etag"), Some("W/\"abc\""));
        assert_eq!(r.body, "{\"n\":1}");
        let r = split_response("HTTP/1.1 200 OK\nEtag: x\n\n[1]\n\n[2]").unwrap();
        assert_eq!(r.body, "[1]\n\n[2]");
    }

    #[test]
    fn a_304_has_an_empty_body_and_garbage_is_none() {
        let r = split_response(NOT_MODIFIED).unwrap();
        assert_eq!((r.status, r.body.as_str()), (304, ""));
        assert_eq!(split_response("not http"), None);
        assert_eq!(split_response(""), None);
    }

    #[test]
    fn errors_map_from_recorded_stderr() {
        let rate = split_response(
            "HTTP/2.0 403 Forbidden\r\nX-Ratelimit-Remaining: 0\r\nX-Ratelimit-Reset: 1700000000\r\n\r\n{}",
        )
        .unwrap();
        assert_eq!(
            map_error(
                Some(403),
                Some(&rate),
                "gh: API rate limit exceeded for user ID 1. (HTTP 403)"
            ),
            Some(GhError::RateLimited {
                reset: Some(1_700_000_000)
            })
        );
        assert_eq!(
            map_error(None, None, "gh: API rate limit exceeded (HTTP 403)"),
            Some(GhError::RateLimited { reset: None })
        );
        assert_eq!(
            map_error(None, None, "gh: Not Found (HTTP 404)"),
            Some(GhError::NotFound)
        );
        assert_eq!(
            map_error(
                None,
                None,
                "error connecting to api.github.com\ncheck your internet connection"
            ),
            Some(GhError::Offline)
        );
        assert_eq!(
            map_error(
                None,
                None,
                "Post \"https://api.github.com/graphql\": dial tcp: lookup api.github.com: no such host"
            ),
            Some(GhError::Offline)
        );
        assert_eq!(
            map_error(Some(500), None, "gh: Server Error (HTTP 500)"),
            None
        );
    }

    #[test]
    fn a_missing_gh_is_reported() {
        let f = fake(vec![Err(io::Error::new(io::ErrorKind::NotFound, "x"))]);
        let c = Client::new(&f, temp_dir("missing"));
        assert_eq!(c.rest("repos/o/r"), Err(GhError::GhMissing));
    }

    #[test]
    fn a_failed_login_check_means_not_logged_in() {
        let f = fake(vec![
            out(
                4,
                "",
                "gh: To get started with GitHub CLI, please run:  gh auth login",
            ),
            out(1, "", "You are not logged into any GitHub hosts."),
        ]);
        let c = Client::new(&f, temp_dir("login"));
        assert_eq!(c.rest("repos/o/r"), Err(GhError::NotLoggedIn));
        assert_eq!(f.calls.borrow()[1], ["auth", "status"]);
    }

    #[test]
    fn a_401_with_a_valid_login_is_other() {
        let f = fake(vec![
            out(
                1,
                "HTTP/2.0 401 Unauthorized\r\n\r\n{}",
                "gh: Bad credentials (HTTP 401)",
            ),
            out(0, "", ""),
        ]);
        let c = Client::new(&f, temp_dir("401"));
        assert_eq!(
            c.rest("repos/o/r"),
            Err(GhError::Other {
                message: "gh: Bad credentials (HTTP 401)".into()
            })
        );
    }

    #[test]
    fn errors_serialize_with_a_kind() {
        assert_eq!(
            serde_json::to_value(GhError::GhMissing).unwrap(),
            json!({"kind": "gh-missing"})
        );
        assert_eq!(
            serde_json::to_value(GhError::NotLoggedIn).unwrap(),
            json!({"kind": "not-logged-in"})
        );
        assert_eq!(
            serde_json::to_value(GhError::RateLimited { reset: Some(5) }).unwrap(),
            json!({"kind": "rate-limited", "reset": 5})
        );
        assert_eq!(
            serde_json::to_value(GhError::Other {
                message: "m".into()
            })
            .unwrap(),
            json!({"kind": "other", "message": "m"})
        );
    }

    #[test]
    fn rest_caches_for_30_seconds_then_revalidates_with_the_etag() {
        let dir = temp_dir("etag");
        let f = fake(vec![out(0, OK, ""), out(0, NOT_MODIFIED, "")]);
        let c = Client::new(&f, dir.clone());
        assert_eq!(c.rest("repos/o/r").unwrap(), "{\"n\":1}");
        assert_eq!(f.calls.borrow()[0], ["api", "--include", "repos/o/r"]);

        f.clock.set(f.clock.get() + Duration::from_secs(29));
        assert_eq!(c.rest("repos/o/r").unwrap(), "{\"n\":1}");
        assert_eq!(f.calls.borrow().len(), 1, "served from cache");

        f.clock.set(f.clock.get() + Duration::from_secs(2));
        assert_eq!(c.rest("repos/o/r").unwrap(), "{\"n\":1}");
        assert_eq!(
            f.calls.borrow()[1],
            [
                "api",
                "--include",
                "-H",
                "If-None-Match: W/\"abc\"",
                "repos/o/r"
            ]
        );
        // The 304 renewed the entry.
        f.clock.set(f.clock.get() + Duration::from_secs(10));
        c.rest("repos/o/r").unwrap();
        assert_eq!(f.calls.borrow().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_cached_body_is_only_returned_for_its_own_path() {
        let dir = temp_dir("paths");
        let f = fake(vec![
            out(0, OK, ""),
            out(0, "HTTP/2.0 200 OK\r\n\r\n{\"n\":2}", ""),
        ]);
        let c = Client::new(&f, dir.clone());
        c.rest("repos/o/a").unwrap();
        assert_eq!(c.rest("repos/o/b").unwrap(), "{\"n\":2}");
        assert_eq!(f.calls.borrow()[1], ["api", "--include", "repos/o/b"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_rest_cache_survives_a_new_client_and_stays_in_the_cache_dir() {
        let dir = temp_dir("disk");
        let f = fake(vec![out(0, OK, "")]);
        Client::new(&f, dir.clone()).rest("repos/o/r").unwrap();
        let g = fake(vec![]);
        assert_eq!(
            Client::new(&g, dir.clone()).rest("repos/o/r").unwrap(),
            "{\"n\":1}"
        );
        let names: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
        assert_eq!(names.len(), 1);
        assert!(names[0].file_name().to_string_lossy().ends_with(".json"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rest_rejects_paths_that_look_like_flags() {
        let f = fake(vec![]);
        let client = Client::new(&f, temp_dir("flag"));
        assert!(matches!(
            client.rest("--hostname=evil"),
            Err(GhError::Other { .. })
        ));
        assert!(f.calls.borrow().is_empty());
    }

    #[test]
    fn rest_errors_are_not_cached_and_not_found_maps() {
        let dir = temp_dir("404");
        let f = fake(vec![out(
            1,
            "HTTP/2.0 404 Not Found\r\n\r\n{}",
            "gh: Not Found (HTTP 404)",
        )]);
        let c = Client::new(&f, dir.clone());
        assert_eq!(c.rest("repos/o/x"), Err(GhError::NotFound));
        assert!(!dir.exists() || std::fs::read_dir(&dir).unwrap().next().is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn graphql_passes_variables_and_caches_per_query_and_vars() {
        let f = fake(vec![
            out(0, "{\"data\":1}", ""),
            out(0, "{\"data\":2}", ""),
            out(0, "{\"data\":3}", ""),
        ]);
        let c = Client::new(&f, temp_dir("gql"));
        let vars = json!({"owner": "o", "first": 5});
        assert_eq!(c.graphql("query{a}", &vars).unwrap(), json!({"data": 1}));
        assert_eq!(
            f.calls.borrow()[0],
            [
                "api",
                "graphql",
                "-f",
                "query=query{a}",
                "-F",
                "first=5",
                "-f",
                "owner=o"
            ]
        );
        assert_eq!(c.graphql("query{a}", &vars).unwrap(), json!({"data": 1}));
        assert_eq!(f.calls.borrow().len(), 1);
        // Other variables are another entry.
        assert_eq!(
            c.graphql("query{a}", &json!({"owner": "p"})).unwrap(),
            json!({"data": 2})
        );
        // Expiry.
        f.clock.set(f.clock.get() + CACHE_TTL);
        assert_eq!(c.graphql("query{a}", &vars).unwrap(), json!({"data": 3}));
    }

    #[test]
    fn graphql_failures_map_to_errors() {
        let f = fake(vec![out(1, "", "error connecting to api.github.com")]);
        let c = Client::new(&f, temp_dir("gqlerr"));
        assert_eq!(c.graphql("q", &json!({})), Err(GhError::Offline));
    }
}
