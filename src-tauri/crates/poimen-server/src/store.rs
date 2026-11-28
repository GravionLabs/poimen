//! The persisted state (`server.json`): the spaces (name, cwd, tab order, active tab), their
//! tabs with the split trees, the id counter, and the running terminals with their hosts, so
//! the next server adopts them (ADR 0009, #604).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub use poimen_protocol::files::state_file_path;
use poimen_protocol::{AgentInfo, AgentWriteAction, Space, Tab};
use serde::{Deserialize, Serialize};

use crate::scheduler::{Alerts, Persisted, StoredSchedule};
use crate::tabs::Layout;

fn current_version() -> u64 {
    VERSION
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    /// Absent from a file before #967 (read as today); never above [`VERSION`] when read.
    #[serde(default = "current_version")]
    version: u64,
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
    /// The scheduler's state (#1000); absent without a schedule.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    schedules: Vec<StoredSchedule>,
    /// The unacknowledged alerts of the scheduler (#1308); absent while there never was one.
    #[serde(default, skip_serializing_if = "Alerts::is_unused")]
    schedule_alerts: Alerts,
    /// The active scoped write grants (ADR 0018, #1453); absent without one. Kept as raw values
    /// so a grant that does not parse costs only the grants, never the spaces and terminals.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    grants: Vec<serde_json::Value>,
}

/// A confirmed grant as the next server needs it (ADR 0018 amendment, #1453). An unknown field
/// (a newer server's, which might narrow the grant) makes the grants unreadable, not wider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StoredGrant {
    pub(crate) id: String,
    /// The lead terminal.
    pub(crate) lead: String,
    pub(crate) spaces: Vec<String>,
    pub(crate) tools: Vec<AgentWriteAction>,
    pub(crate) max_agents: u32,
    /// The grant covers worktree starts (#1451); absent when it does not.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) worktrees: bool,
    /// Milliseconds since the epoch, by the wall clock.
    pub(crate) expires_at: u64,
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
    /// `agent` is the status the program reported with OSC 7501 (#681); `hook_agent` is the
    /// status of the hooks meanwhile.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) osc: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) hook_agent: Option<AgentInfo>,
    /// The `seq` of the last accepted agent report, so a stale report stays dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) agent_seq: Option<u64>,
    /// The child records of OSC 7501 reports with an `id` (#684), as reported.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) records: Vec<poimen_protocol::ChildRecord>,
    /// The agent was hibernated (ADR 0014, #903); it survives a restart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) hibernated: Option<poimen_protocol::Hibernated>,
    /// The terminal whose agent started this one (#982); survives a restart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) started_by: Option<String>,
    /// The agent-events run and task of a scripted session (#744); survive a restart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) task: Option<String>,
    /// The agent's `report_done` (#980); survives a restart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) report: Option<poimen_protocol::report::AgentReport>,
    /// The last agent session seen in the terminal (#1232); kept after `/clear` or `gone`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_agent: Option<LastAgent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) tokens: Vec<StoredToken>,
}

/// What a resume of an agent needs after its terminal's host is gone (#1232): the latest
/// session id wins. Not on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LastAgent {
    pub(crate) agent: String,
    pub(crate) session_id: String,
    pub(crate) cwd: String,
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
    pub(crate) schedules: Persisted,
    /// The grants of the current file; none when it was damaged, missing or any grant in it does
    /// not parse (never a part of them, never the backup's, which may predate a revocation).
    pub(crate) grants: Vec<StoredGrant>,
}

/// The version of the file this server writes and the highest it reads.
pub(crate) const VERSION: u64 = 1;
/// How many `server.json.damaged-<unix seconds>` files are kept.
const KEEP_DAMAGED: usize = 3;

/// A state file written by a newer server: this one neither reads nor writes it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Newer {
    pub(crate) file: PathBuf,
    pub(crate) found: u64,
    pub(crate) known: u64,
    /// Set instead of the versions when a damaged file could not be moved aside: a save would
    /// overwrite the good copy with it, so the server does not start.
    pub(crate) stuck: Option<String>,
}

impl std::fmt::Display for Newer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(cause) = &self.stuck {
            return write!(
                f,
                "{} is damaged and could not be moved aside: {cause}",
                self.file.display()
            );
        }
        write!(
            f,
            "{} has version {}, this poimen-server knows version {}; update poimen-server, nothing was changed",
            self.file.display(),
            self.found,
            self.known
        )
    }
}

