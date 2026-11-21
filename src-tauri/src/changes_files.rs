//! Files the app writes into `<worktree>/.agents/state/` for the Changes view: line comments and
//! CI logs. Real directories only (never a symlink), never overwriting a comments file.

use std::path::Path;

use crate::changes::{text, validate};
use crate::git::run_bytes;

/// Largest comments file the app writes.
pub(crate) const MAX_COMMENTS: usize = 1024 * 1024;
const EXCLUDE_LINE: &str = ".agents/state/";

/// A directory component that is a real directory (created when missing), never a symlink.
fn plain_dir(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_dir() => Ok(()),
        Ok(_) => Err(format!("{} is not a plain directory", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path).map_err(|e| format!("{}: {e}", path.display()))
        }
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// Adds `.agents/state/` to the repository's `info/exclude` (once).
fn exclude_state(top: &str) -> Result<(), String> {
    let out = run_bytes(top, &["rev-parse", "--git-path", "info/exclude"])?;
    let rel = text(&out).trim().to_string();
    if rel.is_empty() {
        return Err("git reported no info/exclude path".into());
    }
    let path = Path::new(top).join(rel);
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current.lines().any(|l| l.trim() == EXCLUDE_LINE) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(EXCLUDE_LINE);
    next.push('\n');
    std::fs::write(&path, next).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes `content` to a new `comments-<unix seconds>.md` in `<top>/.agents/state/` and returns
/// its absolute path. Refuses symlinked `.agents` / `state` directories and never overwrites.
pub(crate) fn write_comments_in(top: &str, content: &str) -> Result<String, String> {
    if content.len() > MAX_COMMENTS {
        return Err("the comments are too large".into());
    }
    let agents = Path::new(top).join(".agents");
    plain_dir(&agents)?;
    let state = agents.join("state");
    plain_dir(&state)?;
    exclude_state(top)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    for n in 0..100 {
        let name = if n == 0 {
            format!("comments-{stamp}.md")
        } else {
            format!("comments-{stamp}-{n}.md")
        };
        let file = state.join(&name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&file)
        {
            Ok(mut f) => {
                use std::io::Write;
                f.write_all(content.as_bytes())
                    .map_err(|e| format!("{}: {e}", file.display()))?;
                return Ok(file.to_string_lossy().into_owned());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", file.display())),
        }
    }
    Err("could not find a free comments file name".into())
}

pub fn write_comments(dir: &str, content: &str, home: Option<&Path>) -> Result<String, String> {
    write_comments_in(&validate(dir, home)?, content)
}

/// Writes `content` to `<top>/.agents/state/ci-failed-<nr>.log` (replacing an earlier one),
/// creating the directories and excluding `.agents/state/` once; returns the absolute path.
pub(crate) fn write_ci_log_in(top: &str, nr: u64, content: &str) -> Result<String, String> {
    let agents = Path::new(top).join(".agents");
    plain_dir(&agents)?;
    let state = agents.join("state");
    plain_dir(&state)?;
    exclude_state(top)?;
    let file = state.join(format!("ci-failed-{nr}.log"));
    // A symlink left at that name is removed, never followed.
    match std::fs::remove_file(&file) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("{}: {e}", file.display())),
    }
    std::fs::write(&file, content).map_err(|e| format!("{}: {e}", file.display()))?;
    Ok(file.to_string_lossy().into_owned())
}

pub fn write_ci_log(
    dir: &str,
    nr: u64,
    content: &str,
    home: Option<&Path>,
) -> Result<String, String> {
    write_ci_log_in(&validate(dir, home)?, nr, content)
}
