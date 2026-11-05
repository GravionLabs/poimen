//! Optional git actions of the Changes view for the case that the agent did not commit: stage,
//! unstage, commit and push the current branch. All git through argv (no shell), paths after
//! `--`, the message as one argv element; never forced, never prompting, with a timeout.

use std::io::Read;
use std::path::{Component, Path};
use std::process::Stdio;
use std::time::{Duration, Instant};

use crate::changes::validate;
use crate::git::run_bytes;

/// Longest a push may take.
const PUSH_TIMEOUT: Duration = Duration::from_secs(120);

/// A path of the repository, relative and without `..`: nothing outside the repo.
fn check_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    let ok = !path.is_empty()
        && !path.contains('\0')
        && p.components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
    if ok {
        Ok(())
    } else {
        Err(format!("{path} is not a path inside the repository"))
    }
}

fn stage_in(top: &str, paths: &[String], stage: bool) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    for p in paths {
        check_path(p)?;
    }
    // Literal pathspecs: `*.txt`, `:(top)x` or `:!x` name files, never patterns.
    let mut args: Vec<&str> = if stage {
        vec!["--literal-pathspecs", "add", "-A", "--"]
    } else {
        vec!["--literal-pathspecs", "restore", "--staged", "--"]
    };
    args.extend(paths.iter().map(String::as_str));
    run_bytes(top, &args).map(|_| ())
}

fn commit_in(top: &str, message: &str) -> Result<String, String> {
    if message.trim().is_empty() {
        return Err("The commit message is empty".into());
    }
    let nothing_staged = run_bytes(top, &["diff", "--cached", "--name-only", "-z"])?.is_empty();
    if nothing_staged {
        return Err("Nothing is staged".into());
    }
    run_git(top, &["commit", "-m", message], PUSH_TIMEOUT, "commit")
}

/// Branches a push from here is refused on.
fn protected(branch: &str) -> bool {
    branch == "main" || branch.starts_with("release/")
}

/// Replaces credentials in URLs (`scheme://user:secret@host`) of git output.
fn redact(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find("://") {
        let (head, tail) = rest.split_at(i + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(tail.len());
        match tail[..end].rfind('@') {
            Some(at) => {
                out.push_str("***");
                rest = &tail[at..];
            }
            None => rest = tail,
        }
    }
    out.push_str(rest);
    out
}

fn drain(mut pipe: impl Read + Send + 'static) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        buf
    })
}

/// Runs git with no prompts and a deadline; returns the combined output.
fn run_git(top: &str, args: &[&str], timeout: Duration, what: &str) -> Result<String, String> {
    let mut cmd = crate::gui_path::command("git");
    cmd.args(["-C", top]).args(args);
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_ASKPASS", "true");
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("git could not be started: {e}"))?;
    let out = drain(child.stdout.take().expect("piped"));
    let err = drain(child.stderr.take().expect("piped"));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(format!("git failed: {e}")),
        }
    };
    let out = String::from_utf8_lossy(&out.join().unwrap_or_default()).into_owned();
    let err = String::from_utf8_lossy(&err.join().unwrap_or_default()).into_owned();
    let text = redact(format!("{}\n{}", out.trim(), err.trim()).trim());
    match status {
        None => Err(format!(
            "The {what} timed out after {} s and was stopped.\n{text}",
            timeout.as_secs().max(1)
        )
        .trim()
        .to_owned()),
        Some(s) if s.success() => Ok(text),
        Some(_) => {
            let lower = text.to_lowercase();
            let hint = if lower.contains("terminal prompts disabled")
                || lower.contains("could not read username")
                || lower.contains("permission denied")
                || lower.contains("authentication failed")
            {
                "\nPoimen cannot ask for credentials: set up a credential helper or an SSH key."
            } else {
                ""
            };
            Err(format!("{text}{hint}"))
        }
    }
}

