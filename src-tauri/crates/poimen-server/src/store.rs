//! The persisted state (`server.json`): the spaces (name, cwd, tab order, active tab), their
//! tabs with the split trees, the id counter, and the running terminals with their hosts, so
//! the next server adopts them (ADR 0009, #604).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use poimen_protocol::{AgentInfo, Channel, Space, Tab};
use serde::{Deserialize, Serialize};

use crate::tabs::Layout;

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    spaces: Vec<Space>,
    /// Absent from a protocol 3 file.
    #[serde(default)]
    tabs: Vec<Tab>,
    /// The next value of the id counter, so no id (a `split_id` above all) is given twice, also
    /// for nodes that were dropped since. Absent from a protocol 3 file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    next_id: Option<u64>,
    /// The running terminals; absent from a file of a server before #604 (and when none runs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    terminals: Vec<StoredTerminal>,
}

/// A running terminal as the next server needs it to adopt the host: who it is, where its host
/// listens, and what the server keeps beside the grid (size, agent status, tokens).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredTerminal {
    pub(crate) id: String,
    pub(crate) space_id: String,
    pub(crate) name: String,
    pub(crate) cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) program: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) args: Vec<String>,
    /// The size last set (by the controller, or at create); the host's `hello` has the PTY's.
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    /// The endpoint of the terminal's `poimen-pty-host`.
    pub(crate) endpoint: PathBuf,
    /// The `seq` of the last output record read from the host when the list was written. A host
    /// behind it is another one.
    #[serde(default)]
    pub(crate) seq: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent: Option<AgentInfo>,
    /// The `seq` of the last accepted agent report, so a stale report stays dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tokens: Vec<StoredToken>,
}

/// A token with its owner and the end of its TTL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct StoredToken {
    pub(crate) name: String,
    pub(crate) value: String,
    pub(crate) source: String,
    /// When the TTL runs out, milliseconds since the epoch; `None`: no TTL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) expires_at_ms: Option<u64>,
}

/// What [`load`] read.
#[derive(Debug, Default)]
pub(crate) struct Stored {
    pub(crate) layout: Layout,
    /// The first id counter value that collides with nothing ever given out.
    pub(crate) next_id: u64,
    /// The terminals that ran when the file was written.
    pub(crate) terminals: Vec<StoredTerminal>,
}

/// Unix: `$POIMEN_STATE_DIR/server.json`, else `$XDG_STATE_HOME/poimen/server.json`, else
/// `~/.local/state/poimen/server.json`. Windows: `$POIMEN_STATE_DIR\server.json`, else
/// `%LOCALAPPDATA%\poimen-server\server.json`, else `~\AppData\Local\poimen-server\server.json`.
/// An rc build uses `poimen-rc` and `poimen-server-rc` ([`poimen_protocol::CHANNEL`]).
pub fn state_file_path() -> PathBuf {
    let channel = poimen_protocol::CHANNEL;
    if cfg!(windows) {
        windows_state_file_from(
            std::env::var_os("POIMEN_STATE_DIR"),
            std::env::var_os("LOCALAPPDATA"),
            std::env::home_dir(),
            channel,
        )
    } else {
        state_file_from(
            std::env::var_os("POIMEN_STATE_DIR"),
            std::env::var_os("XDG_STATE_HOME"),
            std::env::var_os("HOME"),
            channel,
        )
    }
}

/// The Windows [`state_file_path`] with the environment passed in; empty values count as unset.
pub(crate) fn windows_state_file_from(
    state_dir: Option<std::ffi::OsString>,
    local_app_data: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
    channel: Channel,
) -> PathBuf {
    if let Some(dir) = state_dir.filter(|s| !s.is_empty()) {
        return PathBuf::from(dir).join("server.json");
    }
    poimen_protocol::windows_dir_from(local_app_data, channel)
        .unwrap_or_else(|| {
            home.filter(|h| !h.as_os_str().is_empty())
                .unwrap_or_else(std::env::temp_dir)
                .join("AppData")
                .join("Local")
                .join(poimen_protocol::windows_dir_name(channel))
        })
        .join("server.json")
}

pub(crate) fn state_file_from(
    state_dir: Option<std::ffi::OsString>,
    xdg_state: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
    channel: Channel,
) -> PathBuf {
    let set = |v: Option<std::ffi::OsString>| v.filter(|s| !s.is_empty()).map(PathBuf::from);
    if let Some(dir) = set(state_dir) {
        return dir.join("server.json");
    }
    let dir = channel.named("poimen");
    if let Some(state) = set(xdg_state) {
        return state.join(dir).join("server.json");
    }
    set(home)
        .unwrap_or_else(std::env::temp_dir)
        .join(".local")
        .join("state")
        .join(dir)
        .join("server.json")
}

