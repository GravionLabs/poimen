//! The parts of the user settings (`~/.config/poimen/config.json`) that the project-side modules
//! need: the `tasks` and `spaceTemplates` parsers, the trusted projects, and the editor that
//! changes one key of the file in place and keeps comments, formatting and every other key.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

/// What a `tasks` object specifies: `None` where it says nothing (or nothing valid).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskOverride {
    pub modes: Option<Vec<crate::tasks::TaskMode>>,
    pub worktree_dir: Option<String>,
}

/// `trustedProjects`: root to hash; entries that are not strings are skipped with a warning.
pub fn parse_trusted_projects(
    value: &Value,
    warnings: &mut Vec<String>,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let Some(obj) = value.as_object() else {
        warnings.push("trustedProjects must be an object".into());
        return out;
    };
    for (root, hash) in obj {
        match hash.as_str() {
            Some(h) if !h.is_empty() => {
                out.insert(root.clone(), h.to_owned());
            }
            _ => warnings.push(format!(
                "trustedProjects: {root}: the hash must be a string"
            )),
        }
    }
    out
}

/// The `trustedProjects` of the config file under `home`; a missing or invalid file gives none.
pub fn trusted_projects(home: &Path) -> BTreeMap<String, String> {
    let Ok(text) = std::fs::read_to_string(config_path(home)) else {
        return BTreeMap::new();
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(root) => root
            .get("trustedProjects")
            .map(|v| parse_trusted_projects(v, &mut Vec::new()))
            .unwrap_or_default(),
        Err(_) => BTreeMap::new(),
    }
}

/// A token on a sidebar row line (`sidebar.rows`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RowToken {
    pub token: String,
    pub bold: bool,
    pub dim: bool,
}

/// Lines of tokens per row type; `None` keeps the frontend default of that row type.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SidebarRows {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<Vec<Vec<RowToken>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<Vec<Vec<RowToken>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<Vec<Vec<RowToken>>>,
}

/// Built-in row tokens; `$<name>` shows the herdr token `<name>`.
pub const ROW_TOKENS: &[&str] = &[
    "state_icon",
    "name",
    "state_text",
    "agent",
    "title",
    "branch",
    "dirty",
    "sync",
    "pr",
    "notification",
    "dir",
    "tab",
];

fn parse_row_token(value: &Value) -> Result<RowToken, String> {
    let (name, bold, dim) = match value {
        Value::String(s) => (s.as_str(), false, false),
        Value::Object(o) => {
            let Some(Value::String(name)) = o.get("token") else {
                return Err("an object token needs a string \"token\"".into());
            };
            let flag = |key: &str| match o.get(key) {
                None => Ok(false),
                Some(Value::Bool(b)) => Ok(*b),
                Some(other) => Err(format!("{key} {other} must be true or false")),
            };
            (name.as_str(), flag("bold")?, flag("dim")?)
        }
        other => return Err(format!("token {other} must be a string or an object")),
    };
    let known = ROW_TOKENS.contains(&name)
        || name
            .strip_prefix('$')
            .is_some_and(|n| !n.is_empty() && !n.contains(char::is_whitespace));
    if !known {
        return Err(format!("unknown token \"{name}\""));
    }
    Ok(RowToken {
        token: name.to_owned(),
        bold,
        dim,
    })
}

pub fn parse_row_lines(value: &Value) -> Result<Vec<Vec<RowToken>>, String> {
    let Some(lines) = value.as_array().filter(|l| !l.is_empty()) else {
        return Err("must be a non-empty list of lines".into());
    };
    lines
        .iter()
        .map(|line| {
            let Some(tokens) = line.as_array().filter(|t| !t.is_empty()) else {
                return Err("each line must be a non-empty list of tokens".into());
            };
            tokens.iter().map(parse_row_token).collect()
        })
        .collect()
}

