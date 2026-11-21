//! What the Changes view may send to the webview for one file: the text, or only a marker. Two
//! limits, both checked on server side numbers (the counts of `--numstat` and the file size)
//! before any content is read: above the soft limit the diff loads on demand ("Load diff"),
//! above the hard limit it is never offered (#1037).

use serde::Serialize;
use std::path::Path;

use crate::changes::ChangedFile;

/// Hard limit: the largest file content sent to the merge view.
pub const MAX_CONTENT: usize = 1024 * 1024;
/// Hard limit: changed lines (additions plus deletions) above which no diff is offered.
pub const MAX_CHANGED_LINES: u32 = 120_000;
/// Soft limit: changed lines above which the diff loads on demand.
pub const LARGE_CHANGED_LINES: u32 = 5_000;
/// Soft limit: bytes of the file in the working tree above which the diff loads on demand.
pub const LARGE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum FileContent {
    Text {
        old: String,
        new: String,
    },
    Binary,
    /// Above the soft limit: only the counts, the diff is read with `force`.
    Large {
        additions: u32,
        deletions: u32,
    },
    /// Above the hard limit: no diff is offered.
    TooLarge,
}

/// Old and new content of one changed file for the merge view (read-only git). `force` loads a
/// file above the soft limit; the hard limit holds either way.
#[tauri::command]
pub async fn changes_file(
    dir: String,
    scope: crate::changes::Scope,
    path: String,
    force: Option<bool>,
) -> Result<FileContent, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let force = force.unwrap_or(false);
        crate::changes::file(&dir, scope, &path, force, crate::home_dir().as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The marker for a file over a limit, `None` when its content may be read. `force` skips the
/// soft limit only.
pub(crate) fn limit(
    additions: u32,
    deletions: u32,
    bytes: u64,
    force: bool,
) -> Option<FileContent> {
    let lines = additions.saturating_add(deletions);
    if lines > MAX_CHANGED_LINES {
        return Some(FileContent::TooLarge);
    }
    if !force && (lines > LARGE_CHANGED_LINES || bytes > LARGE_BYTES) {
        return Some(FileContent::Large {
            additions,
            deletions,
        });
    }
    None
}

/// [`limit`] for a changed file of the worktree `dir`.
pub(crate) fn over_limit(dir: &str, file: &ChangedFile, force: bool) -> Option<FileContent> {
    let bytes = std::fs::symlink_metadata(Path::new(dir).join(&file.path)).map_or(0, |m| m.len());
    limit(file.additions, file.deletions, bytes, force)
}

/// A file of the working tree, read without following symlinks (a link yields its target
/// path) and only up to `MAX_CONTENT` bytes: `Err(())` means too large, `Ok(None)` unreadable.
pub(crate) fn read_worktree_file(root: &str, path: &str) -> Result<Option<Vec<u8>>, ()> {
    let full = Path::new(root).join(path);
    let Ok(meta) = std::fs::symlink_metadata(&full) else {
        return Ok(None);
    };
    if meta.file_type().is_symlink() {
        return Ok(std::fs::read_link(&full)
            .ok()
            .map(|t| t.into_os_string().into_encoded_bytes()));
    }
    if !meta.is_file() {
        return Ok(None);
    }
    if meta.len() > MAX_CONTENT as u64 {
        return Err(());
    }
    Ok(std::fs::read(&full).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_diff_loads_on_demand() {
        let large = |a, d| {
            Some(FileContent::Large {
                additions: a,
                deletions: d,
            })
        };
        assert_eq!(limit(10, 5, 100, false), None);
        // The soft limit by lines (additions and deletions together) and by bytes.
        assert_eq!(limit(LARGE_CHANGED_LINES, 0, 0, false), None);
        assert_eq!(
            limit(LARGE_CHANGED_LINES, 1, 0, false),
            large(LARGE_CHANGED_LINES, 1)
        );
        assert_eq!(limit(1, 1, LARGE_BYTES + 1, false), large(1, 1));
        // `force` loads it.
        assert_eq!(
            limit(LARGE_CHANGED_LINES + 1, 0, LARGE_BYTES + 1, true),
            None
        );
    }

    #[test]
    fn a_huge_diff_is_never_offered() {
        for force in [false, true] {
            assert_eq!(
                limit(MAX_CHANGED_LINES + 1, 0, 0, force),
                Some(FileContent::TooLarge)
            );
            assert_eq!(
                limit(MAX_CHANGED_LINES, 1, 0, force),
                Some(FileContent::TooLarge)
            );
        }
        assert!(limit(MAX_CHANGED_LINES, 0, 0, true).is_none());
    }
}