fn push_in(top: &str, timeout: Duration) -> Result<String, String> {
    // The full ref, so a tag or branch named like another ref cannot change the name.
    let head = run_bytes(top, &["symbolic-ref", "-q", "HEAD"])
        .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
        .unwrap_or_default();
    let Some(branch) = head.strip_prefix("refs/heads/").filter(|b| !b.is_empty()) else {
        return Err("Refused: HEAD is detached, check out a branch first".into());
    };
    if protected(branch) {
        return Err(format!(
            "Refused: poimen does not push {branch} (main and release/* are protected)"
        ));
    }
    let config = |key: &str| {
        run_bytes(top, &["config", "--get", &format!("branch.{branch}.{key}")])
            .map(|b| String::from_utf8_lossy(&b).trim().to_owned())
            .unwrap_or_default()
    };
    let merge = config("merge");
    let has_upstream = !merge.is_empty();
    if has_upstream {
        let remote = config("remote");
        let target = merge.strip_prefix("refs/heads/").unwrap_or(&merge);
        if remote != "origin" {
            return Err(format!(
                "Refused: {branch} tracks {remote}/{target}; poimen pushes to origin only"
            ));
        }
        if target != branch {
            return Err(format!(
                "Refused: {branch} tracks origin/{target}, a different branch; poimen pushes a branch only to the same name"
            ));
        }
    }
    // Always explicit, so push.default and remote.*.push (even a `+` refspec) are not used.
    let refspec = format!("HEAD:refs/heads/{branch}");
    let mut args = vec![
        "-c",
        "push.default=simple",
        "-c",
        "push.followTags=false",
        "push",
    ];
    if !has_upstream {
        args.push("-u");
    }
    args.extend(["origin", &refspec]);
    run_git(top, &args, timeout, "push")
}

pub fn stage(dir: &str, paths: &[String], stage: bool, home: Option<&Path>) -> Result<(), String> {
    stage_in(&validate(dir, home)?, paths, stage)
}

pub fn commit(dir: &str, message: &str, home: Option<&Path>) -> Result<String, String> {
    commit_in(&validate(dir, home)?, message)
}