/// The valid row types of `sidebar.rows` (the `sidebar` object of the config); an invalid one warns and stays `None` (the default).
pub fn resolve_sidebar_rows(sidebar: &Value, warnings: &mut Vec<String>) -> SidebarRows {
    let mut out = SidebarRows::default();
    let Some(sidebar) = sidebar.as_object() else {
        warnings.push("sidebar must be an object".into());
        return out;
    };
    let Some(rows) = sidebar.get("rows") else {
        return out;
    };
    let Some(rows) = rows.as_object() else {
        warnings.push("sidebar.rows must be an object".into());
        return out;
    };
    for (kind, value) in rows {
        let slot = match kind.as_str() {
            "workspace" => &mut out.workspace,
            "agent" => &mut out.agent,
            "terminal" => &mut out.terminal,
            _ => {
                warnings.push(format!(
                    "sidebar.rows.{kind} is not a known row type; ignored"
                ));
                continue;
            }
        };
        match parse_row_lines(value) {
            Ok(lines) => *slot = Some(lines),
            Err(e) => warnings.push(format!("sidebar.rows.{kind}: {e}; using the default")),
        }
    }
    out
}

/// Index after a `"..."` string that starts at `i`.
pub fn skip_string(b: &[u8], mut i: usize) -> usize {
    i += 1;
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    b.len()
}

/// Index after whitespace and `//` / `/* */` comments starting at `i`.
pub fn skip_trivia(b: &[u8], mut i: usize) -> usize {
    loop {
        match b.get(i) {
            Some(c) if c.is_ascii_whitespace() => i += 1,
            Some(b'/') if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            Some(b'/') if b.get(i + 1) == Some(&b'*') => {
                i += 2;
                while i < b.len() && !b[i..].starts_with(b"*/") {
                    i += 1;
                }
                i = (i + 2).min(b.len());
            }
            _ => return i,
        }
    }
}

/// Index after the JSON value that starts at `start`: a string, an object or array up to its
/// matching close (strings and comments inside skipped), else a scalar up to the next delimiter.
/// A comment after the value is not part of it.
fn value_end(b: &[u8], start: usize) -> usize {
    match b.get(start) {
        Some(b'"') => skip_string(b, start),
        Some(b'{' | b'[') => {
            let (mut e, mut nest) = (start, 0usize);
            while e < b.len() {
                match b[e] {
                    b'"' => {
                        e = skip_string(b, e);
                        continue;
                    }
                    b'/' if matches!(b.get(e + 1), Some(b'/' | b'*')) => {
                        e = skip_trivia(b, e);
                        continue;
                    }
                    b'{' | b'[' => nest += 1,
                    b'}' | b']' => {
                        nest -= 1;
                        if nest == 0 {
                            return e + 1;
                        }
                    }
                    _ => {}
                }
                e += 1;
            }
            e
        }
        _ => {
            let mut e = start;
            while e < b.len()
                && !matches!(b[e], b',' | b'}' | b']' | b'/')
                && !b[e].is_ascii_whitespace()
            {
                e += 1;
            }
            e
        }
    }
}

/// Byte range of the value of `key` in the object that opens at `open` (a `{`), if present.
pub fn find_value(text: &str, open: usize, key: &str) -> Option<(usize, usize)> {
    find_entry(text, open, key).map(|(_, start, stop)| (start, stop))
}

/// Like `find_value`, plus the start of the quoted key: `(key, value start, value stop)`.
pub fn find_entry(text: &str, open: usize, key: &str) -> Option<(usize, usize, usize)> {
    let quoted = format!("\"{key}\"");
    let b = text.as_bytes();
    let (mut i, mut depth) = (open + 1, 1usize);
    while i < b.len() {
        i = skip_trivia(b, i);
        match b.get(i) {
            None => break,
            Some(b'"') => {
                let end = skip_string(b, i);
                let is_key = depth == 1 && text[i..end] == quoted;
                let key_start = i;
                i = end;
                if is_key {
                    let colon = skip_trivia(b, i);
                    if b.get(colon) == Some(&b':') {
                        let start = skip_trivia(b, colon + 1);
                        return Some((key_start, start, value_end(b, start)));
                    }
                }
            }
            Some(b'{' | b'[') => {
                depth += 1;
                i += 1;
            }
            Some(b'}' | b']') => {
                depth -= 1;
                i += 1;
                if depth == 0 {
                    break;
                }
            }
            Some(_) => i += 1,
        }
    }
    None
}

