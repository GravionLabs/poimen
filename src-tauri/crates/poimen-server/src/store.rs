//! The persisted space list (`server.json`): name and cwd per space. Terminals are not stored.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use poimen_protocol::Space;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    spaces: Vec<Space>,
}

/// `$POIMEN_STATE_DIR/server.json`, else `$XDG_STATE_HOME/poimen/server.json`, else
/// `~/.local/state/poimen/server.json`.
pub fn state_file_path() -> PathBuf {
    state_file_from(
        std::env::var_os("POIMEN_STATE_DIR"),
        std::env::var_os("XDG_STATE_HOME"),
        std::env::var_os("HOME"),
    )
}

pub(crate) fn state_file_from(
    state_dir: Option<std::ffi::OsString>,
    xdg_state: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> PathBuf {
    let set = |v: Option<std::ffi::OsString>| v.filter(|s| !s.is_empty()).map(PathBuf::from);
    if let Some(dir) = set(state_dir) {
        return dir.join("server.json");
    }
    if let Some(dir) = set(xdg_state) {
        return dir.join("poimen").join("server.json");
    }
    set(home)
        .unwrap_or_else(std::env::temp_dir)
        .join(".local/state/poimen/server.json")
}

/// Reads the spaces; a missing or unreadable file gives an empty list (a damaged file is
/// reported on stderr and replaced by the next save).
pub(crate) fn load(path: &Path) -> Vec<Space> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<File>(&text)
            .map(|f| f.spaces)
            .unwrap_or_else(|e| {
                eprintln!("poimen-server: ignoring {}: {e}", path.display());
                Vec::new()
            }),
        Err(_) => Vec::new(),
    }
}

/// Writes the spaces atomically (temp file, then rename).
pub(crate) fn save(path: &Path, spaces: &[Space]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = serde_json::to_string_pretty(&File {
        spaces: spaces.to_vec(),
    })
    .map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path)
}

/// The first id counter value that collides with no persisted `s<n>` id.
pub(crate) fn next_id(spaces: &[Space]) -> u64 {
    spaces
        .iter()
        .filter_map(|s| s.id.strip_prefix('s')?.parse::<u64>().ok())
        .max()
        .map_or(1, |n| n + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn space(id: &str) -> Space {
        Space {
            id: id.into(),
            name: "n".into(),
            cwd: "/x".into(),
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-store-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("server.json")
    }

    #[test]
    fn save_then_load_round_trips() {
        let path = temp("rt");
        save(&path, &[space("s1"), space("s4")]).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[1].id, "s4");
    }

    #[test]
    fn missing_and_damaged_files_give_no_spaces() {
        assert!(load(&temp("missing")).is_empty());
        let path = temp("bad");
        fs::write(&path, "{ nope").unwrap();
        assert!(load(&path).is_empty());
    }

    #[test]
    fn ids_continue_after_the_highest_persisted() {
        assert_eq!(next_id(&[]), 1);
        assert_eq!(next_id(&[space("s2"), space("s9"), space("weird")]), 10);
    }

    #[test]
    fn state_file_precedence() {
        assert_eq!(
            state_file_from(Some("/d".into()), Some("/x".into()), Some("/h".into())),
            PathBuf::from("/d/server.json")
        );
        assert_eq!(
            state_file_from(None, Some("/x".into()), Some("/h".into())),
            PathBuf::from("/x/poimen/server.json")
        );
        assert_eq!(
            state_file_from(None, Some("".into()), Some("/h".into())),
            PathBuf::from("/h/.local/state/poimen/server.json")
        );
    }
}
