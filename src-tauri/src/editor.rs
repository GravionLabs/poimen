//! "Open in editor" (#1002): the `editor` setting of `config.json`, the presets, and the launch.
//!
//! `editor` is a preset id (`"vscode"`, `"zed"`, `"cursor"`, `"idea"`, ...) or an argv template,
//! an array of strings with `{path}`, `{line}` and `{column}`. The command runs without a shell:
//! each element is one argument. Without the setting, the editors found on the login-shell `PATH`
//! are offered.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::gui_path;

/// A known editor: the program, and the arguments with and without a line.
struct Preset {
    id: &'static str,
    name: &'static str,
    program: &'static str,
    /// Arguments for a path with a position.
    at: &'static [&'static str],
    /// Arguments for a path only (a folder, or a file without a line).
    plain: &'static [&'static str],
}

const JETBRAINS_AT: &[&str] = &["--line", "{line}", "--column", "{column}", "{path}"];
const JETBRAINS_PLAIN: &[&str] = &["{path}"];

const fn jetbrains(id: &'static str, name: &'static str, program: &'static str) -> Preset {
    Preset {
        id,
        name,
        program,
        at: JETBRAINS_AT,
        plain: JETBRAINS_PLAIN,
    }
}

const PRESETS: &[Preset] = &[
    Preset {
        id: "vscode",
        name: "VS Code",
        program: "code",
        at: &["--goto", "{path}:{line}:{column}"],
        plain: &["{path}"],
    },
    Preset {
        id: "cursor",
        name: "Cursor",
        program: "cursor",
        at: &["--goto", "{path}:{line}:{column}"],
        plain: &["{path}"],
    },
    Preset {
        id: "zed",
        name: "Zed",
        program: "zed",
        at: &["{path}:{line}:{column}"],
        plain: &["{path}"],
    },
    jetbrains("idea", "IntelliJ IDEA", "idea"),
    jetbrains("webstorm", "WebStorm", "webstorm"),
    jetbrains("pycharm", "PyCharm", "pycharm"),
    jetbrains("goland", "GoLand", "goland"),
    jetbrains("clion", "CLion", "clion"),
    jetbrains("rustrover", "RustRover", "rustrover"),
];

/// What the `editor` setting says.
#[derive(Debug, Clone, PartialEq)]
pub enum Choice {
    Preset(&'static str),
    Argv(Vec<String>),
}

/// Reads `editor` from the config text: `Ok(None)` when absent, `Err` when it is invalid.
pub fn parse_setting(config: &str) -> Result<Option<Choice>, String> {
    let Ok(root) = serde_json::from_str::<Value>(config) else {
        return Ok(None);
    };
    match root.get("editor") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(id)) => preset(id.trim())
            .map(|p| Some(Choice::Preset(p.id)))
            .ok_or_else(|| {
                let ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
                format!(
                    "editor \"{id}\" is not a preset ({}); use a preset or an argv array",
                    ids.join(", ")
                )
            }),
        Some(Value::Array(items)) => {
            let argv: Option<Vec<String>> = items
                .iter()
                .map(|v| v.as_str().map(str::to_owned))
                .collect();
            match argv {
                Some(argv) if !argv.is_empty() && !argv[0].trim().is_empty() => {
                    Ok(Some(Choice::Argv(argv)))
                }
                _ => Err("editor must be a non-empty array of strings".into()),
            }
        }
        Some(other) => Err(format!(
            "editor {other} must be a preset id or an array of strings"
        )),
    }
}

fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.id == id)
}

/// The program and arguments for `choice`; `{path}`, `{line}` and `{column}` are replaced inside
/// each element. A custom template gets 1 for a missing line or column.
pub fn build_argv(
    choice: &Choice,
    path: &str,
    position: Option<(u32, u32)>,
) -> (String, Vec<String>) {
    let (program, template): (String, Vec<String>) = match choice {
        Choice::Preset(id) => {
            let p = preset(id).expect("a Choice::Preset holds a known id");
            let args = if position.is_some() { p.at } else { p.plain };
            (
                p.program.to_owned(),
                args.iter().map(|a| (*a).to_owned()).collect(),
            )
        }
        Choice::Argv(argv) => (argv[0].clone(), argv[1..].to_vec()),
    };
    let (line, column) = position.unwrap_or((1, 1));
    let args = template
        .iter()
        .map(|a| {
            a.replace("{path}", path)
                .replace("{line}", &line.to_string())
                .replace("{column}", &column.to_string())
        })
        .collect();
    (program, args)
}

