//! Read-only context of a directory's repository: the branch and whether it is a linked worktree.
//! herdr's pane list carries no branch, so the sidebar asks the repository itself.

use std::collections::HashMap;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoInfo {
    /// Branch name; `None` on a detached HEAD.
    pub branch: Option<String>,
    /// True for a linked worktree (not the main checkout).
    pub worktree: bool,
    /// Canonical common git dir, shared by the main checkout and its linked worktrees.
    pub common_dir: String,
    /// Changed plus untracked files; `None` when `git status` failed or timed out.
    pub dirty: Option<u32>,
    /// Commits ahead of the upstream; `None` without upstream or when unknown.
    pub ahead: Option<u32>,
    /// Commits behind the upstream; `None` without upstream or when unknown.
    pub behind: Option<u32>,
}

/// Working tree state read from `git status --porcelain=v2 --branch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub dirty: u32,
    pub ahead_behind: Option<(u32, u32)>,
}

/// Longest a `git status` may run before it is cancelled.
const STATUS_TIMEOUT: Duration = Duration::from_secs(2);

/// Parses `git status --porcelain=v2 --branch`: entry lines (`1`, `2`, `u`, `?`) count as dirty,
/// `# branch.ab +A -B` gives ahead and behind (absent without upstream).
pub fn parse_status(output: &str) -> Status {
    let mut dirty = 0;
    let mut ahead_behind = None;
    for line in output.lines() {
        if let Some(ab) = line.strip_prefix("# branch.ab ") {
            let mut parts = ab.split_whitespace();
            let a = parts.next().and_then(|p| p.strip_prefix('+')?.parse().ok());
            let b = parts.next().and_then(|p| p.strip_prefix('-')?.parse().ok());
            ahead_behind = a.zip(b);
        } else if matches!(line.as_bytes().first(), Some(b'1' | b'2' | b'u' | b'?')) {
            dirty += 1;
        }
    }
    Status {
        dirty,
        ahead_behind,
    }
}

/// Runs `git status` in a directory; `None` on failure or when `timeout` passes (child killed).
pub trait StatusRunner {
    fn status(&self, dir: &str, timeout: Duration) -> Option<String>;
}

struct GitRunner;

impl StatusRunner for GitRunner {
    fn status(&self, dir: &str, timeout: Duration) -> Option<String> {
        let mut child = crate::gui_path::command("git")
            .args(["-C", dir, "status", "--porcelain=v2", "--branch"])
            .args(["--no-renames", "--untracked-files=normal"])
            .env("GIT_OPTIONAL_LOCKS", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let reader = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stdout.read_to_string(&mut text);
            text
        });
        let deadline = Instant::now() + timeout;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
            }
        };
        let text = reader.join().ok()?;
        status.success().then_some(text)
    }
}

/// The working tree state of `dir`, `None` when unknown (failure or timeout).
pub fn status_with(runner: &dyn StatusRunner, dir: &str, timeout: Duration) -> Option<Status> {
    runner.status(dir, timeout).map(|out| parse_status(&out))
}

fn apply_status(info: &mut RepoInfo, status: Option<Status>) {
    if let Some(s) = status {
        info.dirty = Some(s.dirty);
        info.ahead = s.ahead_behind.map(|(a, _)| a);
        info.behind = s.ahead_behind.map(|(_, b)| b);
    }
}

/// Parses the output of `rev-parse --abbrev-ref HEAD --absolute-git-dir --git-common-dir`.
pub fn parse_rev_parse(output: &str) -> Option<RepoInfo> {
    let mut lines = output.lines();
    let head = lines.next()?.trim();
    let git_dir = lines.next()?.trim();
    let common = lines.next()?.trim();
    if head.is_empty() {
        return None;
    }
    Some(RepoInfo {
        branch: (head != "HEAD").then(|| head.to_string()),
        worktree: git_dir != common,
        common_dir: common.to_string(),
        dirty: None,
        ahead: None,
        behind: None,
    })
}