/// [`load`] for the start: a file of a newer version is an `InvalidData` error whose message is
/// also written to `server.log` (the line the app shows as the start failure). Called before the
/// socket is touched, so a refused start leaves no stale entry.
pub(crate) fn load_or_refuse(path: &Path) -> io::Result<Stored> {
    load(path).map_err(|newer| {
        let message = newer.to_string();
        let now = poimen_protocol::files::utc_now();
        crate::log::append(
            &crate::log::path(path),
            &format!("{now} server refusing to start: {message}"),
        );
        io::Error::new(io::ErrorKind::InvalidData, message)
    })
}

/// The name of the previous copy: `server.json.bak`.
fn backup_path(path: &Path) -> PathBuf {
    with_suffix(path, ".bak")
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

enum Read {
    Missing,
    Good(File),
    Newer(u64),
    Damaged(String),
}

fn read_file(path: &Path) -> Read {
    #[derive(Deserialize)]
    struct Probe {
        version: Option<u64>,
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Read::Missing,
        Err(e) => return Read::Damaged(e.to_string()),
    };
    match serde_json::from_str::<Probe>(&text) {
        Ok(Probe { version: Some(v) }) if v > VERSION => return Read::Newer(v),
        Ok(_) => {}
        Err(e) => return Read::Damaged(e.to_string()),
    }
    match serde_json::from_str::<File>(&text) {
        Ok(file) => Read::Good(file),
        Err(e) => Read::Damaged(e.to_string()),
    }
}

fn note(path: &Path, message: &str) {
    crate::log::line(&crate::log::path(path), "store", message);
}

/// Moves a file that could not be read to `<name>.damaged-<unix seconds>` (a name that is free),
/// keeps the newest [`KEEP_DAMAGED`] of those and removes the older ones.
fn set_aside(path: &Path, cause: &str) -> io::Result<()> {
    let mut seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let name = path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let prefix = format!("{name}.damaged-");
    let target = loop {
        let candidate = with_suffix(path, &format!(".damaged-{seconds}"));
        if !candidate.exists() {
            break candidate;
        }
        seconds += 1;
    };
    let moved = fs::rename(path, &target);
    match &moved {
        Ok(()) => note(
            path,
            &format!(
                "{} is damaged ({cause}), moved to {}",
                path.display(),
                target.display()
            ),
        ),
        Err(e) => note(
            path,
            &format!(
                "{} is damaged ({cause}) and could not be moved aside: {e}",
                path.display()
            ),
        ),
    }
    moved?;
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    let mut kept: Vec<(u64, PathBuf)> = entries
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            let secs = n.strip_prefix(&prefix)?.parse::<u64>().ok()?;
            Some((secs, e.path()))
        })
        .collect();
    kept.sort();
    while kept.len() > KEEP_DAMAGED {
        let (_, old) = kept.remove(0);
        let _ = fs::remove_file(old);
    }
    Ok(())
}

/// Reads the layout from `server.json`; a missing one falls back to `server.json.bak` (the
/// window between the two renames of [`save`]). A file that does not parse is moved aside to
/// `.damaged-<unix seconds>` (never overwritten) and the backup is tried, else the layout is
/// empty; each case writes a line. A file of a newer version is left untouched and refused.
pub(crate) fn load(path: &Path) -> Result<Stored, Newer> {
    let backup = backup_path(path);
    let newer = |file: &Path, found| Newer {
        file: file.to_path_buf(),
        found,
        known: VERSION,
        stuck: None,
    };
    let stuck = |file: &Path, e: io::Error| Newer {
        stuck: Some(e.to_string()),
        ..newer(file, 0)
    };
    let mut file = match read_file(path) {
        Read::Good(file) => file,
        Read::Newer(v) => return Err(newer(path, v)),
        read @ (Read::Missing | Read::Damaged(_)) => {
            let reason = match read {
                Read::Damaged(cause) => {
                    set_aside(path, &cause).map_err(|e| stuck(path, e))?;
                    "damaged"
                }
                _ => "missing",
            };
            match read_file(&backup) {
                Read::Good(mut file) => {
                    note(
                        path,
                        &format!(
                            "{} is {reason}, loaded {} without its grants",
                            path.display(),
                            backup.display()
                        ),
                    );
                    file.grants.clear();
                    file
                }
                Read::Newer(v) => return Err(newer(&backup, v)),
                Read::Damaged(cause) => {
                    set_aside(&backup, &cause).map_err(|e| stuck(&backup, e))?;
                    File::default()
                }
                Read::Missing => File::default(),
            }
        }
    };
    let layout = Layout::new(file.spaces, file.tabs);
    let ids = layout
        .ids()
        .into_iter()
        .chain(file.terminals.iter().map(|t| t.id.as_str()));
    let next_id = next_id(ids).max(file.next_id.unwrap_or(1));
    let grants = std::mem::take(&mut file.grants)
        .into_iter()
        .map(serde_json::from_value)
        .collect::<Result<Vec<StoredGrant>, _>>()
        .unwrap_or_else(|e| {
            note(
                path,
                &format!("the grants of {} are dropped: {e}", path.display()),
            );
            Vec::new()
        });
    Ok(Stored {
        layout,
        next_id,
        terminals: file.terminals,
        schedules: Persisted {
            list: file.schedules,
            alerts: file.schedule_alerts,
        },
        grants,
    })
}

