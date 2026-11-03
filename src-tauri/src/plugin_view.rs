//! Native views for herdr plugins (ADR 0004): the `data` action contract.
//!
//! A plugin declares an action `data` whose command prints one versioned JSON envelope. poimen reads
//! the command from `plugin.list`, runs it itself in the plugin directory (no shell, with a timeout)
//! and parses the envelope. Nothing here writes; the plugin owns its logic and its cache.
//!
//! Gaps of the ADR resolved here:
//! - `manifest_path` of `plugin.list` is the `herdr-plugin.toml` file, not a directory: the plugin
//!   directory is `plugin_root` when reported, else the parent of `manifest_path`.
//! - A macOS GUI app starts with `PATH=/usr/bin:/bin:/usr/sbin:/sbin`, so `node` and `gh` are not
//!   found. The child gets the login shell's `PATH` plus the usual install directories, and the
//!   program is resolved against that.
//! - The `data` action is hidden from the Plugins menu and the palette (`herdr::DATA_ACTION_ID`).
//! - `plugin.action.invoke` was not used as the transport: its `PluginCommandLogInfo.stdout` is an
//!   optional string with no documented limit and `status` may be `running`, so stdout can be
//!   incomplete. Running the command directly gives the exit code and the whole output.

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

use crate::herdr::DATA_ACTION_ID;

/// The plugin whose data the deliver board view renders, and the `data` versions poimen reads.
pub const BOARD_PLUGIN: &str = "deliver-board";
pub const BOARD_VERSIONS: &[u64] = &[1];

const TIMEOUT: Duration = Duration::from_secs(60);
const MAX_OUTPUT: u64 = 8 * 1024 * 1024;

/// How to run a plugin's `data` action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataCommand {
    pub dir: PathBuf,
    pub argv: Vec<String>,
}

/// The plugin directory: `plugin_root`, else the directory holding `manifest_path`.
fn plugin_dir(plugin: &Value) -> Option<PathBuf> {
    let text = |key: &str| {
        plugin
            .get(key)
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
    };
    if let Some(root) = text("plugin_root") {
        return Some(PathBuf::from(root));
    }
    Path::new(text("manifest_path")?)
        .parent()
        .map(Path::to_path_buf)
}