/// Sets `key` of the object opening at `open` to the JSON `value`; a missing key is inserted as
/// the first entry.
pub fn set_key_at(text: &str, open: usize, key: &str, value: &str) -> String {
    if let Some((start, stop)) = find_value(text, open, key) {
        return format!("{}{value}{}", &text[..start], &text[stop..]);
    }
    let first = skip_trivia(text.as_bytes(), open + 1);
    let entry = format!("\"{key}\": {value}");
    if text.as_bytes().get(first) == Some(&b'}') {
        format!("{}{entry}{}", &text[..=open], &text[first..])
    } else {
        format!("{}\n  {entry},{}", &text[..=open], &text[open + 1..])
    }
}

/// Start of the top-level object, or an error when the text is something else.
pub fn root_open(text: &str) -> Result<usize, String> {
    let open = skip_trivia(text.as_bytes(), 0);
    if text.as_bytes().get(open) == Some(&b'{') {
        Ok(open)
    } else {
        Err("config.json is not a JSON object".into())
    }
}

/// Sets the top-level `key` to the JSON `value` (already serialized), touching nothing else. A
/// missing key is inserted as the first entry.
pub fn set_key_text(text: &str, key: &str, value: &str) -> Result<String, String> {
    Ok(set_key_at(text, root_open(text)?, key, value))
}

/// Sets the value at the key `path` (outermost first) of the object opening at `open`. Missing
/// objects on the way are created; a key on the way that is not an object is an error.
pub fn set_path_at(text: &str, open: usize, path: &[&str], value: &str) -> Result<String, String> {
    let (key, rest) = path.split_first().ok_or("empty key path")?;
    if rest.is_empty() {
        return Ok(set_key_at(text, open, key, value));
    }
    match find_value(text, open, key) {
        None => {
            let nested = rest.iter().rev().fold(value.to_owned(), |inner, k| {
                format!("{{ \"{k}\": {inner} }}")
            });
            Ok(set_key_at(text, open, key, &nested))
        }
        Some((start, _)) if text.as_bytes().get(start) == Some(&b'{') => {
            set_path_at(text, start, rest, value)
        }
        Some(_) => Err(format!("{key} in config.json is not an object")),
    }
}

/// Sets the nested key `path` (e.g. `["sidebar", "layout"]`) to the JSON `value`, touching nothing
/// else: other keys, comments and formatting stay.
pub fn set_path_text(text: &str, path: &[&str], value: &str) -> Result<String, String> {
    set_path_at(text, root_open(text)?, path, value)
}

/// Index before the whitespace (not comments) that ends just before `i`.
pub fn trim_ws_back(b: &[u8], mut i: usize) -> usize {
    while i > 0 && b[i - 1].is_ascii_whitespace() {
        i -= 1;
    }
    i
}

/// Removes the nested key `path` with its value and one separating comma, touching nothing else.
/// A missing key (or a missing object on the way) leaves the text unchanged.
pub fn remove_path_text(text: &str, path: &[&str]) -> Result<String, String> {
    let b = text.as_bytes();
    let mut open = root_open(text)?;
    let (last, parents) = path.split_last().ok_or("empty key path")?;
    for key in parents {
        match find_value(text, open, key) {
            None => return Ok(text.to_owned()),
            Some((start, _)) if b.get(start) == Some(&b'{') => open = start,
            Some(_) => return Err(format!("{key} in config.json is not an object")),
        }
    }
    let Some((key_start, _, stop)) = find_entry(text, open, last) else {
        return Ok(text.to_owned());
    };
    let stop = trim_ws_back(b, stop).max(key_start);
    let mut after = stop;
    while after < b.len() && b[after].is_ascii_whitespace() {
        after += 1;
    }
    if b.get(after) == Some(&b',') {
        // Not the last entry: drop the entry, its comma and the whitespace before it.
        let from = trim_ws_back(b, key_start);
        return Ok(format!("{}{}", &text[..from], &text[after + 1..]));
    }
    // The last entry: drop the comma before it, if any.
    let before = trim_ws_back(b, key_start);
    let from = if before > 0 && b[before - 1] == b',' {
        before - 1
    } else {
        before
    };
    Ok(format!("{}{}", &text[..from], &text[stop..]))
}