/// Reads the layout; a missing or unreadable file gives an empty one (a damaged file is reported
/// on stderr and replaced by the next save).
pub(crate) fn load(path: &Path) -> Stored {
    let file = match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str::<File>(&text).unwrap_or_else(|e| {
            eprintln!("poimen-server: ignoring {}: {e}", path.display());
            File::default()
        }),
        Err(_) => File::default(),
    };
    let layout = Layout::new(file.spaces, file.tabs);
    let ids = layout
        .ids()
        .into_iter()
        .chain(file.terminals.iter().map(|t| t.id.as_str()));
    let next_id = next_id(ids).max(file.next_id.unwrap_or(1));
    Stored {
        layout,
        next_id,
        terminals: file.terminals,
    }
}

/// Writes the layout, the id counter and the running terminals atomically (temp file, then
/// rename), so a crash leaves the previous file or this one, never half of it.
pub(crate) fn save(
    path: &Path,
    layout: &Layout,
    next_id: u64,
    terminals: Vec<StoredTerminal>,
) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        create_private_dir(dir)?;
    }
    let text = serde_json::to_string_pretty(&File {
        spaces: layout.spaces.clone(),
        tabs: layout.tabs.clone(),
        next_id: Some(next_id),
        terminals,
    })
    .map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    write_private(&tmp, text.as_bytes())?;
    fs::rename(&tmp, path)
}

/// Creates the directory (and parents) with mode 0700; an existing one is left as it is.
fn create_private_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(dir)
    }
    #[cfg(not(unix))]
    fs::create_dir_all(dir)
}

/// Writes the file with mode 0600 (a leftover temp file is replaced).
fn write_private(path: &Path, data: &[u8]) -> io::Result<()> {
    use std::io::Write;
    let _ = fs::remove_file(path);
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)?.write_all(data)
}