pub fn info(dir: &str) -> Option<RepoInfo> {
    let out = crate::gui_path::command("git")
        .args(["-C", dir, "rev-parse", "--abbrev-ref", "HEAD"])
        .args([
            "--absolute-git-dir",
            "--path-format=absolute",
            "--git-common-dir",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut info = parse_rev_parse(&String::from_utf8_lossy(&out.stdout))?;
    if let Ok(real) = std::fs::canonicalize(&info.common_dir) {
        info.common_dir = real.to_string_lossy().into_owned();
    }
    apply_status(&mut info, status_with(&GitRunner, dir, STATUS_TIMEOUT));
    Some(info)
}

/// The top-level directory of the repository containing `dir`; `None` outside a repository.
pub fn toplevel(dir: &str) -> Option<String> {
    let out = crate::gui_path::command("git")
        .args(["-C", dir, "rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let top = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!top.is_empty()).then_some(top)
}

/// The common git dir (absolute) of the repository containing `dir`, shared by its linked
/// worktrees; `None` outside a repository.
pub fn common_dir(dir: &str) -> Option<String> {
    let out = crate::gui_path::command("git")
        .args([
            "-C",
            dir,
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let common = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!common.is_empty()).then_some(common)
}

/// Parses `rev-parse --show-toplevel --git-common-dir` (absolute paths): the toplevel and the
/// common git dir that identifies the repository across its linked worktrees.
pub fn parse_roots(output: &str) -> Option<(String, String)> {
    let mut lines = output.lines();
    let top = lines.next()?.trim();
    let common = lines.next()?.trim();
    (!top.is_empty() && !common.is_empty()).then(|| (top.to_owned(), common.to_owned()))
}

/// The toplevel and the common git dir (absolute) of the repository containing `dir`; `None`
/// outside a repository.
pub fn roots(dir: &str) -> Option<(String, String)> {
    let out = crate::gui_path::command("git")
        .args(["-C", dir, "rev-parse", "--path-format=absolute"])
        .args(["--show-toplevel", "--git-common-dir"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_roots(&String::from_utf8_lossy(&out.stdout))
}

/// The distinct repositories among `dirs` (a linked worktree counts as its main repository), each
/// by the toplevel of the first directory that belongs to it; directories outside one are skipped.
pub fn repo_roots(dirs: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut roots = Vec::new();
    for dir in dirs {
        if let Some((top, common)) = self::roots(dir)
            && seen.insert(common)
        {
            roots.push(top);
        }
    }
    roots
}

/// Repository info per directory; directories outside a repository are left out.
pub fn infos(dirs: Vec<String>) -> HashMap<String, RepoInfo> {
    dirs.into_iter()
        .filter_map(|dir| info(&dir).map(|i| (dir, i)))
        .collect()
}

/// Runs `git -C dir <args>` (argv, no shell) and returns its stdout bytes.
pub fn run_bytes(dir: &str, args: &[&str]) -> Result<Vec<u8>, String> {
    let out = crate::gui_path::command("git")
        .args(["-C", dir])
        .args(args)
        .output()
        .map_err(|e| format!("git could not be started: {e}"))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

/// The URL of the remote `origin`; `None` outside a repository or without that remote.
pub fn origin_url(dir: &str) -> Option<String> {
    let out = run_bytes(dir, &["config", "--get", "remote.origin.url"]).ok()?;
    let url = String::from_utf8_lossy(&out).trim().to_owned();
    (!url.is_empty()).then_some(url)
}

/// The ref work is based on, without fetching: `origin/release/next` if it exists, else the
/// default branch of `origin` (`refs/remotes/origin/HEAD`); `None` when there is neither.
pub fn base_ref(dir: &str) -> Option<String> {
    if run_bytes(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            "refs/remotes/origin/release/next",
        ],
    )
    .is_ok()
    {
        return Some("origin/release/next".into());
    }
    let head = run_bytes(dir, &["symbolic-ref", "refs/remotes/origin/HEAD"]).ok()?;
    let head = String::from_utf8_lossy(&head).trim().to_owned();
    let name = head.strip_prefix("refs/remotes/").unwrap_or(&head);
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_branch_in_the_main_checkout() {
        let out = "main\n/r/.git\n/r/.git\n";
        assert_eq!(
            parse_rev_parse(out),
            Some(RepoInfo {
                branch: Some("main".into()),
                worktree: false,
                common_dir: "/r/.git".into(),
                dirty: None,
                ahead: None,
                behind: None
            })
        );
    }

    #[test]
    fn detects_a_linked_worktree() {
        let out = "feat/x\n/r/.git/worktrees/x\n/r/.git\n";
        assert_eq!(
            parse_rev_parse(out),
            Some(RepoInfo {
                branch: Some("feat/x".into()),
                worktree: true,
                common_dir: "/r/.git".into(),
                dirty: None,
                ahead: None,
                behind: None
            })
        );
    }

    #[test]
    fn detached_head_has_no_branch() {
        let out = "HEAD\n/r/.git\n/r/.git\n";
        assert_eq!(parse_rev_parse(out).unwrap().branch, None);
    }

    #[test]
    fn rejects_incomplete_output() {
        assert_eq!(parse_rev_parse(""), None);
        assert_eq!(parse_rev_parse("main\n"), None);
    }

    #[test]
    fn roots_need_a_toplevel_and_a_common_dir() {
        assert_eq!(
            parse_roots("/r\n/r/.git\n"),
            Some(("/r".into(), "/r/.git".into()))
        );
        assert_eq!(parse_roots("/r\n"), None);
        assert_eq!(parse_roots(""), None);
    }

    #[test]
    fn no_roots_outside_a_repository() {
        assert!(repo_roots(&["/nonexistent-poimen-dir".to_owned()]).is_empty());
    }

    #[test]
    fn a_linked_worktree_shares_the_common_dir_of_its_main_checkout() {
        let tmp = std::env::temp_dir().join(format!("poimen-git-709-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let main = tmp.join("main");
        let wt = tmp.join("wt");
        std::fs::create_dir_all(&main).unwrap();
        let git = |dir: &std::path::Path, args: &[&str]| {
            let ok = crate::gui_path::command("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git(&main, &["init", "-q", "-b", "main"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "i"]);
        git(
            &main,
            &["worktree", "add", "-q", "-b", "feat", wt.to_str().unwrap()],
        );
        let a = info(main.to_str().unwrap()).unwrap();
        let b = info(wt.to_str().unwrap()).unwrap();
        let _ = std::fs::remove_dir_all(&tmp);
        assert!(!a.worktree && b.worktree);
        assert_eq!(a.common_dir, b.common_dir);
        assert!(a.common_dir.ends_with(".git"));
    }

    #[test]
    fn outside_a_repository_is_none() {
        assert_eq!(info("/nonexistent-poimen-dir"), None);
    }

    const CLEAN: &str =
        "# branch.oid abc\n# branch.head main\n# branch.upstream origin/main\n# branch.ab +0 -0\n";

    #[test]
    fn clean_tree_is_not_dirty() {
        assert_eq!(
            parse_status(CLEAN),
            Status {
                dirty: 0,
                ahead_behind: Some((0, 0))
            }
        );
    }

    #[test]
    fn counts_changed_and_untracked_files() {
        let out = format!(
            "{CLEAN}1 .M N... 100644 100644 100644 a b src/a.rs\n2 R. N... 100644 100644 100644 a b R100 new\told\nu UU N... 1 2 3 4 a b c f\n? new.txt\n! ignored.log\n"
        );
        assert_eq!(parse_status(&out).dirty, 4);
    }

    #[test]
    fn reads_ahead_and_behind() {
        let out = "# branch.head main\n# branch.upstream origin/main\n# branch.ab +2 -1\n";
        assert_eq!(parse_status(out).ahead_behind, Some((2, 1)));
    }

    #[test]
    fn no_upstream_has_no_ahead_behind() {
        let out = "# branch.oid abc\n# branch.head feat\n? x\n";
        assert_eq!(
            parse_status(out),
            Status {
                dirty: 1,
                ahead_behind: None
            }
        );
    }

    #[test]
    fn detached_head_has_no_ahead_behind() {
        let out = "# branch.oid abc\n# branch.head (detached)\n1 M. N... 1 1 1 a b f\n";
        assert_eq!(
            parse_status(out),
            Status {
                dirty: 1,
                ahead_behind: None
            }
        );
    }

    struct Fake(Option<&'static str>);
    impl StatusRunner for Fake {
        fn status(&self, _dir: &str, _timeout: Duration) -> Option<String> {
            self.0.map(str::to_owned)
        }
    }

    #[test]
    fn a_timed_out_status_leaves_the_fields_unknown() {
        assert_eq!(status_with(&Fake(None), "/r", STATUS_TIMEOUT), None);
        let mut info = parse_rev_parse("main\n/r/.git\n/r/.git\n").unwrap();
        apply_status(&mut info, None);
        assert_eq!((info.dirty, info.ahead, info.behind), (None, None, None));
    }

    #[test]
    fn a_status_fills_the_fields() {
        let status = status_with(&Fake(Some(CLEAN)), "/r", STATUS_TIMEOUT);
        let mut info = parse_rev_parse("main\n/r/.git\n/r/.git\n").unwrap();
        apply_status(&mut info, status);
        assert_eq!(
            (info.dirty, info.ahead, info.behind),
            (Some(0), Some(0), Some(0))
        );
    }

    #[test]
    fn the_real_runner_gives_nothing_for_a_missing_directory() {
        assert_eq!(
            GitRunner.status("/nonexistent-poimen-dir", Duration::from_secs(2)),
            None
        );
    }
}