/// `program` as an executable file on the directories of `search_path`.
pub fn find_program(program: &str, search_path: &str) -> Option<PathBuf> {
    let given = Path::new(program);
    if given.components().count() > 1 {
        return given.is_file().then(|| given.to_path_buf());
    }
    let extensions: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    std::env::split_paths(search_path).find_map(|dir| {
        extensions
            .iter()
            .map(|ext| dir.join(format!("{program}{ext}")))
            .find(|candidate| candidate.is_file())
    })
}

/// One editor found on the `PATH`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Found {
    pub id: &'static str,
    pub name: &'static str,
}

pub fn detect(search_path: &str) -> Vec<Found> {
    PRESETS
        .iter()
        .filter(|p| find_program(p.program, search_path).is_some())
        .map(|p| Found {
            id: p.id,
            name: p.name,
        })
        .collect()
}

fn read_config() -> String {
    crate::home_dir()
        .and_then(|home| std::fs::read_to_string(poimen_core::settings::config_path(&home)).ok())
        .unwrap_or_default()
}

/// The program, arguments and resolved binary for opening `path`. The `editor` setting wins; without
/// it the first editor found on `search_path` is used.
pub fn launch(
    config: &str,
    search_path: &str,
    path: &str,
    line: Option<u32>,
    column: Option<u32>,
) -> Result<(String, Vec<String>, PathBuf), String> {
    if !Path::new(path).exists() {
        return Err(format!("{path} does not exist"));
    }
    let choice = match parse_setting(config)? {
        Some(choice) => choice,
        None => match detect(search_path).first() {
            Some(found) => Choice::Preset(found.id),
            None => {
                return Err(
                    "no editor found on the PATH; set \"editor\" in config.json (Settings)".into(),
                );
            }
        },
    };
    let position = line.map(|l| (l.max(1), column.unwrap_or(1).max(1)));
    let (program, args) = build_argv(&choice, path, position);
    let resolved = find_program(&program, search_path).ok_or_else(|| {
        format!("the editor \"{program}\" was not found on the PATH; set \"editor\" in config.json (Settings)")
    })?;
    Ok((program, args, resolved))
}

/// Opens `path` (at `line` and `column`) in the editor. With `machine` the path is on that machine
/// (#1005, connected now) and opens over ssh in VS Code Remote-SSH or Zed.
#[tauri::command]
pub fn open_in_editor(
    machines: tauri::State<'_, std::sync::Arc<crate::machines::Machines>>,
    path: String,
    line: Option<u32>,
    column: Option<u32>,
    machine: Option<String>,
) -> Result<(), String> {
    let search_path = gui_path::child_path();
    if let Some(machine) = machine {
        machines.connected_backend(&machine)?;
        let ssh = machines.config(&machine)?.ssh;
        let (_, args, resolved) = launch_remote(&read_config(), &search_path, &ssh, &path)?;
        return spawn_detached(&resolved, args, &search_path);
    }
    let (_, args, resolved) = launch(&read_config(), &search_path, &path, line, column)?;
    spawn_detached(&resolved, args, &search_path)
}

