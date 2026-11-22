//! The parts of the user settings (`~/.config/poimen/config.json`) the project-side modules need:
//! the `tasks` and `spaceTemplates` parsers, trusted projects, and the in-place key editor.

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
    "started_by",
    "memory",
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

/// How the spaces tree is ordered (`sidebar.sort`): the user's own order, alphabetical, by the
/// most urgent agent or by the last focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarSort {
    #[default]
    Manual,
    Name,
    Attention,
    Recent,
}

impl SidebarSort {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Name => "name",
            Self::Attention => "attention",
            Self::Recent => "recent",
        }
    }

    /// `manual`, `name`, `attention` or `recent`; anything else is `None`.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "manual" => Some(Self::Manual),
            "name" => Some(Self::Name),
            "attention" => Some(Self::Attention),
            "recent" => Some(Self::Recent),
            _ => None,
        }
    }
}

/// `sidebar.sort` of the `sidebar` object; absent is `manual`, an unknown value warns and is `manual`.
pub fn resolve_sidebar_sort(sidebar: &Value, warnings: &mut Vec<String>) -> SidebarSort {
    match sidebar.get("sort") {
        None => SidebarSort::Manual,
        Some(Value::String(s)) if SidebarSort::parse(s).is_some() => {
            SidebarSort::parse(s).unwrap_or_default()
        }
        Some(other) => {
            warnings.push(format!(
                "sidebar.sort {other} must be \"manual\", \"name\", \"attention\" or \"recent\"; using manual"
            ));
            SidebarSort::Manual
        }
    }
}

/// `sidebar.groupWorktrees` of the `sidebar` object; absent is `true`, a non-boolean warns and is `true`.
pub fn resolve_group_worktrees(sidebar: &Value, warnings: &mut Vec<String>) -> bool {
    match sidebar.get("groupWorktrees") {
        None => true,
        Some(Value::Bool(b)) => *b,
        Some(other) => {
            warnings.push(format!(
                "sidebar.groupWorktrees {other} must be true or false; using true"
            ));
            true
        }
    }
}

/// Default of `agents.silentAfter` in minutes (#898).
pub const DEFAULT_SILENT_AFTER_MIN: u64 = 10;

/// `agents.silentAfter` of the `agents` object, in minutes: a whole number, `0` is off. Absent is
/// the default; anything else warns and is the default.
pub fn resolve_silent_after(agents: &Value, warnings: &mut Vec<String>) -> u64 {
    match agents.get("silentAfter") {
        None => DEFAULT_SILENT_AFTER_MIN,
        Some(v) => v.as_u64().unwrap_or_else(|| {
            warnings.push(format!(
                "agents.silentAfter {v} must be a whole number of minutes (0 is off); using {DEFAULT_SILENT_AFTER_MIN}"
            ));
            DEFAULT_SILENT_AFTER_MIN
        }),
    }
}

/// `agents.silentAfter` in minutes from the config `text`; an unreadable text is the default.
pub fn silent_after_text(text: &str) -> u64 {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|root| root.get("agents").cloned())
        .filter(Value::is_object)
        .map_or(DEFAULT_SILENT_AFTER_MIN, |a| {
            resolve_silent_after(&a, &mut Vec::new())
        })
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
        let rest = &text[after + 1..];
        let gap = line_break(text, from, key_start, rest);
        return Ok(format!("{}{gap}{rest}", &text[..from]));
    }
    // The last entry: drop the comma before it, if any, and keep the comments between that comma
    // and the entry (like the comments above a middle entry, which stay too).
    let before = trim_ws_back(b, key_start);
    let rest = &text[stop..];
    let gap = line_break(text, before, key_start, rest);
    match separator_before(b, open, key_start) {
        Some(comma) => Ok(format!(
            "{}{}{gap}{rest}",
            &text[..comma],
            &text[comma + 1..before]
        )),
        None => Ok(format!("{}{gap}{rest}", &text[..before])),
    }
}