/// Finds the `data` command of an enabled plugin in a `plugin.list` result. `Err` says why the
/// plugin has no native view: not installed, disabled, or no `data` action yet.
pub fn data_command(list: &Value, plugin_id: &str) -> Result<DataCommand, String> {
    let plugin = list
        .get("plugins")
        .and_then(Value::as_array)
        .and_then(|all| {
            all.iter()
                .find(|p| p.get("plugin_id").and_then(Value::as_str) == Some(plugin_id))
        })
        .ok_or_else(|| format!("the plugin {plugin_id} is not installed"))?;
    if plugin.get("enabled").and_then(Value::as_bool) == Some(false) {
        return Err(format!("the plugin {plugin_id} is disabled"));
    }
    let argv: Vec<String> = plugin
        .get("actions")
        .and_then(Value::as_array)
        .and_then(|actions| {
            actions
                .iter()
                .find(|a| a.get("id").and_then(Value::as_str) == Some(DATA_ACTION_ID))
        })
        .and_then(|a| a.get("command")?.as_array())
        .map(|c| {
            c.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .filter(|c: &Vec<String>| !c.is_empty())
        .ok_or_else(|| {
            format!(
                "the installed {plugin_id} has no `{DATA_ACTION_ID}` action yet; update the plugin"
            )
        })?;
    let dir = plugin_dir(plugin)
        .ok_or_else(|| format!("herdr reports no directory for the plugin {plugin_id}"))?;
    Ok(DataCommand { dir, argv })
}

// ---------------------------------------------------------------- PATH of a GUI app

/// Directories where `node`, `gh` and `herdr` usually live on macOS and Linux.
fn usual_dirs(home: &str) -> Vec<String> {
    let mut dirs = vec![
        "/opt/homebrew/bin".to_owned(),
        "/usr/local/bin".to_owned(),
        format!("{home}/.local/bin"),
        format!("{home}/.volta/bin"),
        format!("{home}/.local/share/mise/shims"),
        format!("{home}/.asdf/shims"),
    ];
    // nvm keeps one directory per version; take the newest by name.
    if let Ok(entries) = std::fs::read_dir(format!("{home}/.nvm/versions/node")) {
        let mut versions: Vec<String> = entries
            .flatten()
            .map(|e| e.path().join("bin").to_string_lossy().into_owned())
            .collect();
        versions.sort();
        dirs.extend(versions.into_iter().rev().take(1));
    }
    dirs
}

/// The `PATH` for a plugin child: the login shell's, the usual install directories, then the
/// current one; duplicates and empty entries dropped, order kept.
pub fn augmented_path(shell_path: Option<&str>, home: &str, current: &str) -> String {
    let mut seen = HashSet::new();
    let mut parts = Vec::new();
    let extra = usual_dirs(home);
    let sources = shell_path
        .unwrap_or("")
        .split(':')
        .chain(extra.iter().map(String::as_str))
        .chain(current.split(':'));
    for part in sources {
        if !part.is_empty() && seen.insert(part.to_owned()) {
            parts.push(part.to_owned());
        }
    }
    parts.join(":")
}

/// Resolves `program` against `path` (a name without slash) the way a shell would.
pub fn resolve_program(program: &str, path: &str) -> Option<PathBuf> {
    if program.contains('/') {
        return Some(PathBuf::from(program));
    }
    path.split(':')
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join(program))
        .find(|candidate| candidate.is_file())
}

/// `PATH` as the user's login shell sees it, read once (`-ilc`, as fix-path does); `None` when the
/// shell fails or takes longer than five seconds.
fn login_shell_path() -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
            let mut child = Command::new(shell)
                .args(["-ilc", "printf '%s' \"__POIMEN_PATH__$PATH\""])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            let mut stdout = child.stdout.take()?;
            let reader = thread::spawn(move || {
                let mut text = String::new();
                stdout.read_to_string(&mut text).ok().map(|_| text)
            });
            let started = Instant::now();
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => break,
                    Ok(None) if started.elapsed() < Duration::from_secs(5) => {
                        thread::sleep(Duration::from_millis(25));
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break;
                    }
                }
            }
            let text = reader.join().ok()??;
            text.rsplit_once("__POIMEN_PATH__")
                .map(|(_, path)| path.trim().to_owned())
                .filter(|p| !p.is_empty())
        })
        .clone()
}

fn child_path() -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    let current = std::env::var("PATH").unwrap_or_default();
    augmented_path(login_shell_path().as_deref(), &home, &current)
}

// ---------------------------------------------------------------- running the command

/// Runs the command in the plugin directory with `--cwd <repo>`; stdout on success. Errors say
/// what failed (program not found, timeout, exit code with the start of stderr).
pub fn run_data_command(
    cmd: &DataCommand,
    repo: &str,
    state_dir: &Path,
    herdr_bin: &Path,
) -> Result<String, String> {
    let path = child_path();
    let program = resolve_program(&cmd.argv[0], &path)
        .ok_or_else(|| format!("`{}` was not found on PATH", cmd.argv[0]))?;
    let _ = std::fs::create_dir_all(state_dir);
    let mut child = Command::new(program)
        .args(&cmd.argv[1..])
        .args(["--cwd", repo])
        .current_dir(&cmd.dir)
        .env("PATH", &path)
        .env("HERDR_PLUGIN_STATE_DIR", state_dir)
        .env("HERDR_BIN_PATH", herdr_bin)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not start the data command: {e}"))?;
    let read = |stream: Option<Box<dyn Read + Send>>| {
        thread::spawn(move || {
            let mut text = String::new();
            if let Some(stream) = stream {
                let _ = stream.take(MAX_OUTPUT).read_to_string(&mut text);
            }
            text
        })
    };
    let out = read(child.stdout.take().map(|s| Box::new(s) as _));
    let err = read(child.stderr.take().map(|s| Box::new(s) as _));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < TIMEOUT => thread::sleep(Duration::from_millis(25)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "the data command timed out after {} s",
                    TIMEOUT.as_secs()
                ));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let stdout = out.join().unwrap_or_default();
    let stderr = err.join().unwrap_or_default();
    if status.success() {
        Ok(stdout)
    } else {
        let head: String = stderr.trim().chars().take(300).collect();
        Err(format!("the data command failed ({status}): {head}"))
    }
}