/// Returns `text` with `trustedProjects` replaced by `projects`, touching nothing else (comments
/// inside the old `trustedProjects` object go with it).
pub fn set_trusted_projects_text(
    text: &str,
    projects: &BTreeMap<String, String>,
) -> Result<String, String> {
    let value = serde_json::to_string(projects).map_err(|e| e.to_string())?;
    set_key_text(text, "trustedProjects", &value)
}

/// The `trustedProjects` of the config `text`, read from that value alone (comments elsewhere in
/// the file do not matter). An entry that is not valid JSON is an error, so a write never drops
/// the entries it could not read.
pub fn trusted_projects_text(text: &str) -> Result<BTreeMap<String, String>, String> {
    let Some((start, _)) = find_value(text, root_open(text)?, "trustedProjects") else {
        return Ok(BTreeMap::new());
    };
    let value = serde_json::Deserializer::from_str(&text[start..])
        .into_iter::<Value>()
        .next()
        .and_then(Result::ok)
        .ok_or("trustedProjects in config.json is not valid JSON; it was not changed")?;
    if !value.is_object() {
        return Err("trustedProjects in config.json is not an object; it was not changed".into());
    }
    Ok(parse_trusted_projects(&value, &mut Vec::new()))
}

/// Stores `hash` as the trusted content of the project file of `root`. The file is read right
/// before it is written and only `trustedProjects` changes, so a concurrent edit of another key
/// (the app, the server, the user) is kept.
pub fn trust_project(home: &Path, root: &str, hash: &str) -> Result<(), String> {
    save_with(home, |text| {
        let mut projects = trusted_projects_text(text)?;
        projects.insert(root.to_owned(), hash.to_owned());
        set_trusted_projects_text(text, &projects)
    })
}

/// Drops the trust of `root`; an unknown root or a missing file changes nothing.
pub fn revoke_project(home: &Path, root: &str) -> Result<(), String> {
    match std::fs::read_to_string(config_path(home)) {
        Ok(text) if trusted_projects_text(&text)?.contains_key(root) => {}
        Ok(_) => return Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(format!("cannot read config.json: {e}")),
    }
    save_with(home, |text| {
        let mut projects = trusted_projects_text(text)?;
        if projects.remove(root).is_none() {
            return Ok(text.to_owned());
        }
        set_trusted_projects_text(text, &projects)
    })
}