/// The whitespace `text[end..key_start]` when the kept text ends in a `//` comment at `end` and
/// `rest` does not start on a new line (the comment would swallow it), else nothing.
fn line_break<'a>(text: &'a str, end: usize, key_start: usize, rest: &str) -> &'a str {
    let rest_breaks = rest
        .trim_start_matches([' ', '\t'])
        .starts_with(['\n', '\r']);
    if !rest_breaks && line_comment_ends_at(text.as_bytes(), end) {
        &text[end..key_start]
    } else {
        ""
    }
}

/// Whether a `//` comment (outside a string) runs up to `end` on its line. A `//` inside a block
/// comment counts too, which only keeps a line break that was not needed.
fn line_comment_ends_at(b: &[u8], end: usize) -> bool {
    let mut i = b[..end]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |n| n + 1);
    while i < end {
        match b[i] {
            b'"' => i = skip_string(b, i),
            b'/' if b.get(i + 1) == Some(&b'/') => return true,
            _ => i += 1,
        }
    }
    false
}

/// The comma that separates the entry whose key starts at `key_start` from the entry before it,
/// in the object opening at `open`; `None` for the first entry. Comments between them are skipped.
fn separator_before(b: &[u8], open: usize, key_start: usize) -> Option<usize> {
    let (mut i, mut comma) = (open + 1, None);
    loop {
        i = skip_trivia(b, i);
        if i >= key_start {
            return comma;
        }
        match b[i] {
            b',' => {
                comma = Some(i);
                i += 1;
            }
            b':' => i = value_end(b, skip_trivia(b, i + 1)),
            b'"' => i = skip_string(b, i),
            _ => i += 1,
        }
    }
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
        Ok(text) => {
            let next = edit(&text)?;
            if next == text {
                return Ok(());
            }
            next
        }
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

/// `agents.hibernateAfter` (ADR 0014, #903): minutes a `done` agent waits before the server
/// hibernates it. Absent, `null` and `0` are off (`Ok(None)`); anything but a non-negative
/// whole number is an error with the reason (the setting then counts as off).
pub fn hibernate_after_minutes(root: &Value) -> Result<Option<u64>, String> {
    match root.get("agents").and_then(|a| a.get("hibernateAfter")) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => match v.as_u64() {
            Some(0) => Ok(None),
            Some(n) => Ok(Some(n)),
            None => {
                Err("agents.hibernateAfter must be a whole number of minutes (0 is off)".into())
            }
        },
    }
}

/// Parses `layouts[]` (ADR 0011): `{name, tabs}` with a unique `name`; invalid entries warn and
/// are skipped, so the rest loads. `panes[]` is accepted in place of `tabs[]`.
pub fn parse_layouts(value: &Value, warnings: &mut Vec<String>) -> Vec<crate::layout::NamedLayout> {
    let Some(list) = value.as_array() else {
        warnings.push("layouts must be an array".into());
        return Vec::new();
    };
    let mut out: Vec<crate::layout::NamedLayout> = Vec::new();
    for (i, entry) in list.iter().enumerate() {
        let name = entry["name"].as_str().map(str::trim).unwrap_or_default();
        if name.is_empty() {
            warnings.push(format!(
                "layouts[{i}].name must be a non-empty string; ignored"
            ));
            continue;
        }
        let at = format!("layouts[{name}]");
        if out.iter().any(|l| l.name == name) {
            warnings.push(format!("{at}: duplicate name; the first one wins"));
            continue;
        }
        match crate::layout::parse_tabs_value(entry) {
            Ok((layout, w)) => {
                warnings.extend(w.into_iter().map(|w| format!("{at}: {w}")));
                out.push(crate::layout::NamedLayout {
                    name: name.to_owned(),
                    layout,
                });
            }
            Err(e) => warnings.push(format!("{at}: {e}; ignored")),
        }
    }
    out
}