// ---------------------------------------------------------------- the envelope and the board

/// What reading one repository's data came to.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BoardOutcome {
    Ok {
        generated: Option<String>,
        repo: Option<String>,
        rows: Vec<BoardRow>,
    },
    /// The plugin reported an error as data (`{"version":1,"error":{...}}`).
    PluginError { code: String, message: String },
    /// A `version` poimen does not know: the plugin or poimen must be updated.
    Outdated { version: u64 },
    /// The command failed or printed something that is not the envelope.
    Failed { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoardRow {
    pub number: u64,
    pub title: String,
    pub url: String,
    pub assignee: Option<String>,
    pub labels: Vec<String>,
    pub branch: Option<String>,
    pub pr: Option<BoardPr>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoardPr {
    pub number: u64,
    pub url: String,
    pub state: String,
    pub checks: BoardChecks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoardChecks {
    pub state: String,
    pub passed: u64,
    pub failed: u64,
    pub pending: u64,
    pub skipped: u64,
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|s| !s.is_empty())
}

fn count(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn parse_pr(pr: &Value) -> Option<BoardPr> {
    let checks = pr.get("checks").unwrap_or(&Value::Null);
    Some(BoardPr {
        number: pr.get("number")?.as_u64()?,
        url: str_of(pr, "url").unwrap_or_default(),
        state: str_of(pr, "state").unwrap_or_default(),
        checks: BoardChecks {
            state: str_of(checks, "state").unwrap_or_else(|| "none".into()),
            passed: count(checks, "passed"),
            failed: count(checks, "failed"),
            pending: count(checks, "pending"),
            skipped: count(checks, "skipped"),
        },
    })
}

fn parse_row(row: &Value) -> Option<BoardRow> {
    Some(BoardRow {
        number: row.get("number")?.as_u64()?,
        title: str_of(row, "title").unwrap_or_default(),
        url: str_of(row, "url").unwrap_or_default(),
        assignee: str_of(row, "assignee"),
        labels: row
            .get("labels")
            .and_then(Value::as_array)
            .map(|l| {
                l.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default(),
        branch: str_of(row, "branch"),
        pr: row.get("pr").and_then(parse_pr),
    })
}

/// Reads the stdout of the deliver-board `data` action (envelope, version 1). Rows without a
/// number are skipped; an unknown `version` is reported as such, an `error` object wins over data.
pub fn parse_board(stdout: &str) -> BoardOutcome {
    let failed = |message: &str| BoardOutcome::Failed {
        message: message.to_owned(),
    };
    let Ok(env) = serde_json::from_str::<Value>(stdout.trim()) else {
        return failed("the data command did not print JSON");
    };
    let Some(version) = env.get("version").and_then(Value::as_u64) else {
        return failed("the data envelope has no version");
    };
    if let Some(error) = env.get("error").filter(|e| e.is_object()) {
        return BoardOutcome::PluginError {
            code: str_of(error, "code").unwrap_or_else(|| "error".into()),
            message: str_of(error, "message").unwrap_or_default(),
        };
    }
    if !BOARD_VERSIONS.contains(&version) {
        return BoardOutcome::Outdated { version };
    }
    if str_of(&env, "plugin").is_some_and(|p| p != BOARD_PLUGIN) {
        return failed("the data envelope belongs to another plugin");
    }
    let Some(data) = env.get("data").filter(|d| d.is_object()) else {
        return failed("the data envelope has no data");
    };
    let Some(rows) = data.get("rows").and_then(Value::as_array) else {
        return failed("the board data has no rows list");
    };
    BoardOutcome::Ok {
        generated: str_of(&env, "generated"),
        repo: str_of(data, "repo"),
        rows: rows.iter().filter_map(parse_row).collect(),
    }
}

// ---------------------------------------------------------------- the board across repositories

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RepoBoard {
    pub root: String,
    pub name: String,
    pub outcome: BoardOutcome,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum BoardView {
    /// The plugin has no native view yet (not installed, disabled, no `data` action).
    Unsupported {
        reason: String,
    },
    Repos {
        repos: Vec<RepoBoard>,
    },
}

fn repo_name(root: &str) -> String {
    Path::new(root)
        .file_name()
        .map_or_else(|| root.to_owned(), |n| n.to_string_lossy().into_owned())
}

/// Runs the board's data command once per known repository root, in parallel.
pub fn load_board(list: &Value, roots: &[String], state_dir: &Path, herdr_bin: &Path) -> BoardView {
    let cmd = match data_command(list, BOARD_PLUGIN) {
        Ok(cmd) => cmd,
        Err(reason) => return BoardView::Unsupported { reason },
    };
    let repos = thread::scope(|scope| {
        let jobs: Vec<_> = roots
            .iter()
            .map(|root| {
                let cmd = &cmd;
                scope.spawn(move || {
                    let outcome = match run_data_command(cmd, root, state_dir, herdr_bin) {
                        Ok(stdout) => parse_board(&stdout),
                        Err(message) => BoardOutcome::Failed { message },
                    };
                    RepoBoard {
                        root: root.clone(),
                        name: repo_name(root),
                        outcome,
                    }
                })
            })
            .collect();
        jobs.into_iter().filter_map(|j| j.join().ok()).collect()
    });
    BoardView::Repos { repos }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Shape of the live `plugin.list` entry (herdr 0.9.5, protocol 22), with a `data` action.
    fn list(with_data: bool) -> Value {
        let mut actions =
            vec![json!({"id": "open", "title": "Open", "command": ["node", "bin/open.ts"]})];
        if with_data {
            actions
                .push(json!({"id": "data", "title": "Data", "command": ["node", "bin/data.ts"]}));
        }
        json!({"type": "plugin_list", "plugins": [{
            "plugin_id": "deliver-board", "enabled": true,
            "manifest_path": "/p/checkout/deliver-board/herdr-plugin.toml",
            "plugin_root": "/p/checkout/deliver-board",
            "actions": actions
        }]})
    }

    #[test]
    fn the_data_command_runs_in_the_plugin_root() {
        let cmd = data_command(&list(true), "deliver-board").unwrap();
        assert_eq!(cmd.dir, PathBuf::from("/p/checkout/deliver-board"));
        assert_eq!(cmd.argv, ["node", "bin/data.ts"]);
    }

    #[test]
    fn the_manifest_path_is_a_file_so_its_parent_is_the_directory() {
        let mut l = list(true);
        l["plugins"][0]
            .as_object_mut()
            .unwrap()
            .remove("plugin_root");
        let cmd = data_command(&l, "deliver-board").unwrap();
        assert_eq!(cmd.dir, PathBuf::from("/p/checkout/deliver-board"));
    }

    #[test]
    fn a_plugin_without_data_action_is_unsupported() {
        let err = data_command(&list(false), "deliver-board").unwrap_err();
        assert!(err.contains("no `data` action"), "{err}");
        assert!(
            data_command(&list(true), "other")
                .unwrap_err()
                .contains("not installed")
        );
        let mut off = list(true);
        off["plugins"][0]["enabled"] = json!(false);
        assert!(
            data_command(&off, "deliver-board")
                .unwrap_err()
                .contains("disabled")
        );
        assert!(data_command(&json!({}), "deliver-board").is_err());
    }

    #[test]
    fn the_path_puts_the_shell_path_first_and_drops_duplicates() {
        let path = augmented_path(Some("/a:/b::/a"), "/nonexistent-home", "/usr/bin:/b");
        let parts: Vec<&str> = path.split(':').collect();
        assert_eq!(&parts[..2], ["/a", "/b"]);
        assert!(parts.contains(&"/opt/homebrew/bin"));
        assert!(parts.contains(&"/usr/bin"));
        assert_eq!(parts.iter().filter(|p| **p == "/b").count(), 1);
        assert!(augmented_path(None, "/h", "").contains("/h/.local/bin"));
    }

    #[test]
    fn programs_resolve_against_the_given_path() {
        let dir = std::env::temp_dir().join(format!("poimen-pv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("fake-node"), "").unwrap();
        let path = format!("/nonexistent:{}", dir.display());
        assert_eq!(
            resolve_program("fake-node", &path),
            Some(dir.join("fake-node"))
        );
        assert_eq!(resolve_program("missing-node", &path), None);
        assert_eq!(
            resolve_program("/abs/node", ""),
            Some(PathBuf::from("/abs/node"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    const OK: &str = include_str!("../fixtures/deliver-board-data-v1.json");

    #[test]
    fn the_board_fixture_of_the_adr_is_parsed() {
        let BoardOutcome::Ok {
            generated,
            repo,
            rows,
        } = parse_board(OK)
        else {
            panic!("expected data");
        };
        assert_eq!(generated.as_deref(), Some("2026-11-02T10:00:00Z"));
        assert_eq!(repo.as_deref(), Some("GravionLabs/poimen"));
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].number, 42);
        assert_eq!(rows[0].assignee.as_deref(), Some("jkaufmann"));
        assert_eq!(rows[0].labels, ["in-progress"]);
        let pr = rows[0].pr.as_ref().unwrap();
        assert_eq!((pr.number, pr.state.as_str()), (150, "OPEN"));
        assert_eq!(
            (
                pr.checks.state.as_str(),
                pr.checks.passed,
                pr.checks.pending
            ),
            ("pending", 3, 1)
        );
        assert_eq!(rows[1].assignee, None);
        assert_eq!(rows[1].branch, None);
        assert_eq!(rows[1].pr, None);
    }

    #[test]
    fn rows_without_a_number_are_skipped() {
        let out = r#"{"version":1,"plugin":"deliver-board","data":{"rows":[{"title":"x"},{"number":3}]}}"#;
        let BoardOutcome::Ok { rows, .. } = parse_board(out) else {
            panic!("expected data");
        };
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].number, 3);
    }

    #[test]
    fn a_plugin_error_is_data() {
        let out = r#"{"version":1,"error":{"code":"no-gh-login","message":"run gh auth login"}}"#;
        assert_eq!(
            parse_board(out),
            BoardOutcome::PluginError {
                code: "no-gh-login".into(),
                message: "run gh auth login".into()
            }
        );
    }

    #[test]
    fn unknown_versions_and_broken_output_are_reported() {
        assert_eq!(
            parse_board(r#"{"version":2,"plugin":"deliver-board","data":{"rows":[]}}"#),
            BoardOutcome::Outdated { version: 2 }
        );
        for bad in [
            "",
            "not json",
            "{}",
            r#"{"version":1,"plugin":"deliver-board"}"#,
            r#"{"version":1,"plugin":"deliver-board","data":{}}"#,
            r#"{"version":1,"plugin":"other","data":{"rows":[]}}"#,
        ] {
            assert!(
                matches!(parse_board(bad), BoardOutcome::Failed { .. }),
                "{bad}"
            );
        }
    }

    #[test]
    fn a_board_without_a_data_action_serializes_as_unsupported() {
        let view = load_board(&list(false), &[], Path::new("/s"), Path::new("/herdr"));
        let json = serde_json::to_value(&view).unwrap();
        assert_eq!(json["status"], "unsupported");
        assert!(
            json["reason"]
                .as_str()
                .unwrap()
                .contains("update the plugin")
        );
    }

    #[test]
    fn a_missing_program_fails_with_a_message() {
        let cmd = DataCommand {
            dir: PathBuf::from("/"),
            argv: vec!["poimen-no-such-program".into()],
        };
        let err =
            run_data_command(&cmd, "/r", Path::new("/nonexistent/s"), Path::new("/h")).unwrap_err();
        assert!(err.contains("was not found"), "{err}");
    }
}