/// Starts the launcher without a shell and reaps it on a thread.
fn spawn_detached(resolved: &Path, args: Vec<String>, search_path: &str) -> Result<(), String> {
    let mut command = gui_path::command(resolved);
    command
        .args(args)
        .env("PATH", search_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut child = command
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", resolved.display()))?;
    // Reap the launcher (it usually returns at once) without blocking the command.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// The editors that open a folder on a machine over ssh (#1005), in the order they are tried.
const REMOTE_PRESETS: &[&str] = &["vscode", "zed"];

/// The program and arguments that open `path` on the machine `ssh` (a `~/.ssh/config` host or
/// `user@host`) in the preset `id`: VS Code with the Remote-SSH authority `ssh-remote+<ssh>`, Zed
/// with `ssh://<ssh><path>`. `None` for an editor without remote support.
pub fn remote_argv(id: &str, ssh: &str, path: &str) -> Option<(String, Vec<String>)> {
    let program = preset(id)?.program.to_owned();
    let args = match id {
        "vscode" => vec![
            "--remote".to_owned(),
            format!("ssh-remote+{ssh}"),
            path.to_owned(),
        ],
        "zed" => vec![format!("ssh://{ssh}{}", percent_encode_path(path))],
        _ => return None,
    };
    Some((program, args))
}

/// `path` with every byte outside `A-Z a-z 0-9 - . _ ~ /` as `%XX` (a URL path).
fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The program, arguments and resolved binary for opening the remote `path` on `ssh`. The `editor`
/// setting wins and has to be a preset with remote support; without it the first of those found on
/// `search_path` is used.
pub fn launch_remote(
    config: &str,
    search_path: &str,
    ssh: &str,
    path: &str,
) -> Result<(String, Vec<String>, PathBuf), String> {
    if !path.starts_with('/') {
        return Err(format!("{path} is not an absolute path on the machine"));
    }
    if ssh.is_empty() || ssh.starts_with('-') {
        return Err(format!("{ssh:?} is not an ssh target"));
    }
    let id = match parse_setting(config)? {
        Some(Choice::Preset(id)) if REMOTE_PRESETS.contains(&id) => id,
        Some(_) => {
            return Err(
                "the editor in config.json cannot open a remote space; use \"vscode\" or \"zed\""
                    .into(),
            );
        }
        None => REMOTE_PRESETS
            .iter()
            .copied()
            .find(|id| preset(id).is_some_and(|p| find_program(p.program, search_path).is_some()))
            .ok_or("no editor with remote support (VS Code, Zed) found on the PATH")?,
    };
    let (program, args) = remote_argv(id, ssh, path).expect("a remote preset has an argv");
    let resolved = find_program(&program, search_path).ok_or_else(|| {
        format!("the editor \"{program}\" was not found on the PATH; set \"editor\" in config.json (Settings)")
    })?;
    Ok((program, args, resolved))
}

/// A path found in terminal text that is a file (#1003).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FileLink {
    /// The file, absolute.
    pub path: String,
    /// The root of the git worktree that holds it and its path there (`/` separators).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<RepoFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoFile {
    pub root: String,
    pub rel: String,
}

/// The file `text` names, relative to `dir` (a leading `~/` is the home directory); `None` when
/// it is not an existing file.
pub fn resolve_file(dir: &str, text: &str, home: Option<&Path>) -> Option<PathBuf> {
    if text.is_empty() || text.contains('\0') {
        return None;
    }
    let path = match (text.strip_prefix("~/"), home) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => Path::new(dir).join(text),
    };
    path.is_file().then_some(path)
}

fn repo_file(path: &Path) -> Option<RepoFile> {
    let parent = path.parent()?.to_str()?;
    let out =
        poimen_core::git::run_bytes(parent, &["rev-parse", "--show-toplevel", "--show-prefix"])
            .ok()?;
    let out = String::from_utf8_lossy(&out);
    let mut lines = out.lines();
    let root = lines.next()?.trim().to_owned();
    let prefix = lines.next().unwrap_or("").trim();
    let name = path.file_name()?.to_str()?;
    (!root.is_empty()).then(|| RepoFile {
        root,
        rel: format!("{prefix}{name}"),
    })
}

