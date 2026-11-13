//! A repository under `reposDir` (ADR 0008, `repo.ensure` of `poimen-server`): cloned with
//! `gh repo clone` when its path is absent, fetched when the path holds a clone of it, and an
//! error naming the path otherwise. A clone never goes into an existing directory, and nothing is
//! written into a directory that is not a clone of that repository.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Serialize;
use serde_json::Value;

use crate::github::{Output, parse_remote};
use crate::gui_path;

/// Where a repository goes when the user config sets no `reposDir`.
pub const DEFAULT_REPOS_DIR: &str = "~/Source/{owner}/{repo}";

/// Runs `gh`; faked in tests, so no test needs the network.
pub trait Gh: Send + Sync {
    /// Runs `gh` with `args`. `ErrorKind::NotFound` means `gh` is not installed.
    fn run(&self, args: &[String]) -> io::Result<Output>;
}

/// The real `gh` on the login-shell `PATH`, without a time limit (a clone may take long) and
/// without prompts.
pub struct SystemGh;

impl Gh for SystemGh {
    fn run(&self, args: &[String]) -> io::Result<Output> {
        let path = gui_path::child_path();
        let program = gui_path::resolve_program("gh", &path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "gh not found"))?;
        let out = gui_path::command(program)
            .args(args)
            .env("PATH", &path)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("NO_COLOR", "1")
            .stdin(Stdio::null())
            .output()?;
        Ok(Output {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

/// What `ensure` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Ensured {
    pub path: String,
    /// `true` when the repository was cloned, `false` when an existing clone was fetched.
    pub cloned: bool,
}

/// Why `ensure` did nothing or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureError {
    /// `owner`, `repo` or `reposDir` cannot name a path.
    Invalid(String),
    /// The path exists and is not a clone of the repository; nothing was written.
    Occupied(String),
    /// `gh repo clone` or `git fetch` failed.
    Failed(String),
}

impl std::fmt::Display for EnsureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(m) | Self::Occupied(m) | Self::Failed(m) => f.write_str(m),
        }
    }
}

/// `reposDir` of the user config: a non-empty string, else [`DEFAULT_REPOS_DIR`].
pub fn repos_dir(config: &Value) -> String {
    match config.get("reposDir") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.trim().to_owned(),
        _ => DEFAULT_REPOS_DIR.to_owned(),
    }
}

/// `reposDir` of `~/.config/poimen/config.json` under `home`; a missing or invalid file gives the
/// default.
pub fn user_repos_dir(home: Option<&Path>) -> String {
    let config = home
        .and_then(|h| std::fs::read_to_string(crate::settings::config_path(h)).ok())
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or(Value::Null);
    repos_dir(&config)
}

/// A GitHub owner or repository name: letters, digits, `-`, `_` and `.`, not `.` or `..`.
fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The path of `owner/repo`: `template` with `{owner}` and `{repo}` filled in and a leading `~`
/// replaced by `home`. The result must be absolute.
pub fn target(
    template: &str,
    home: Option<&Path>,
    owner: &str,
    repo: &str,
) -> Result<PathBuf, EnsureError> {
    if !valid_name(owner) || !valid_name(repo) {
        return Err(EnsureError::Invalid(format!(
            "{owner:?}/{repo:?} is not a GitHub repository name"
        )));
    }
    let expanded = template.replace("{owner}", owner).replace("{repo}", repo);
    let path = if expanded == "~" {
        home_or_err(home)?
    } else if let Some(rest) = expanded
        .strip_prefix("~/")
        .or_else(|| expanded.strip_prefix("~\\"))
    {
        home_or_err(home)?.join(rest)
    } else {
        PathBuf::from(&expanded)
    };
    if !path.is_absolute() {
        return Err(EnsureError::Invalid(format!(
            "reposDir {template:?} must be an absolute path or start with ~"
        )));
    }
    // Rebuilt from its components, so a template written with `/` gets the platform's
    // separator: on Windows `~/Source/o/r` must not become `C:\Users\me\Source/o/r`.
    Ok(path.components().collect())
}