/// The value of the top-level `key` of the config `text`, read from that value alone; `None` when
/// the key is missing, an error when it is not valid JSON.
fn top_value(text: &str, key: &str) -> Result<Option<Value>, String> {
    let Some((start, _)) = find_value(text, root_open(text)?, key) else {
        return Ok(None);
    };
    serde_json::Deserializer::from_str(&text[start..])
        .into_iter::<Value>()
        .next()
        .and_then(Result::ok)
        .map(Some)
        .ok_or(format!(
            "{key} in config.json is not valid JSON; it was not changed"
        ))
}

/// The names of the templates of `templates` (`spaceTemplates`) that refer to the layout `name`.
pub fn layout_users(templates: &Value, name: &str) -> Vec<String> {
    templates
        .as_array()
        .map(|list| {
            list.iter()
                .filter(|t| t["layout"].as_str().map(str::trim) == Some(name))
                .filter_map(|t| t["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// Returns `text` with `layouts` replaced by `layouts` (a list of `{name, tabs}`) after validating
/// it like the loader does (any warning rejects the write). A layout of the old `layouts[]` that
/// is gone from the new list (deleted or renamed) while a template of `spaceTemplates` refers to
/// it is refused, naming the templates. Other keys and comments stay; an unchanged list leaves
/// the text as it is.
pub fn set_layouts_text(text: &str, layouts: &Value) -> Result<String, String> {
    let mut warnings = Vec::new();
    let parsed = parse_layouts(layouts, &mut warnings);
    if warnings.is_empty() && parsed.len() != layouts.as_array().map_or(0, Vec::len) {
        warnings.push("a layout was not accepted".into());
    }
    if !warnings.is_empty() {
        return Err(format!("layouts: {}", warnings.join("; ")));
    }
    let old = top_value(text, "layouts")?;
    if old.as_ref() == Some(layouts) {
        return Ok(text.to_owned());
    }
    if let Some(old) = &old {
        // Entries the loader skips would be lost by the rewrite, so the file is left to the user.
        let mut skipped = Vec::new();
        let kept = parse_layouts(old, &mut skipped).len();
        if !skipped.is_empty() || kept != old.as_array().map_or(0, Vec::len) {
            return Err(format!(
                "layouts in config.json has entries poimen cannot read ({}); it was not changed, fix the file by hand",
                skipped.join("; ")
            ));
        }
    }
    if let Some(templates) = top_value(text, "spaceTemplates")? {
        let mut ignored = Vec::new();
        let old_names: Vec<String> = old
            .as_ref()
            .map(|o| parse_layouts(o, &mut ignored))
            .unwrap_or_default()
            .into_iter()
            .map(|l| l.name)
            .collect();
        for name in old_names
            .iter()
            .filter(|n| !parsed.iter().any(|l| &l.name == *n))
        {
            let users = layout_users(&templates, name);
            if !users.is_empty() {
                return Err(format!(
                    "layout \"{name}\" is used by the templates {}; change them first",
                    users.join(", ")
                ));
            }
        }
    }
    set_key_text(text, "layouts", &layouts.to_string())
}

/// Writes `layouts` below `home`, like `save_theme`.
pub fn save_layouts(home: &Path, layouts: &Value) -> Result<(), String> {
    save_with(home, |text| set_layouts_text(text, layouts))
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
        let (layout, layout_name) = match obj.get("layout") {
            None => (None, None),
            Some(Value::String(n)) if !n.trim().is_empty() => (None, Some(n.trim().to_owned())),
            Some(Value::String(_)) => {
                warnings.push(format!("{at}.layout: a name must not be empty; ignored"));
                continue;
            }
            Some(l) => match crate::layout::parse_layout_value(l) {
                Ok(l) => (Some(l), None),
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
            layout_name,
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
#[path = "settings_tests.rs"]
mod tests;