/// For each of `paths` (as written in the terminal) the file it names, or `null`. Read only.
#[tauri::command]
pub async fn resolve_file_links(dir: String, paths: Vec<String>) -> Vec<Option<FileLink>> {
    tauri::async_runtime::spawn_blocking(move || {
        let home = std::env::home_dir();
        paths
            .iter()
            .take(64)
            .map(|text| {
                let path = resolve_file(&dir, text, home.as_deref())?;
                Some(FileLink {
                    repo: repo_file(&path),
                    path: path.to_string_lossy().into_owned(),
                })
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn setting_is_a_preset_or_an_argv() {
        assert_eq!(parse_setting("{}"), Ok(None));
        assert_eq!(
            parse_setting(r#"{"editor":"zed"}"#),
            Ok(Some(Choice::Preset("zed")))
        );
        assert_eq!(
            parse_setting(r#"{"editor":["ed","{path}"]}"#),
            Ok(Some(Choice::Argv(vec!["ed".into(), "{path}".into()])))
        );
        assert!(parse_setting(r#"{"editor":"nano"}"#).is_err());
        assert!(parse_setting(r#"{"editor":[]}"#).is_err());
        assert!(parse_setting(r#"{"editor":[1]}"#).is_err());
        assert!(parse_setting(r#"{"editor":3}"#).is_err());
    }

    #[test]
    fn presets_put_the_position_in_their_own_form() {
        let at = Some((12, 3));
        let (p, a) = build_argv(&Choice::Preset("vscode"), "/r/a.rs", at);
        assert_eq!(
            (p.as_str(), a),
            ("code", vec!["--goto".into(), "/r/a.rs:12:3".into()])
        );
        let (_, a) = build_argv(&Choice::Preset("zed"), "/r/a.rs", at);
        assert_eq!(a, vec!["/r/a.rs:12:3"]);
        let (_, a) = build_argv(&Choice::Preset("idea"), "/r/a.rs", at);
        assert_eq!(a, vec!["--line", "12", "--column", "3", "/r/a.rs"]);
        let (_, a) = build_argv(&Choice::Preset("vscode"), "/r", None);
        assert_eq!(a, vec!["/r"]);
    }

    #[test]
    fn a_template_replaces_inside_elements_and_never_splits_on_spaces() {
        let choice = Choice::Argv(vec!["ed".into(), "+{line}".into(), "{path}".into()]);
        let (p, a) = build_argv(&choice, "/a b/c; rm -rf", Some((4, 2)));
        assert_eq!(p, "ed");
        assert_eq!(a, vec!["+4", "/a b/c; rm -rf"]);
        let (_, a) = build_argv(&choice, "/x", None);
        assert_eq!(a, vec!["+1", "/x"]);
    }

    #[cfg(unix)]
    #[test]
    fn programs_are_found_on_the_given_path_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("poimen-editor-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("zed");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        let search = dir.to_string_lossy().into_owned();
        assert_eq!(find_program("zed", &search), Some(bin));
        assert_eq!(find_program("code", &search), None);
        let found = detect(&search);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "zed");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_editor_names_the_setting() {
        let here = std::env::temp_dir();
        let path = here.to_string_lossy();
        let err = launch(r#"{"editor":"zed"}"#, "", &path, None, None).unwrap_err();
        assert!(err.contains("\"editor\""), "{err}");
        let err = launch("{}", "", &path, None, None).unwrap_err();
        assert!(err.contains("\"editor\""), "{err}");
        let err = launch("{}", "", "/definitely/not/here", None, None).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }

    #[test]
    fn only_existing_files_resolve() {
        let dir = std::env::temp_dir().join(format!("poimen-links-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.ts"), "x").unwrap();
        let d = dir.to_string_lossy().into_owned();
        assert_eq!(
            resolve_file(&d, "src/a.ts", None),
            Some(dir.join("src/a.ts"))
        );
        assert_eq!(resolve_file(&d, "src", None), None);
        assert_eq!(resolve_file(&d, "src/b.ts", None), None);
        assert_eq!(resolve_file(&d, "", None), None);
        assert_eq!(
            resolve_file("/elsewhere", "~/src/a.ts", Some(&dir)),
            Some(dir.join("src/a.ts"))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn remote_editors_get_the_ssh_authority_or_url() {
        assert_eq!(
            remote_argv("vscode", "box", "/srv/my repo"),
            Some((
                "code".into(),
                vec![
                    "--remote".into(),
                    "ssh-remote+box".into(),
                    "/srv/my repo".into()
                ]
            ))
        );
        assert_eq!(
            remote_argv("zed", "me@box", "/srv/my repo"),
            Some(("zed".into(), vec!["ssh://me@box/srv/my%20repo".into()]))
        );
        assert_eq!(remote_argv("idea", "box", "/r"), None);
    }

    #[test]
    fn remote_launch_needs_a_remote_editor_and_a_clean_target() {
        let bad = launch_remote(r#"{"editor":"idea"}"#, "", "box", "/r").unwrap_err();
        assert!(bad.contains("cannot open a remote space"));
        let argv = launch_remote(r#"{"editor":["ed","{path}"]}"#, "", "box", "/r");
        assert!(argv.is_err());
        assert!(launch_remote("{}", "", "-oProxy", "/r").is_err());
        assert!(launch_remote("{}", "", "box", "relative").is_err());
        let none = launch_remote("{}", "/nonexistent", "box", "/r").unwrap_err();
        assert!(none.contains("remote support"));
    }
}