/// Writes the layout, the id counter, the running terminals and the grants: the temp file is synced, the
/// previous `server.json` becomes `server.json.bak`, the temp file becomes `server.json` and
/// the directory is synced (Unix). A crash leaves a readable `server.json` or `server.json.bak`.
pub(crate) fn save(
    path: &Path,
    layout: &Layout,
    next_id: u64,
    terminals: Vec<StoredTerminal>,
    schedules: impl Into<Persisted>,
    grants: &[StoredGrant],
) -> io::Result<()> {
    let Persisted { list, alerts } = schedules.into();
    if let Some(dir) = path.parent() {
        create_private_dir(dir)?;
    }
    let text = serde_json::to_string_pretty(&File {
        version: VERSION,
        spaces: layout.spaces.clone(),
        tabs: layout.tabs.clone(),
        next_id: Some(next_id),
        terminals,
        schedules: list,
        schedule_alerts: alerts,
        grants: grants
            .iter()
            .map(serde_json::to_value)
            .collect::<Result<_, _>>()
            .map_err(io::Error::other)?,
    })
    .map_err(io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    write_private(&tmp, text.as_bytes())?;
    match fs::rename(path, backup_path(path)) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    fs::rename(&tmp, path)?;
    sync_dir(path.parent());
    Ok(())
}

/// Syncs the directory so the renames survive a power loss (Unix; Windows has no such call).
fn sync_dir(dir: Option<&Path>) {
    #[cfg(unix)]
    if let Some(dir) = dir.map(|d| {
        if d.as_os_str().is_empty() {
            Path::new(".")
        } else {
            d
        }
    }) && let Ok(handle) = fs::File::open(dir)
    {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
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
    let mut file = options.open(path)?;
    file.write_all(data)?;
    file.sync_all()
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
#[path = "store_durability_tests.rs"]
mod durability;

#[cfg(test)]
mod tests {
    use super::*;
    use poimen_protocol::Channel;
    use poimen_protocol::files::{state_file_from, windows_state_file_from};

    use poimen_protocol::{Node, SplitDirection};

    fn space(id: &str) -> Space {
        Space {
            id: id.into(),
            name: "n".into(),
            cwd: "/x".into(),
            ..Default::default()
        }
    }

    pub(super) fn layout(spaces: &[&str]) -> Layout {
        Layout::new(spaces.iter().map(|id| space(id)).collect(), Vec::new())
    }

    /// The state file path in a fresh directory; keep the guard, it removes the directory on drop.
    pub(super) fn temp(name: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::Builder::new()
            .prefix(&format!("poimen-store-{name}-"))
            .tempdir()
            .unwrap();
        let path = dir.path().join("server.json");
        (dir, path)
    }

    #[cfg(unix)]
    #[test]
    fn the_state_file_is_0600_and_a_new_directory_0700() {
        use std::os::unix::fs::PermissionsExt;
        let (dir, _) = temp("modes");
        let path = dir.path().join("fresh").join("server.json");
        save(&path, &layout(&["s1"]), 2, Vec::new(), Vec::new(), &[]).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
    }

    #[test]
    fn save_then_load_round_trips_spaces_tabs_trees_and_the_counter() {
        let (_dir, path) = temp("rt");
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
            parent: None,
            place: None,
        });
        save(&path, &saved, 40, Vec::new(), Vec::new(), &[]).unwrap();
        let loaded = load(&path).unwrap();
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
                reported_at: Some(1_700_000_005_000),
                status_since: Some(1_700_000_000_000),
                silent_since: Some(1_700_000_600_000),
                status: poimen_protocol::AgentStatus::Blocked,
                agent: Some("claude".into()),
                session_id: Some("abc".into()),
                message: Some("Allow Bash?".into()),
                message_at: Some(1_700_000_000_000),
                progress: Some(40),
                kind: Some("permission".into()),
                tone: Some("warning".into()),
                title: Some("build".into()),
            }),
            osc: false,
            hook_agent: None,
            records: vec![poimen_protocol::ChildRecord {
                id: "run/a".into(),
                status: poimen_protocol::AgentStatus::Working,
                app: Some("codex".into()),
                message: None,
                message_at: None,
                progress: Some(5),
                kind: None,
                tone: Some("warning".into()),
                title: None,
            }],
            agent_seq: Some(17),
            hibernated: Some(poimen_protocol::Hibernated {
                agent: "claude".into(),
                session_id: "abc".into(),
                since: 1_700_000_100_000,
            }),
            started_by: Some("t0".into()),
            run: Some("M001".into()),
            task: Some("744".into()),
            report: None,
            last_agent: Some(LastAgent {
                agent: "claude".into(),
                session_id: "abc".into(),
                cwd: "/w".into(),
            }),
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
        let (_dir, path) = temp("terms");
        save(
            &path,
            &layout(&["s1"]),
            20,
            vec![stored("t9")],
            Vec::new(),
            &[],
        )
        .unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.terminals, vec![stored("t9")]);
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\"endpoint\": \"/run/u/poimen/hosts/t9\""),
            "{text}"
        );
        assert!(text.contains("\"agent_seq\": 17"), "{text}");
        assert!(text.contains("\"reported_at\": 1700000005000"), "{text}");
        assert!(text.contains("\"status_since\": 1700000000000"), "{text}");
    }

    #[test]
    fn a_file_without_terminals_has_no_terminal_key_and_loads_an_empty_list() {
        let (_dir, path) = temp("noterms");
        save(&path, &layout(&["s1"]), 2, Vec::new(), Vec::new(), &[]).unwrap();
        assert!(!fs::read_to_string(&path).unwrap().contains("terminals"));
        assert!(load(&path).unwrap().terminals.is_empty());
    }

    #[test]
    fn stored_terminal_ids_are_never_given_again() {
        let (_dir, path) = temp("termids");
        fs::write(
            &path,
            r#"{"spaces":[{"id":"s1","name":"a","cwd":"/x"}],"terminals":[{"id":"t41","space_id":"s1","name":"sh","cwd":"/x","cols":80,"rows":24,"endpoint":"/e/t41"}]}"#,
        )
        .unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.next_id, 42);
        // Fields a server before #604 never wrote take their defaults.
        assert_eq!(loaded.terminals[0].seq, 0);
        assert_eq!(loaded.terminals[0].agent, None);
        assert!(loaded.terminals[0].tokens.is_empty());
    }

    #[test]
    fn a_protocol_3_file_loads_its_spaces_without_tabs() {
        let (_dir, path) = temp("v3");
        fs::write(
            &path,
            r#"{"spaces":[{"id":"s1","name":"api","cwd":"/x"},{"id":"s3","name":"b","cwd":"/y"}]}"#,
        )
        .unwrap();
        let loaded = load(&path).unwrap();
        assert_eq!(loaded.layout.spaces.len(), 2);
        assert!(loaded.layout.spaces.iter().all(|s| s.tabs.is_empty()));
        assert!(loaded.layout.tabs.is_empty());
        assert_eq!(loaded.next_id, 4);
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
        let (_dir, path) = temp("counter");
        save(&path, &layout(&["s1"]), 30, Vec::new(), Vec::new(), &[]).unwrap();
        assert_eq!(load(&path).unwrap().next_id, 30);
        save(&path, &layout(&["s50"]), 30, Vec::new(), Vec::new(), &[]).unwrap();
        assert_eq!(load(&path).unwrap().next_id, 51);
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