/// Reads `config.json` under `home`, applies `edit` to its text and writes the result through a
/// temporary file of its own (the app and the server both write the file). Every writer of the
/// file goes through here: the text is read right before the write and `edit` changes one key,
/// so keys and comments it does not touch stay.
pub fn save_with(home: &Path, edit: impl Fn(&str) -> Result<String, String>) -> Result<(), String> {
    static NEXT_TMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let path = config_path(home);
    let next = match std::fs::read_to_string(&path) {
        Ok(text) => edit(&text)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => edit("{}").map(|t| t + "\n")?,
        Err(e) => return Err(format!("cannot read config.json: {e}")),
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let n = NEXT_TMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = path.with_extension(format!("json.{}-{n}.tmp", std::process::id()));
    std::fs::write(&tmp, next).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

/// poimen's own files live under `<home>/.config/poimen` on every system, Windows included
/// (deliberately not `%APPDATA%`: one documented location, like `~/.config/poimen/themes`). An
/// rc build keeps them apart in `<home>/.config/poimen-rc` ([`poimen_protocol::CHANNEL`]).
pub fn config_dir(home: &Path) -> PathBuf {
    config_dir_for(home, poimen_protocol::CHANNEL)
}

/// [`config_dir`] of `channel`.
pub fn config_dir_for(home: &Path, channel: poimen_protocol::Channel) -> PathBuf {
    home.join(".config").join(channel.named("poimen"))
}

/// `config.json` in [`config_dir`].
pub fn config_path(home: &Path) -> PathBuf {
    config_dir(home).join("config.json")
}

/// Parses `spaceTemplates[]`: `name` (unique), `match.dir` and/or `match.remote`, `agent`, optional `model` and `layout`.
/// Invalid entries warn and are skipped. The agent is checked against `harnesses` at use.
pub fn parse_space_templates(
    value: &Value,
    warnings: &mut Vec<String>,
) -> Vec<crate::templates::Template> {
    let Some(list) = value.as_array() else {
        warnings.push("spaceTemplates must be an array".into());
        return Vec::new();
    };
    let mut out: Vec<crate::templates::Template> = Vec::new();
    for (i, entry) in list.iter().enumerate() {
        let at = format!("spaceTemplates[{i}]");
        let Some(obj) = entry.as_object() else {
            warnings.push(format!("{at} must be an object; ignored"));
            continue;
        };
        let text = |v: Option<&Value>| match v {
            Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_owned()),
            _ => None,
        };
        let Some(name) = text(obj.get("name")) else {
            warnings.push(format!("{at}.name must be a non-empty string; ignored"));
            continue;
        };
        let at = format!("spaceTemplates[{name}]");
        if out.iter().any(|t| t.name == name) {
            warnings.push(format!("{at}: duplicate name; the first one wins"));
            continue;
        }
        let m = obj.get("match").and_then(Value::as_object);
        let dir = match m.and_then(|m| m.get("dir")) {
            None => None,
            Some(v) => match text(Some(v)) {
                Some(d) if crate::templates::is_valid_dir(&d) => Some(d),
                Some(_) => {
                    warnings.push(format!(
                        "{at}.match.dir must be absolute or start with ~; ignored"
                    ));
                    continue;
                }
                None => {
                    warnings.push(format!(
                        "{at}.match.dir must be a non-empty string; ignored"
                    ));
                    continue;
                }
            },
        };
        let remote = match m.and_then(|m| m.get("remote")) {
            None => None,
            Some(v) => match parse_remote_match(v) {
                Ok(r) => Some(r),
                Err(e) => {
                    warnings.push(format!("{at}.match.remote: {e}; ignored"));
                    continue;
                }
            },
        };
        if dir.is_none() && remote.is_none() {
            // No `match`: the default template, used when no other template matches.
            if out.iter().any(|t| t.dir.is_none() && t.remote.is_none()) {
                warnings.push(format!(
                    "{at}: a default template (no match) exists already; ignored"
                ));
                continue;
            }
        }
        let Some(agent) = text(obj.get("agent")) else {
            warnings.push(format!("{at}.agent must be a non-empty string; ignored"));
            continue;
        };
        let model = match obj.get("model") {
            None => None,
            Some(v) => match text(Some(v)) {
                Some(m) => Some(m),
                None => {
                    warnings.push(format!("{at}.model must be a non-empty string; ignored"));
                    continue;
                }
            },
        };
        let layout = match obj.get("layout") {
            None => None,
            Some(l) => match crate::layout::parse_layout_value(l) {
                Ok(l) => Some(l),
                Err(e) => {
                    warnings.push(format!("{at}.layout: {e}; ignored"));
                    continue;
                }
            },
        };
        out.push(crate::templates::Template {
            name,
            dir,
            remote,
            agent,
            model,
            layout,
        });
    }
    out
}

/// `match.remote`: `{ provider: "github", owner }` or
/// `{ provider: "azure-devops", organization, project }`.
fn parse_remote_match(value: &Value) -> Result<crate::remote::Remote, String> {
    use crate::remote::Remote;
    let obj = value.as_object().ok_or("must be an object")?;
    let field = |k: &str| match obj.get(k) {
        Some(Value::String(s)) if !s.trim().is_empty() => Ok(s.trim().to_owned()),
        _ => Err(format!("{k} must be a non-empty string")),
    };
    match obj.get("provider").and_then(Value::as_str) {
        Some("github") => Ok(Remote::GitHub {
            owner: field("owner")?,
        }),
        Some("azure-devops") => Ok(Remote::AzureDevOps {
            organization: field("organization")?,
            project: field("project")?,
        }),
        _ => Err("provider must be \"github\" or \"azure-devops\"".into()),
    }
}

/// A command of a task mode: an argv array (non-empty strings, first one the program) or, still
/// accepted for one release, a string. `Ok(None)` when absent; `Err` (after a warning) when invalid,
/// which skips the mode.
fn spec(
    m: &serde_json::Map<String, Value>,
    key: &str,
    at: &str,
    warnings: &mut Vec<String>,
) -> Result<Option<crate::tasks::CommandSpec>, ()> {
    use crate::tasks::CommandSpec;
    match m.get(key) {
        None => Ok(None),
        Some(Value::String(v)) if !v.trim().is_empty() => Ok(Some(CommandSpec::Shell(v.clone()))),
        Some(Value::Array(items)) => {
            let argv: Option<Vec<String>> = items
                .iter()
                .map(|i| i.as_str().map(str::to_owned))
                .collect();
            match argv {
                Some(a) if a.first().is_some_and(|p| !p.trim().is_empty()) => {
                    Ok(Some(CommandSpec::Argv(a)))
                }
                _ => {
                    warnings.push(format!(
                        "{at}.{key} must be a non-empty array of strings; ignored"
                    ));
                    Err(())
                }
            }
        }
        Some(_) => {
            warnings.push(format!(
                "{at}.{key} must be an array of strings or a non-empty string; ignored"
            ));
            Err(())
        }
    }
}

/// Parses a `tasks` object (`modes[]`, `worktreeDir`) of the user config or of a project file.
/// Invalid entries warn and are skipped. `fallback` ends the warning for a `modes` list without any
/// valid mode ("using the default agent mode", "using your modes").
pub fn parse_tasks(value: &Value, fallback: &str, warnings: &mut Vec<String>) -> TaskOverride {
    use crate::tasks::{
        CLEANUP_PLACEHOLDERS, COMMAND_PLACEHOLDERS, CommandSpec, DEFAULT_BRANCH, PATH_PLACEHOLDERS,
        PROMPT_PLACEHOLDERS, TaskMode, unknown_placeholder,
    };
    let mut result = TaskOverride::default();
    let Some(obj) = value.as_object() else {
        warnings.push("tasks must be an object".into());
        return result;
    };
    match obj.get("worktreeDir") {
        None => {}
        Some(Value::String(d)) if !d.trim().is_empty() => {
            match unknown_placeholder(d, PATH_PLACEHOLDERS) {
                Some(p) => warnings.push(format!(
                    "tasks.worktreeDir: unknown placeholder {{{p}}}; using the default"
                )),
                None => result.worktree_dir = Some(d.clone()),
            }
        }
        Some(_) => {
            warnings.push("tasks.worktreeDir must be a non-empty string; using the default".into())
        }
    }
    let Some(modes) = obj.get("modes") else {
        return result;
    };
    let Some(modes) = modes.as_array() else {
        warnings.push("tasks.modes must be an array".into());
        return result;
    };
    let mut out: Vec<TaskMode> = Vec::new();
    for (i, m) in modes.iter().enumerate() {
        let at = format!("tasks.modes[{i}]");
        let Some(m) = m.as_object() else {
            warnings.push(format!("{at} must be an object; ignored"));
            continue;
        };
        let text = |key: &str, at: &str, warnings: &mut Vec<String>| match m.get(key) {
            None => None,
            Some(Value::String(v)) if !v.trim().is_empty() => Some(v.clone()),
            Some(_) => {
                warnings.push(format!("{at}.{key} must be a non-empty string; ignored"));
                None
            }
        };
        let Some(name) = text("name", &at, warnings) else {
            if !m.contains_key("name") {
                warnings.push(format!("{at}.name is missing; ignored"));
            }
            continue;
        };
        let at = format!("tasks.modes[{name}]");
        if out.iter().any(|o| o.name == name) {
            warnings.push(format!("{at}: duplicate name; the first one wins"));
            continue;
        }
        let command = match spec(m, "command", &at, warnings) {
            Ok(c) => c,
            Err(()) => continue,
        };
        let harness = text("harness", &at, warnings);
        let model = text("model", &at, warnings);
        let prompt = text("prompt", &at, warnings);
        if command.is_some() && (harness.is_some() || model.is_some() || prompt.is_some()) {
            warnings.push(format!(
                "{at}: command cannot be combined with harness, model or prompt; ignored"
            ));
            continue;
        }
        let worktree = match m.get("worktree") {
            None => true,
            Some(Value::Bool(b)) => *b,
            Some(_) => {
                warnings.push(format!("{at}.worktree must be a boolean; using true"));
                true
            }
        };
        let branch = text("branch", &at, warnings).unwrap_or_else(|| DEFAULT_BRANCH.into());
        let cleanup = spec(m, "cleanup", &at, warnings).unwrap_or(None);
        let templates = |spec: &Option<CommandSpec>| -> Vec<String> {
            match spec {
                None => Vec::new(),
                Some(CommandSpec::Shell(line)) => vec![line.clone()],
                Some(CommandSpec::Argv(argv)) => argv.clone(),
            }
        };
        let checks = [
            ("command", templates(&command), COMMAND_PLACEHOLDERS),
            ("cleanup", templates(&cleanup), CLEANUP_PLACEHOLDERS),
            (
                "prompt",
                prompt.iter().cloned().collect(),
                PROMPT_PLACEHOLDERS,
            ),
            ("branch", vec![branch.clone()], PATH_PLACEHOLDERS),
        ];
        let bad = checks.iter().find_map(|(k, items, allowed)| {
            items
                .iter()
                .find_map(|v| unknown_placeholder(v, allowed))
                .map(|p| (*k, p))
        });
        if let Some((key, p)) = bad {
            warnings.push(format!("{at}.{key}: unknown placeholder {{{p}}}; ignored"));
            continue;
        }
        for (key, value) in [("command", &command), ("cleanup", &cleanup)] {
            if matches!(value, Some(CommandSpec::Shell(_))) {
                warnings.push(format!(
                    "tasks.modes.{name}: string {key} is deprecated, use {}",
                    if key == "command" {
                        "harness + prompt or an argv list"
                    } else {
                        "an argv list"
                    }
                ));
            }
        }
        out.push(TaskMode {
            name,
            worktree,
            harness,
            model,
            prompt,
            command,
            branch,
            cleanup,
        });
    }
    if !out.is_empty() {
        result.modes = Some(out);
    } else if !modes.is_empty() {
        warnings.push(format!("tasks.modes has no valid mode; {fallback}"));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rc_channel_has_its_own_config_dir() {
        let home = Path::new("/home/jo");
        assert_eq!(
            config_dir_for(home, poimen_protocol::Channel::Stable),
            PathBuf::from("/home/jo/.config/poimen")
        );
        assert_eq!(
            config_dir_for(home, poimen_protocol::Channel::Rc),
            PathBuf::from("/home/jo/.config/poimen-rc")
        );
        assert_eq!(config_path(home), config_dir(home).join("config.json"));
    }

    #[test]
    fn set_trusted_projects_keeps_comments_and_other_keys() {
        let text = "{\n  // keep\n  \"theme\": \"dark\"\n}";
        let map = BTreeMap::from([("C:\\work\\app".to_owned(), "h1".to_owned())]);
        let out = set_trusted_projects_text(text, &map).unwrap();
        assert!(out.contains("// keep") && out.contains("\"theme\": \"dark\""));
        let parsed: Value = serde_json::from_str(&out.replace("  // keep\n", "")).unwrap();
        assert_eq!(parsed["trustedProjects"]["C:\\work\\app"], "h1");
        // Replacing an existing entry set keeps the rest too.
        let again = set_trusted_projects_text(&out, &BTreeMap::new()).unwrap();
        assert!(again.contains("// keep") && again.contains("\"trustedProjects\": {}"));
    }

    #[test]
    fn trusted_projects_skip_bad_entries_with_warnings() {
        let mut warnings = Vec::new();
        let value: Value = serde_json::from_str(r#"{"/a":"abc","/b":3,"/c":""}"#).unwrap();
        let map = parse_trusted_projects(&value, &mut warnings);
        assert_eq!(map.keys().collect::<Vec<_>>(), ["/a"]);
        assert_eq!(warnings.len(), 2);
        let none = parse_trusted_projects(&serde_json::json!([1]), &mut warnings);
        assert!(none.is_empty());
        assert_eq!(warnings.len(), 3);
    }

    #[test]
    fn trust_and_revoke_write_only_trusted_projects() {
        let home = std::env::temp_dir().join(format!("poimen-core-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        assert!(trusted_projects(&home).is_empty());
        revoke_project(&home, "/a").unwrap();
        assert!(!config_path(&home).exists());
        trust_project(&home, "/a", "h1").unwrap();
        trust_project(&home, "/a", "h2").unwrap();
        assert_eq!(trusted_projects(&home)["/a"], "h2");
        let text = std::fs::read_to_string(config_path(&home)).unwrap();
        assert!(text.contains("\"trustedProjects\""));
        revoke_project(&home, "/a").unwrap();
        assert!(trusted_projects(&home).is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    fn temp_home(name: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("poimen-core-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(config_dir(&home)).unwrap();
        home
    }

    #[test]
    fn trust_keeps_comments_other_keys_and_entries_in_a_commented_file() {
        let home = temp_home("trust-comments");
        let text = "{\n  // my settings\n  \"theme\": \"dark\", /* inline */\n  \"trustedProjects\": {\"/old\": \"h0\"} // trailing\n}\n";
        std::fs::write(config_path(&home), text).unwrap();
        trust_project(&home, "/new", "h1").unwrap();
        let out = std::fs::read_to_string(config_path(&home)).unwrap();
        for kept in [
            "// my settings",
            "\"theme\": \"dark\"",
            "/* inline */",
            "// trailing",
        ] {
            assert!(out.contains(kept), "{kept} lost: {out}");
        }
        let map = trusted_projects_text(&out).unwrap();
        assert_eq!(map.get("/old").map(String::as_str), Some("h0"));
        assert_eq!(map.get("/new").map(String::as_str), Some("h1"));
        revoke_project(&home, "/old").unwrap();
        let out = std::fs::read_to_string(config_path(&home)).unwrap();
        assert!(out.contains("// my settings") && out.contains("// trailing"));
        assert_eq!(trusted_projects_text(&out).unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_trusted_projects_value_that_cannot_be_read_is_never_overwritten() {
        let home = temp_home("trust-broken");
        let text = "{\"trustedProjects\": {\"/a\": \"h\", // note\n}}";
        std::fs::write(config_path(&home), text).unwrap();
        let err = trust_project(&home, "/b", "h2").unwrap_err();
        assert!(err.contains("not changed"), "{err}");
        assert_eq!(std::fs::read_to_string(config_path(&home)).unwrap(), text);
        std::fs::write(config_path(&home), "{\"trustedProjects\": [1]}").unwrap();
        assert!(trust_project(&home, "/b", "h2").is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn two_writers_in_turn_keep_both_changes() {
        // The app writes its keys (`theme`, `sidebar.layout`) and the server `trustedProjects`,
        // each reading the file right before the write.
        let home = temp_home("two-writers");
        std::fs::write(
            config_path(&home),
            "{\n  // mine\n  \"theme\": \"light\"\n}\n",
        )
        .unwrap();
        trust_project(&home, "/a", "h1").unwrap();
        save_with(&home, |t| set_key_text(t, "theme", "\"dark\"")).unwrap();
        trust_project(&home, "/b", "h2").unwrap();
        save_with(&home, |t| {
            set_path_text(t, &["sidebar", "layout"], "\"split\"")
        })
        .unwrap();
        let out = std::fs::read_to_string(config_path(&home)).unwrap();
        assert!(out.contains("// mine"), "{out}");
        let parsed: Value = serde_json::from_str(&out.replace("  // mine\n", "")).unwrap();
        assert_eq!(parsed["theme"], "dark");
        assert_eq!(parsed["sidebar"]["layout"], "split");
        assert_eq!(parsed["trustedProjects"]["/a"], "h1");
        assert_eq!(parsed["trustedProjects"]["/b"], "h2");
        let leftovers: Vec<_> = std::fs::read_dir(config_dir(&home))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn tasks_and_templates_parsers_warn_and_skip_invalid_entries() {
        let mut warnings = Vec::new();
        let tasks = parse_tasks(
            &serde_json::json!({"worktreeDir": "../{repo}-{nr}", "modes": [{"name": "a"}, {"name": "a"}, 3]}),
            "using the default agent mode",
            &mut warnings,
        );
        assert_eq!(tasks.worktree_dir.as_deref(), Some("../{repo}-{nr}"));
        assert_eq!(tasks.modes.unwrap().len(), 1);
        assert_eq!(warnings.len(), 2);
        let mut warnings = Vec::new();
        let templates = parse_space_templates(
            &serde_json::json!([{"name": "t", "agent": "claude"}, {"agent": "x"}]),
            &mut warnings,
        );
        assert_eq!(templates.len(), 1);
        assert_eq!(warnings.len(), 1);
    }
}