fn home_or_err(home: Option<&Path>) -> Result<PathBuf, EnsureError> {
    home.map(Path::to_path_buf)
        .ok_or_else(|| EnsureError::Invalid("reposDir starts with ~ and there is no home".into()))
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = gui_path::command("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("git could not be started: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    } else {
        Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// Whether `path` is the top level of a git clone whose `origin` is `owner/repo` on GitHub; the
/// error says what the path is instead. Only reads.
fn check_clone(path: &Path, owner: &str, repo: &str) -> Result<(), String> {
    let shown = path.display();
    if !path.is_dir() {
        return Err(format!("{shown} exists and is not a directory"));
    }
    let top = git(path, &["rev-parse", "--show-toplevel"])
        .map_err(|_| format!("{shown} exists and is not a git clone"))?;
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if !same(Path::new(&top), path) {
        return Err(format!(
            "{shown} exists and is inside the git clone {top}, not a clone of its own"
        ));
    }
    // The configured URL, not `remote get-url` (which applies `insteadOf` rewrites).
    let url = git(path, &["config", "--get", "remote.origin.url"])
        .map_err(|_| format!("{shown} exists and is a git clone without an origin"))?;
    match parse_remote(&url) {
        Some(r) if r.owner.eq_ignore_ascii_case(owner) && r.name.eq_ignore_ascii_case(repo) => {
            Ok(())
        }
        _ => Err(format!(
            "{shown} exists and is a clone of {url}, not of {owner}/{repo}"
        )),
    }
}

/// Clones `owner/repo` into its path under `template` when the path is absent (`gh repo clone`),
/// fetches `origin` when the path is a clone of it, and refuses anything else at the path.
pub fn ensure(
    gh: &dyn Gh,
    home: Option<&Path>,
    template: &str,
    owner: &str,
    repo: &str,
) -> Result<Ensured, EnsureError> {
    let path = target(template, home, owner, repo)?;
    let path_str = path.to_string_lossy().into_owned();
    // `symlink_metadata`: a dangling link is something at the path too.
    if std::fs::symlink_metadata(&path).is_ok() {
        check_clone(&path, owner, repo).map_err(EnsureError::Occupied)?;
        git(&path, &["fetch", "origin"]).map_err(EnsureError::Failed)?;
        return Ok(Ensured {
            path: path_str,
            cloned: false,
        });
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            EnsureError::Failed(format!("{} could not be created: {e}", parent.display()))
        })?;
    }
    let args = [
        "repo".to_owned(),
        "clone".to_owned(),
        format!("{owner}/{repo}"),
        path_str.clone(),
    ];
    match gh.run(&args) {
        Ok(out) if out.code == Some(0) => Ok(Ensured {
            path: path_str,
            cloned: true,
        }),
        Ok(out) => Err(EnsureError::Failed(format!(
            "gh repo clone {owner}/{repo} failed: {}",
            out.stderr.trim()
        ))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            Err(EnsureError::Failed("gh is not installed".into()))
        }
        Err(e) => Err(EnsureError::Failed(format!("gh could not be started: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn repos_dir_defaults_and_reads_the_user_value() {
        assert_eq!(repos_dir(&Value::Null), DEFAULT_REPOS_DIR);
        assert_eq!(repos_dir(&json!({"reposDir": "  "})), DEFAULT_REPOS_DIR);
        assert_eq!(repos_dir(&json!({"reposDir": 3})), DEFAULT_REPOS_DIR);
        assert_eq!(
            repos_dir(&json!({"reposDir": "/srv/{owner}-{repo}"})),
            "/srv/{owner}-{repo}"
        );
    }

    #[test]
    fn target_expands_home_owner_and_repo() {
        // An absolute path on every OS.
        let home = std::env::temp_dir();
        assert_eq!(
            target(DEFAULT_REPOS_DIR, Some(&home), "GravionLabs", "poimen").unwrap(),
            home.join("Source/GravionLabs/poimen")
        );
    }

    #[test]
    fn target_refuses_bad_names_relative_paths_and_a_missing_home() {
        for (owner, repo) in [("..", "x"), ("o", ""), ("o/x", "r"), ("o", "r r")] {
            assert!(
                matches!(
                    target(DEFAULT_REPOS_DIR, Some(Path::new("/h")), owner, repo),
                    Err(EnsureError::Invalid(_))
                ),
                "{owner}/{repo}"
            );
        }
        assert!(matches!(
            target("src/{repo}", Some(Path::new("/h")), "o", "r"),
            Err(EnsureError::Invalid(_))
        ));
        assert!(matches!(
            target(DEFAULT_REPOS_DIR, None, "o", "r"),
            Err(EnsureError::Invalid(_))
        ));
    }
}