pub fn push(dir: &str, home: Option<&Path>) -> Result<String, String> {
    push_in(&validate(dir, home)?, PUSH_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};

    static N: AtomicU32 = AtomicU32::new(0);

    fn git(dir: &Path, args: &[&str]) -> String {
        let out = crate::gui_path::command("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// A bare `origin` and a clone `app` with one commit on `main`, on branch `feat/x`.
    struct Fixture {
        base: PathBuf,
        app: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let base = std::env::temp_dir().join(format!(
                "poimen-changes-git-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = std::fs::remove_dir_all(&base);
            std::fs::create_dir_all(&base).unwrap();
            let base = base.canonicalize().unwrap();
            git(&base, &["init", "--bare", "-b", "main", "origin.git"]);
            let origin = base.join("origin.git");
            git(&base, &["init", "-b", "main", "app"]);
            let app = base.join("app");
            git(&app, &["config", "user.name", "t"]);
            git(&app, &["config", "user.email", "t@example.com"]);
            git(&app, &["config", "commit.gpgsign", "false"]);
            git(&app, &["remote", "add", "origin", origin.to_str().unwrap()]);
            std::fs::write(app.join("a.txt"), "a\n").unwrap();
            git(&app, &["add", "."]);
            git(&app, &["commit", "-m", "init"]);
            git(&app, &["checkout", "-b", "feat/x"]);
            Self { base, app }
        }

        fn top(&self) -> &str {
            self.app.to_str().unwrap()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.base);
        }
    }

    fn staged_of(f: &Fixture) -> Vec<String> {
        crate::changes::list(f.top(), crate::changes::Scope::Uncommitted, None)
            .unwrap()
            .files
            .into_iter()
            .filter(|c| c.staged)
            .map(|c| c.path)
            .collect()
    }

    fn v(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| (*s).to_owned()).collect()
    }

    const SECS: Duration = Duration::from_secs(30);

    #[test]
    fn stages_and_unstages_a_file() {
        let f = Fixture::new();
        std::fs::write(f.app.join("a.txt"), "changed\n").unwrap();
        std::fs::write(f.app.join("new.txt"), "new\n").unwrap();
        assert!(staged_of(&f).is_empty());
        stage(f.top(), &v(&["a.txt", "new.txt"]), true, None).unwrap();
        assert_eq!(staged_of(&f), v(&["a.txt", "new.txt"]));
        stage(f.top(), &v(&["new.txt"]), false, None).unwrap();
        assert_eq!(staged_of(&f), v(&["a.txt"]));
    }

    #[test]
    fn stages_a_deletion_and_odd_names() {
        let f = Fixture::new();
        std::fs::remove_file(f.app.join("a.txt")).unwrap();
        std::fs::write(f.app.join("-rf x.txt"), "x\n").unwrap();
        stage(f.top(), &v(&["a.txt", "-rf x.txt"]), true, None).unwrap();
        assert_eq!(staged_of(&f), v(&["-rf x.txt", "a.txt"]));
    }

    #[test]
    fn refuses_paths_outside_the_repository() {
        let f = Fixture::new();
        for bad in ["../x", "/etc/passwd", "a/../../x", ""] {
            let err = stage(f.top(), &v(&[bad]), true, None).unwrap_err();
            assert!(err.contains("not a path inside"), "{bad}: {err}");
        }
    }

    #[test]
    fn commits_the_staged_changes_with_the_message_as_one_argument() {
        let f = Fixture::new();
        std::fs::write(f.app.join("a.txt"), "changed\n").unwrap();
        std::fs::write(f.app.join("b.txt"), "b\n").unwrap();
        stage(f.top(), &v(&["a.txt"]), true, None).unwrap();
        commit(f.top(), "feat: x; touch pwned\n\nbody \"quoted\"", None).unwrap();
        assert_eq!(
            git(&f.app, &["log", "-1", "--format=%B"]),
            "feat: x; touch pwned\n\nbody \"quoted\""
        );
        assert_eq!(
            git(&f.app, &["show", "--name-only", "--format=", "HEAD"]),
            "a.txt"
        );
        assert!(!f.app.join("pwned").exists());
        assert_eq!(git(&f.app, &["status", "--porcelain"]), "?? b.txt");
    }

    #[test]
    fn commit_needs_a_message_and_something_staged() {
        let f = Fixture::new();
        std::fs::write(f.app.join("a.txt"), "changed\n").unwrap();
        assert_eq!(
            commit(f.top(), "msg", None).unwrap_err(),
            "Nothing is staged"
        );
        stage(f.top(), &v(&["a.txt"]), true, None).unwrap();
        assert_eq!(
            commit(f.top(), "  \n", None).unwrap_err(),
            "The commit message is empty"
        );
        assert_eq!(git(&f.app, &["rev-list", "--count", "HEAD"]), "1");
    }

    #[test]
    fn commit_is_allowed_on_main() {
        let f = Fixture::new();
        git(&f.app, &["checkout", "main"]);
        std::fs::write(f.app.join("a.txt"), "changed\n").unwrap();
        stage(f.top(), &v(&["a.txt"]), true, None).unwrap();
        commit(f.top(), "fix: on main", None).unwrap();
        assert_eq!(git(&f.app, &["rev-list", "--count", "HEAD"]), "2");
    }

    #[test]
    fn pushes_with_upstream_then_plain() {
        let f = Fixture::new();
        push_in(f.top(), SECS).unwrap();
        assert_eq!(
            git(&f.app, &["rev-parse", "--abbrev-ref", "@{u}"]),
            "origin/feat/x"
        );
        let origin = f.base.join("origin.git");
        assert_eq!(
            git(&origin, &["rev-parse", "feat/x"]),
            git(&f.app, &["rev-parse", "HEAD"])
        );
        std::fs::write(f.app.join("a.txt"), "more\n").unwrap();
        stage(f.top(), &v(&["a.txt"]), true, None).unwrap();
        commit(f.top(), "feat: more", None).unwrap();
        push_in(f.top(), SECS).unwrap();
        assert_eq!(
            git(&origin, &["rev-parse", "feat/x"]),
            git(&f.app, &["rev-parse", "HEAD"])
        );
    }

    #[test]
    fn push_is_refused_on_protected_branches_and_detached_head() {
        let f = Fixture::new();
        for branch in ["main", "release/next"] {
            git(&f.app, &["checkout", "-B", branch]);
            let err = push_in(f.top(), SECS).unwrap_err();
            assert!(err.starts_with("Refused"), "{branch}: {err}");
        }
        git(&f.app, &["checkout", "--detach"]);
        let err = push_in(f.top(), SECS).unwrap_err();
        assert!(err.contains("detached"), "{err}");
        let origin = f.base.join("origin.git");
        assert_eq!(git(&origin, &["branch", "--list"]), "");
    }

    #[test]
    fn push_never_forces() {
        let f = Fixture::new();
        push_in(f.top(), SECS).unwrap();
        git(
            &f.app,
            &["commit", "--allow-empty", "--amend", "-m", "rewritten"],
        );
        let err = push_in(f.top(), SECS).unwrap_err();
        assert!(err.to_lowercase().contains("rejected"), "{err}");
    }

    #[test]
    fn push_ignores_configured_targets_and_refspecs() {
        let f = Fixture::new();
        let origin = f.base.join("origin.git");
        // Upstream of feat/x is origin/main with push.default=upstream: refused, main untouched.
        git(&f.app, &["push", "origin", "main"]);
        let main_before = git(&origin, &["rev-parse", "main"]);
        git(&f.app, &["config", "branch.feat/x.remote", "origin"]);
        git(
            &f.app,
            &["config", "branch.feat/x.merge", "refs/heads/main"],
        );
        git(&f.app, &["config", "push.default", "upstream"]);
        git(&f.app, &["commit", "--allow-empty", "-m", "work"]);
        let err = push_in(f.top(), SECS).unwrap_err();
        assert!(
            err.starts_with("Refused") && err.contains("origin/main"),
            "{err}"
        );
        assert_eq!(git(&origin, &["rev-parse", "main"]), main_before);
        // A forcing wildcard refspec in config pushes only feat/x, without force.
        git(&f.app, &["config", "--unset", "branch.feat/x.merge"]);
        git(&f.app, &["config", "--unset", "branch.feat/x.remote"]);
        git(
            &f.app,
            &["config", "remote.origin.push", "+refs/heads/*:refs/heads/*"],
        );
        push_in(f.top(), SECS).unwrap();
        assert_eq!(git(&origin, &["rev-parse", "main"]), main_before);
        assert_eq!(
            git(&origin, &["rev-parse", "feat/x"]),
            git(&f.app, &["rev-parse", "HEAD"])
        );
        git(
            &f.app,
            &["commit", "--allow-empty", "--amend", "-m", "rewritten"],
        );
        let err = push_in(f.top(), SECS).unwrap_err();
        assert!(err.to_lowercase().contains("rejected"), "{err}");
    }

    #[test]
    fn a_tag_named_like_the_branch_does_not_hide_main() {
        let f = Fixture::new();
        git(&f.app, &["checkout", "main"]);
        git(&f.app, &["tag", "heads/main"]);
        let err = push_in(f.top(), SECS).unwrap_err();
        assert!(err.starts_with("Refused"), "{err}");
    }

    #[test]
    fn staging_uses_literal_pathspecs() {
        let f = Fixture::new();
        std::fs::write(f.app.join("b.txt"), "b\n").unwrap();
        assert!(stage(f.top(), &v(&["*.txt"]), true, None).is_err());
        assert!(stage(f.top(), &v(&[":(top)b.txt"]), true, None).is_err());
        assert!(staged_of(&f).is_empty());
        stage(f.top(), &v(&["b.txt"]), true, None).unwrap();
        assert_eq!(staged_of(&f), vec!["b.txt".to_owned()]);
    }

    #[test]
    fn push_without_a_remote_reports_the_error() {
        let f = Fixture::new();
        git(&f.app, &["remote", "remove", "origin"]);
        assert!(push_in(f.top(), SECS).is_err());
    }

    #[test]
    #[cfg(unix)] // the hook is a shell script
    fn a_slow_push_is_stopped_at_the_timeout() {
        let f = Fixture::new();
        let hook = f.base.join("origin.git/hooks/pre-receive");
        std::fs::write(&hook, "#!/bin/sh\nsleep 5\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let err = push_in(f.top(), Duration::from_millis(500)).unwrap_err();
        assert!(err.contains("timed out"), "{err}");
    }

    #[test]
    fn redacts_credentials_in_urls() {
        assert_eq!(
            redact("fatal: https://user:tok@github.com/o/r.git failed, see https://x.io/a@b"),
            "fatal: https://***@github.com/o/r.git failed, see https://x.io/a@b"
        );
    }
}