/// The first id counter value that collides with none of the ids (`<letters><number>`, e.g.
/// `s3`, `tab4`, `split9`; others are ignored).
pub(crate) fn next_id<'a>(ids: impl IntoIterator<Item = &'a str>) -> u64 {
    ids.into_iter()
        .filter_map(|id| {
            id.trim_start_matches(|c: char| c.is_ascii_alphabetic())
                .parse::<u64>()
                .ok()
        })
        .max()
        .map_or(1, |n| n + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    use poimen_protocol::{Node, SplitDirection};

    fn space(id: &str) -> Space {
        Space {
            id: id.into(),
            name: "n".into(),
            cwd: "/x".into(),
            ..Default::default()
        }
    }

    fn layout(spaces: &[&str]) -> Layout {
        Layout::new(spaces.iter().map(|id| space(id)).collect(), Vec::new())
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-store-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir.join("server.json")
    }

    #[cfg(unix)]
    #[test]
    fn the_state_file_is_0600_and_a_new_directory_0700() {
        use std::os::unix::fs::PermissionsExt;
        let base = temp("modes");
        let path = base.parent().unwrap().join("fresh").join("server.json");
        save(&path, &layout(&["s1"]), 2, Vec::new()).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    fn save_then_load_round_trips_spaces_tabs_trees_and_the_counter() {
        let path = temp("rt");
        let mut saved = layout(&["s1", "s4"]);
        saved.spaces[0].tabs = vec!["tab5".into()];
        saved.spaces[0].active_tab = Some("tab5".into());
        saved.tabs.push(Tab {
            tab_id: "tab5".into(),
            space_id: "s1".into(),
            name: "dev".into(),
            root: Node::Split {
                split_id: "split7".into(),
                direction: SplitDirection::Down,
                ratio: 0.3,
                children: Box::new([Node::terminal("t2"), Node::terminal("t6")]),
            },
            active_terminal: "t6".into(),
        });
        save(&path, &saved, 40, Vec::new()).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.layout, saved);
        assert_eq!(loaded.next_id, 40);
    }

    fn stored(id: &str) -> StoredTerminal {
        StoredTerminal {
            id: id.into(),
            space_id: "s1".into(),
            name: "agent".into(),
            cwd: "/w".into(),
            program: Some("zsh".into()),
            args: vec!["-l".into()],
            cols: 120,
            rows: 40,
            endpoint: PathBuf::from(format!("/run/u/poimen/hosts/{id}")),
            seq: 812,
            agent: Some(AgentInfo {
                status: poimen_protocol::AgentStatus::Blocked,
                agent: Some("claude".into()),
                session_id: Some("abc".into()),
                message: Some("Allow Bash?".into()),
                message_at: Some(1_700_000_000_000),
            }),
            agent_seq: Some(17),
            tokens: vec![StoredToken {
                name: "deliver_step".into(),
                value: "review".into(),
                source: "deliver".into(),
                expires_at_ms: Some(1_700_000_060_000),
            }],
        }
    }

    #[test]
    fn the_terminal_list_round_trips_with_endpoint_seq_status_and_tokens() {
        let path = temp("terms");
        save(&path, &layout(&["s1"]), 20, vec![stored("t9")]).unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.terminals, vec![stored("t9")]);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\"endpoint\": \"/run/u/poimen/hosts/t9\""),
            "{text}"
        );
        assert!(text.contains("\"agent_seq\": 17"), "{text}");
    }

    #[test]
    fn a_file_without_terminals_has_no_terminal_key_and_loads_an_empty_list() {
        let path = temp("noterms");
        save(&path, &layout(&["s1"]), 2, Vec::new()).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("terminals"));
        assert!(load(&path).terminals.is_empty());
    }

    #[test]
    fn stored_terminal_ids_are_never_given_again() {
        let path = temp("termids");
        fs::write(
            &path,
            r#"{"spaces":[{"id":"s1","name":"a","cwd":"/x"}],"terminals":[{"id":"t41","space_id":"s1","name":"sh","cwd":"/x","cols":80,"rows":24,"endpoint":"/e/t41"}]}"#,
        )
        .unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.next_id, 42);
        // Fields a server before #604 never wrote take their defaults.
        assert_eq!(loaded.terminals[0].seq, 0);
        assert_eq!(loaded.terminals[0].agent, None);
        assert!(loaded.terminals[0].tokens.is_empty());
    }

    #[test]
    fn a_protocol_3_file_loads_its_spaces_without_tabs() {
        let path = temp("v3");
        fs::write(
            &path,
            r#"{"spaces":[{"id":"s1","name":"api","cwd":"/x"},{"id":"s3","name":"b","cwd":"/y"}]}"#,
        )
        .unwrap();
        let loaded = load(&path);
        assert_eq!(loaded.layout.spaces.len(), 2);
        assert!(loaded.layout.spaces.iter().all(|s| s.tabs.is_empty()));
        assert!(loaded.layout.tabs.is_empty());
        assert_eq!(loaded.next_id, 4);
    }

    #[test]
    fn missing_and_damaged_files_give_no_spaces() {
        assert!(load(&temp("missing")).layout.spaces.is_empty());
        let path = temp("bad");
        fs::write(&path, "{ nope").unwrap();
        let loaded = load(&path);
        assert!(loaded.layout.spaces.is_empty());
        assert_eq!(loaded.next_id, 1);
    }

    #[test]
    fn ids_continue_after_the_highest_persisted() {
        assert_eq!(next_id([]), 1);
        assert_eq!(next_id(["s2", "s9", "weird", "tab12x"]), 10);
        assert_eq!(next_id(["s2", "tab11", "split14"]), 15);
    }

    #[test]
    fn the_persisted_counter_wins_over_the_ids_left_in_the_file() {
        // A split that was dropped took its id with it; the counter still remembers it.
        let path = temp("counter");
        save(&path, &layout(&["s1"]), 30, Vec::new()).unwrap();
        assert_eq!(load(&path).next_id, 30);
        save(&path, &layout(&["s50"]), 30, Vec::new()).unwrap();
        assert_eq!(load(&path).next_id, 51);
    }

    #[test]
    fn state_file_precedence() {
        let stable = Channel::Stable;
        assert_eq!(
            state_file_from(
                Some("/d".into()),
                Some("/x".into()),
                Some("/h".into()),
                stable
            ),
            PathBuf::from("/d/server.json")
        );
        assert_eq!(
            state_file_from(None, Some("/x".into()), Some("/h".into()), stable),
            PathBuf::from("/x/poimen/server.json")
        );
        assert_eq!(
            state_file_from(None, Some("".into()), Some("/h".into()), stable),
            PathBuf::from("/h/.local/state/poimen/server.json")
        );
    }

    #[test]
    fn the_rc_channel_keeps_its_own_state_file() {
        let rc = Channel::Rc;
        assert_eq!(
            state_file_from(None, Some("/x".into()), Some("/h".into()), rc),
            PathBuf::from("/x/poimen-rc/server.json")
        );
        assert_eq!(
            state_file_from(None, None, Some("/h".into()), rc),
            PathBuf::from("/h/.local/state/poimen-rc/server.json")
        );
        // An explicit state dir is taken as it is.
        assert_eq!(
            state_file_from(Some("/d".into()), None, None, rc),
            PathBuf::from("/d/server.json")
        );
    }

    #[test]
    fn windows_state_file_precedence() {
        let local = PathBuf::from(r"C:\Users\u\AppData\Local");
        let home = PathBuf::from(r"C:\Users\u");
        let stable = Channel::Stable;
        assert_eq!(
            windows_state_file_from(
                Some(r"D:\s".into()),
                Some(local.clone().into()),
                Some(home.clone()),
                stable
            ),
            PathBuf::from(r"D:\s").join("server.json")
        );
        assert_eq!(
            windows_state_file_from(
                Some("".into()),
                Some(local.clone().into()),
                Some(home.clone()),
                stable
            ),
            local.join("poimen-server").join("server.json")
        );
        assert_eq!(
            windows_state_file_from(None, None, Some(home.clone()), stable),
            home.join("AppData")
                .join("Local")
                .join("poimen-server")
                .join("server.json")
        );
        assert_eq!(
            windows_state_file_from(None, Some(local.clone().into()), None, Channel::Rc),
            local.join("poimen-server-rc").join("server.json")
        );
        assert_eq!(
            windows_state_file_from(None, None, Some(home.clone()), Channel::Rc),
            home.join("AppData")
                .join("Local")
                .join("poimen-server-rc")
                .join("server.json")
        );
    }
}
