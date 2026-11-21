//! Export and import of the user settings (#581): `config.json` as a JSON file, as a whole or a
//! single space template.
//!
//! Settings are preferences only: UI state never is in `config.json`, so it is never exported.
//! `trustedProjects` is per machine: it is left out of an export, skipped on import and never
//! written. An import is validated with the settings loader (`settings::resolve`), entry by entry
//! in the settings the import gives (an entry is invalid only when it adds a warning there),
//! so an invalid entry is listed and skipped while the rest is kept. The plan lists the keys that
//! the import adds, changes and removes; applying it writes only those keys through
//! `poimen_core::settings::save_with`, so comments and formatting elsewhere stay.

use std::cell::RefCell;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use poimen_core::settings::{
    config_path, remove_path_text, save_with, set_path_text, skip_string, skip_trivia,
};

/// Keys that belong to one machine: never exported, imported or written by an import.
pub const PER_MACHINE: &[&str] = &["trustedProjects"];

/// The largest file an import reads.
const MAX_IMPORT_BYTES: u64 = 1024 * 1024;

/// What an import does with the keys the file does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Keeps every key the file does not set; objects merge key by key, space templates by name.
    Merge,
    /// The file becomes the settings: keys it does not have are removed (`trustedProjects` stays).
    Replace,
}

/// What an import reads of the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// The whole settings file.
    All,
    /// Only `spaceTemplates` (or a bare template object), always merged by name.
    Template,
}

/// One key the import adds, changes or removes; `path` is dotted, a template is
/// `spaceTemplates[<name>]`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Change {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<Value>,
    /// The key path that is written for this change.
    #[serde(skip)]
    write: Vec<String>,
}

/// An entry of the file that is not imported, with the reason.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rejected {
    pub path: String,
    pub reason: String,
}

/// What an import would do (or did): the diff against the current settings and the entries left
/// out.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Plan {
    pub added: Vec<Change>,
    pub changed: Vec<Change>,
    pub removed: Vec<Change>,
    /// Entries the settings loader rejects; never written.
    pub invalid: Vec<Rejected>,
    /// Entries left out on purpose (per-machine keys, keys outside a template import).
    pub skipped: Vec<Rejected>,
}

/// `text` without `//` and `/* */` comments (strings kept), so a commented config parses.
fn without_comments(text: &str) -> String {
    let b = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut i, mut last) = (0, 0);
    while i < b.len() {
        match b[i] {
            b'"' => i = skip_string(b, i),
            b'/' if matches!(b.get(i + 1), Some(b'/' | b'*')) => {
                out.push_str(&text[last..i]);
                out.push(' ');
                i = skip_trivia(b, i);
                last = i;
            }
            _ => i += 1,
        }
    }
    out.push_str(&text[last..]);
    out
}

/// The top-level object of a config text (comments allowed).
pub fn parse_object(text: &str) -> Result<Map<String, Value>, String> {
    match serde_json::from_str(&without_comments(text)) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err("the file is not a JSON object".into()),
        Err(e) => Err(format!("the file is not valid JSON: {e}")),
    }
}

fn pretty(value: &Value) -> String {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string());
    text.push('\n');
    text
}

/// The export of the config `text` (`None`: no config file): every key but the per-machine ones.
pub fn export_text(text: Option<&str>) -> Result<String, String> {
    let mut map = match text {
        Some(text) => parse_object(text).map_err(|e| format!("config.json: {e}"))?,
        None => Map::new(),
    };
    for key in PER_MACHINE {
        map.remove(*key);
    }
    Ok(pretty(&Value::Object(map)))
}

fn template_name(value: &Value) -> Option<&str> {
    value.get("name").and_then(Value::as_str).map(str::trim)
}

/// The export of the space template `name` of the config `text`: `{"spaceTemplates": [<it>]}`,
/// layout included.
pub fn export_template_text(text: Option<&str>, name: &str) -> Result<String, String> {
    let map = match text {
        Some(text) => parse_object(text).map_err(|e| format!("config.json: {e}"))?,
        None => Map::new(),
    };
    let template = map
        .get("spaceTemplates")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find(|t| template_name(t) == Some(name.trim())))
        .ok_or_else(|| format!("no space template named {name:?}"))?;
    Ok(pretty(&serde_json::json!({ "spaceTemplates": [template] })))
}

/// The warnings of the settings loader for `config` alone.
fn warnings_of(config: Value) -> Vec<String> {
    crate::settings::resolve(Some(&config.to_string()), || Some(String::new())).warnings
}

/// `ctx` with the value at `path` set (`Some`) or removed (`None`); missing or non-object parents
/// become objects.
fn with_at(ctx: &Map<String, Value>, path: &[&str], value: Option<Value>) -> Map<String, Value> {
    let mut out = ctx.clone();
    let Some((first, rest)) = path.split_first() else {
        return out;
    };
    if rest.is_empty() {
        match value {
            Some(v) => out.insert((*first).to_owned(), v),
            None => out.remove(*first),
        };
    } else {
        let inner = match ctx.get(*first) {
            Some(Value::Object(m)) => m.clone(),
            _ => Map::new(),
        };
        out.insert(
            (*first).to_owned(),
            Value::Object(with_at(&inner, rest, value)),
        );
    }
    out
}

/// The settings an entry of an import is checked in: what the import gives (the file over the
/// current settings, or the file alone on a replace), and on a merge the current settings an
/// entry is merged into.
struct Ctx {
    settings: Map<String, Value>,
    base: Option<Map<String, Value>>,
}

impl Ctx {
    /// The value at `path` once `value` (`None`: the file has nothing there) is imported.
    fn imported(&self, path: &[&str], value: Option<Value>) -> Option<Value> {
        let Some(base) = &self.base else {
            return value;
        };
        let path_owned: Vec<String> = path.iter().map(|k| (*k).to_owned()).collect();
        match value {
            None => value_at(base, &path_owned).cloned(),
            Some(v) => {
                let Value::Object(over) = wrap(path, v) else {
                    return None;
                };
                let merged = deep_merge(base, &over, true, &mut Vec::new());
                value_at(&merged, &path_owned).cloned()
            }
        }
    }
}

/// The warnings that `candidate` at `path` adds to the settings of `ctx`, compared as sets with
/// `baseline` there (`None`: the file has nothing there). Checking in context keeps entries that
/// depend on another key (an agent action using a harness of `harnesses`).
fn new_warnings(
    ctx: &Ctx,
    path: &[&str],
    candidate: Value,
    baseline: Option<Value>,
) -> Vec<String> {
    let at = |value: Option<Value>| {
        let value = ctx.imported(path, value);
        warnings_of(Value::Object(with_at(&ctx.settings, path, value)))
    };
    let before: std::collections::HashSet<String> = at(baseline).into_iter().collect();
    at(Some(candidate))
        .into_iter()
        .filter(|w| !before.contains(w))
        .collect()
}

fn wrap(path: &[&str], value: Value) -> Value {
    path.iter().rev().fold(value, |inner, key| {
        let mut map = Map::new();
        map.insert((*key).to_owned(), inner);
        Value::Object(map)
    })
}

/// A key the in-place editor can write: no quote, backslash or control character.
fn plain_key(key: &str) -> bool {
    !key.is_empty() && !key.chars().any(|c| c == '"' || c == '\\' || c.is_control())
}

/// The items of the array at `path` that the loader accepts in `ctx`, one after the other (a
/// duplicate of an accepted name is rejected like the loader does).
fn accept_items(
    ctx: &Ctx,
    path: &[&str],
    items: &[Value],
    invalid: &mut Vec<Rejected>,
) -> Vec<Value> {
    let mut kept: Vec<Value> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let mut trial = kept.clone();
        trial.push(item.clone());
        let warnings = new_warnings(
            ctx,
            path,
            Value::Array(trial),
            Some(Value::Array(kept.clone())),
        );
        if warnings.is_empty() {
            kept.push(item.clone());
        } else {
            let label = template_name(item)
                .filter(|n| !n.is_empty())
                .map_or_else(|| i.to_string(), str::to_owned);
            invalid.push(Rejected {
                path: format!("{}[{label}]", path.join(".")),
                reason: warnings.join("; "),
            });
        }
    }
    kept
}

/// The value at `path` as far as the loader accepts it in the settings `ctx`: whole, else (arrays)
/// item by item, else (objects up to `depth` levels) key by key. `None` when nothing of it is
/// accepted.
fn accept_value(
    ctx: &Ctx,
    path: &[&str],
    value: &Value,
    depth: usize,
    invalid: &mut Vec<Rejected>,
) -> Option<Value> {
    let warnings = new_warnings(ctx, path, value.clone(), None);
    if warnings.is_empty() {
        return Some(value.clone());
    }
    let reject = |invalid: &mut Vec<Rejected>| {
        invalid.push(Rejected {
            path: path.join("."),
            reason: warnings.join("; "),
        });
        None
    };
    match value {
        Value::Array(items) if !items.is_empty() => {
            let kept = accept_items(ctx, path, items, invalid);
            (!kept.is_empty()).then_some(Value::Array(kept))
        }
        Value::Object(map) if !map.is_empty() && depth > 0 => {
            let mut kept = Map::new();
            for (key, inner) in map {
                let mut sub = path.to_vec();
                sub.push(key);
                if !plain_key(key) {
                    invalid.push(Rejected {
                        path: sub.join("."),
                        reason: "unsupported key name".into(),
                    });
                } else if let Some(v) = accept_value(ctx, &sub, inner, depth - 1, invalid) {
                    kept.insert(key.clone(), v);
                }
            }
            if kept.is_empty() {
                return None;
            }
            // The accepted keys together must be valid too.
            let together = Value::Object(kept);
            let warnings = new_warnings(ctx, path, together.clone(), None);
            if warnings.is_empty() {
                Some(together)
            } else {
                invalid.push(Rejected {
                    path: path.join("."),
                    reason: warnings.join("; "),
                });
                None
            }
        }
        _ => reject(invalid),
    }
}

/// The part of an imported file the import uses: per-machine keys skipped, for a template import
/// only `spaceTemplates`, each entry validated in the settings the import gives (the file over
/// `current`, the file alone on a replace), so an entry is invalid only when it adds a warning.
fn accepted(
    current: &Map<String, Value>,
    file: &Map<String, Value>,
    mode: Mode,
    scope: Scope,
    plan: &mut Plan,
) -> Map<String, Value> {
    let mut source = file.clone();
    if scope == Scope::Template {
        if !source.contains_key("spaceTemplates") && source.contains_key("name") {
            source = Map::from_iter([(
                "spaceTemplates".to_owned(),
                Value::Array(vec![Value::Object(file.clone())]),
            )]);
        }
        for key in source.keys() {
            if key != "spaceTemplates" {
                plan.skipped.push(Rejected {
                    path: key.clone(),
                    reason: "not a space template".into(),
                });
            }
        }
        source.retain(|k, _| k == "spaceTemplates");
    }
    let raw = without_per_machine(&source);
    let ctx = match (mode, scope) {
        (Mode::Replace, Scope::All) => Ctx {
            settings: raw,
            base: None,
        },
        _ => {
            let base = without_per_machine(current);
            Ctx {
                settings: deep_merge(&base, &raw, true, &mut Vec::new()),
                base: Some(base),
            }
        }
    };
    let mut out = Map::new();
    for (key, value) in &source {
        if PER_MACHINE.contains(&key.as_str()) {
            plan.skipped.push(Rejected {
                path: key.clone(),
                reason: "per machine, never imported".into(),
            });
        } else if !plain_key(key) {
            plan.invalid.push(Rejected {
                path: key.clone(),
                reason: "unsupported key name".into(),
            });
        } else if let Some(v) = accept_value(&ctx, &[key], value, 1, &mut plan.invalid) {
            out.insert(key.clone(), v);
        }
    }
    out
}

/// `base` with the templates of `over`: a template of the same name is replaced in place, a new
/// one appended.
fn merge_templates(base: &[Value], over: &[Value]) -> Vec<Value> {
    let mut out = base.to_vec();
    for t in over {
        match out
            .iter_mut()
            .find(|b| template_name(b).is_some() && template_name(b) == template_name(t))
        {
            Some(slot) => *slot = t.clone(),
            None => out.push(t.clone()),
        }
    }
    out
}

/// `base` with `over` merged in: objects key by key, `spaceTemplates` (top level) by name, where an
/// imported template that would make the templates invalid (a second default) is listed and left
/// out.
fn deep_merge(
    base: &Map<String, Value>,
    over: &Map<String, Value>,
    top: bool,
    invalid: &mut Vec<Rejected>,
) -> Map<String, Value> {
    let mut out = base.clone();
    for (key, value) in over {
        let next = match (base.get(key), value) {
            (Some(Value::Object(a)), Value::Object(b)) => {
                Value::Object(deep_merge(a, b, false, invalid))
            }
            (Some(Value::Array(a)), Value::Array(b)) if top && key == "spaceTemplates" => {
                Value::Array(merge_templates_checked(a, b, invalid))
            }
            _ => value.clone(),
        };
        out.insert(key.clone(), next);
    }
    out
}

/// [`merge_templates`] one template at a time, keeping only those that add no warning.
fn merge_templates_checked(
    base: &[Value],
    over: &[Value],
    invalid: &mut Vec<Rejected>,
) -> Vec<Value> {
    let count =
        |list: &[Value]| warnings_of(wrap(&["spaceTemplates"], Value::Array(list.to_vec())));
    let mut out = base.to_vec();
    for (i, t) in over.iter().enumerate() {
        let next = merge_templates(&out, std::slice::from_ref(t));
        let added = count(&next);
        if added.len() <= count(&out).len() {
            out = next;
        } else {
            let label = template_name(t).map_or_else(|| i.to_string(), str::to_owned);
            invalid.push(Rejected {
                path: format!("spaceTemplates[{label}]"),
                reason: added.join("; "),
            });
        }
    }
    out
}

fn without_per_machine(map: &Map<String, Value>) -> Map<String, Value> {
    let mut out = map.clone();
    for key in PER_MACHINE {
        out.remove(*key);
    }
    out
}

fn change(
    path: String,
    write: Vec<String>,
    before: Option<&Value>,
    after: Option<&Value>,
) -> Change {
    Change {
        path,
        before: before.cloned(),
        after: after.cloned(),
        write,
    }
}

/// Adds the template differences (by name) of `spaceTemplates`.
fn diff_templates(before: &[Value], after: &[Value], plan: &mut Plan) {
    let write = vec!["spaceTemplates".to_owned()];
    let label = |t: &Value, i: usize| {
        let name = template_name(t).map_or_else(|| i.to_string(), str::to_owned);
        format!("spaceTemplates[{name}]")
    };
    let find = |list: &[Value], t: &Value| {
        list.iter()
            .find(|o| template_name(o).is_some() && template_name(o) == template_name(t))
            .cloned()
    };
    let start = plan.added.len() + plan.changed.len() + plan.removed.len();
    for (i, t) in before.iter().enumerate() {
        match find(after, t) {
            None => plan
                .removed
                .push(change(label(t, i), write.clone(), Some(t), None)),
            Some(a) if &a != t => {
                plan.changed
                    .push(change(label(t, i), write.clone(), Some(t), Some(&a)));
            }
            Some(_) => {}
        }
    }
    for (i, t) in after.iter().enumerate() {
        if find(before, t).is_none() {
            plan.added
                .push(change(label(t, i), write.clone(), None, Some(t)));
        }
    }
    let end = plan.added.len() + plan.changed.len() + plan.removed.len();
    if start == end && before != after {
        // Only the order (or unnamed entries) differ.
        let (b, a) = (Value::Array(before.to_vec()), Value::Array(after.to_vec()));
        plan.changed
            .push(change("spaceTemplates".into(), write, Some(&b), Some(&a)));
    }
}

fn diff(
    before: &Map<String, Value>,
    after: &Map<String, Value>,
    prefix: &[String],
    plan: &mut Plan,
) {
    let mut keys: Vec<&String> = before.keys().collect();
    keys.extend(after.keys().filter(|k| !before.contains_key(*k)));
    for key in keys {
        let mut path = prefix.to_vec();
        path.push(key.clone());
        let dotted = path.join(".");
        match (before.get(key), after.get(key)) {
            (Some(b), None) => plan.removed.push(change(dotted, path, Some(b), None)),
            (None, Some(a)) => plan.added.push(change(dotted, path, None, Some(a))),
            (Some(Value::Object(b)), Some(Value::Object(a))) => diff(b, a, &path, plan),
            (Some(Value::Array(b)), Some(Value::Array(a)))
                if prefix.is_empty() && key == "spaceTemplates" =>
            {
                diff_templates(b, a, plan);
            }
            (Some(b), Some(a)) if a != b => {
                plan.changed.push(change(dotted, path, Some(b), Some(a)))
            }
            _ => {}
        }
    }
}

/// The plan of importing `file` into the settings `current`, and the settings after it.
pub fn plan(
    current: &Map<String, Value>,
    file: &Map<String, Value>,
    mode: Mode,
    scope: Scope,
) -> (Plan, Map<String, Value>) {
    let mut plan = Plan::default();
    let import = accepted(current, file, mode, scope, &mut plan);
    let before = without_per_machine(current);
    let after = match (mode, scope) {
        (Mode::Replace, Scope::All) => import,
        _ => deep_merge(&before, &import, true, &mut plan.invalid),
    };
    diff(&before, &after, &[], &mut plan);
    (plan, after)
}

/// The value at `path` of `map`.
fn value_at<'a>(map: &'a Map<String, Value>, path: &[String]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    rest.iter()
        .try_fold(map.get(first)?, |v, key| v.as_object()?.get(key))
}

/// `text` with the key paths of `plan` written (`depth` segments at most): each set to its value
/// in `after`, or removed.
fn write_paths(
    text: &str,
    plan: &Plan,
    after: &Map<String, Value>,
    depth: usize,
) -> Result<String, String> {
    let mut written: Vec<&[String]> = Vec::new();
    let mut next = text.to_owned();
    for c in plan.removed.iter().chain(&plan.changed).chain(&plan.added) {
        let write = &c.write[..c.write.len().min(depth)];
        if written.contains(&write) {
            continue;
        }
        written.push(write);
        let path: Vec<&str> = write.iter().map(String::as_str).collect();
        next = match value_at(after, write) {
            Some(value) => set_path_text(&next, &path, &value.to_string())?,
            None => remove_path_text(&next, &path)?,
        };
    }
    Ok(next)
}

/// `text` (whose settings are `current`) with the changes of `plan` written; nothing else of the
/// text changes, per-machine keys included. Where writing a nested key would not give the
/// expected settings (a comment next to a removed entry), its top-level key is written whole.
pub fn apply_text(
    text: &str,
    current: &Map<String, Value>,
    plan: &Plan,
    after: &Map<String, Value>,
) -> Result<String, String> {
    let mut expected = after.clone();
    for key in PER_MACHINE {
        if let Some(v) = current.get(*key) {
            expected.insert((*key).to_owned(), v.clone());
        }
    }
    for depth in [usize::MAX, 1] {
        let next = write_paths(text, plan, after, depth)?;
        if parse_object(&next).is_ok_and(|m| m == expected) {
            return Ok(next);
        }
    }
    Err(
        "config.json cannot be edited without changing keys the import leaves alone; \
         edit the keys of the import in config.json by hand"
            .into(),
    )
}

/// Reads an import file: a regular file up to 1 MiB holding a JSON object (comments allowed).
pub fn read_import(path: &Path) -> Result<Map<String, Value>, String> {
    let meta =
        std::fs::metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", path.display()));
    }
    if meta.len() > MAX_IMPORT_BYTES {
        return Err(format!("{} is larger than 1 MiB", path.display()));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    parse_object(&text)
}

fn read_config(home: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(config_path(home)) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read config.json: {e}")),
    }
}

/// Writes `text` to `path`, which must end in `.json` (the save dialog's filter).
pub fn write_export(path: &Path, text: &str) -> Result<(), String> {
    let json = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if !json {
        return Err("the export file must end in .json".into());
    }
    std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Exports the settings below `home` to `path`; `template` exports only that space template.
pub fn export(home: &Path, path: &Path, template: Option<&str>) -> Result<(), String> {
    let config = read_config(home)?;
    let text = match template {
        Some(name) => export_template_text(config.as_deref(), name)?,
        None => export_text(config.as_deref())?,
    };
    write_export(path, &text)
}

/// The plan of importing `path` into the settings below `home`; writes nothing.
pub fn import_plan(home: &Path, path: &Path, mode: Mode, scope: Scope) -> Result<Plan, String> {
    let file = read_import(path)?;
    let current = match read_config(home)? {
        Some(text) => parse_object(&text).map_err(|e| format!("config.json: {e}"))?,
        None => Map::new(),
    };
    Ok(plan(&current, &file, mode, scope).0)
}

/// Imports `path` into the settings below `home`: the plan is made again against the config read
/// right before the write, and only its keys are written. Returns that plan.
pub fn import_apply(home: &Path, path: &Path, mode: Mode, scope: Scope) -> Result<Plan, String> {
    let file = read_import(path)?;
    let done = RefCell::new(Plan::default());
    save_with(home, |text| {
        let current = parse_object(text).map_err(|e| format!("config.json: {e}"))?;
        let (p, after) = plan(&current, &file, mode, scope);
        let next = apply_text(text, &current, &p, &after)?;
        *done.borrow_mut() = p;
        Ok(next)
    })?;
    Ok(done.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn obj(v: Value) -> Map<String, Value> {
        match v {
            Value::Object(m) => m,
            _ => panic!("not an object"),
        }
    }

    fn paths(list: &[Change]) -> Vec<&str> {
        list.iter().map(|c| c.path.as_str()).collect()
    }

    fn rejected(list: &[Rejected]) -> Vec<&str> {
        list.iter().map(|r| r.path.as_str()).collect()
    }

    fn temp_home(name: &str) -> std::path::PathBuf {
        let home =
            std::env::temp_dir().join(format!("poimen-transfer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
        home
    }

    const TEMPLATE_A: &str = r#"{ "name": "a", "match": { "dir": "~/a" }, "agent": "claude" }"#;

    #[test]
    fn export_leaves_out_trusted_projects_and_keeps_the_rest() {
        let text = r#"{
  // my theme
  "theme": "dark",
  "trustedProjects": { "/repo": "abc" },
  "sidebar": { "layout": "split" }
}"#;
        let out: Value = serde_json::from_str(&export_text(Some(text)).unwrap()).unwrap();
        assert_eq!(
            out,
            json!({ "theme": "dark", "sidebar": { "layout": "split" } })
        );
    }

    #[test]
    fn export_without_a_config_is_an_empty_object() {
        assert_eq!(export_text(None).unwrap(), "{}\n");
    }

    #[test]
    fn export_of_an_invalid_config_is_an_error() {
        assert!(
            export_text(Some("[1]"))
                .unwrap_err()
                .contains("not a JSON object")
        );
        assert!(
            export_text(Some("{ nope"))
                .unwrap_err()
                .contains("not valid JSON")
        );
    }

    #[test]
    fn export_template_writes_one_template_with_its_layout() {
        let text = r#"{ "spaceTemplates": [
  { "name": "a", "agent": "claude", "layout": { "panes": [ { "name": "x" } ] } },
  { "name": "b", "agent": "copilot" }
] }"#;
        let out: Value =
            serde_json::from_str(&export_template_text(Some(text), "a").unwrap()).unwrap();
        assert_eq!(
            out,
            json!({ "spaceTemplates": [
                { "name": "a", "agent": "claude", "layout": { "panes": [ { "name": "x" } ] } }
            ] })
        );
        assert!(
            export_template_text(Some(text), "c")
                .unwrap_err()
                .contains("no space template named \"c\"")
        );
    }

    #[test]
    fn export_template_keeps_a_layout_name_reference() {
        let text =
            r#"{ "spaceTemplates": [ { "name": "a", "agent": "claude", "layout": "review" } ] }"#;
        let out: Value =
            serde_json::from_str(&export_template_text(Some(text), "a").unwrap()).unwrap();
        assert_eq!(out["spaceTemplates"][0]["layout"], "review");
    }

    #[test]
    fn comments_in_strings_stay() {
        let map = parse_object(
            r#"{ "a": "http://x/*y*/" /* c */, "b": 1 // d
}"#,
        )
        .unwrap();
        assert_eq!(Value::Object(map), json!({ "a": "http://x/*y*/", "b": 1 }));
    }

    #[test]
    fn merge_plan_lists_added_and_changed_keys_and_removes_nothing() {
        let current =
            obj(json!({ "theme": "dark", "sidebar": { "layout": "tree", "agents": "list" } }));
        let file = obj(
            json!({ "theme": "light", "sidebar": { "layout": "split" }, "ui": { "fontSize": 14 } }),
        );
        let (p, after) = plan(&current, &file, Mode::Merge, Scope::All);
        assert_eq!(paths(&p.added), ["ui"]);
        assert_eq!(paths(&p.changed), ["sidebar.layout", "theme"]);
        assert!(p.removed.is_empty());
        assert_eq!(p.changed[1].before, Some(json!("dark")));
        assert_eq!(p.changed[1].after, Some(json!("light")));
        assert_eq!(
            after["sidebar"],
            json!({ "layout": "split", "agents": "list" })
        );
    }

    #[test]
    fn replace_plan_removes_keys_the_file_does_not_have() {
        let current =
            obj(json!({ "theme": "dark", "sidebar": { "sort": "name", "agents": "list" } }));
        let file = obj(json!({ "sidebar": { "agents": "list" } }));
        let (p, _) = plan(&current, &file, Mode::Replace, Scope::All);
        assert!(p.added.is_empty());
        assert!(p.changed.is_empty());
        assert_eq!(paths(&p.removed), ["sidebar.sort", "theme"]);
    }

    #[test]
    fn identical_file_changes_nothing() {
        let current = obj(json!({ "theme": "dark" }));
        let (p, _) = plan(&current, &current, Mode::Replace, Scope::All);
        assert_eq!(p, Plan::default());
    }

    #[test]
    fn invalid_entries_are_listed_and_skipped() {
        let file = obj(json!({
            "theme": "purple",
            "sidebar": { "sort": "zigzag", "agents": "priority" },
            "spaceTemplates": [
                serde_json::from_str::<Value>(TEMPLATE_A).unwrap(),
                { "agent": "claude" },
                { "name": "a", "agent": "copilot" }
            ]
        }));
        let (p, after) = plan(&Map::new(), &file, Mode::Merge, Scope::All);
        assert_eq!(
            rejected(&p.invalid),
            [
                "sidebar.sort",
                "spaceTemplates[1]",
                "spaceTemplates[a]",
                "theme"
            ]
        );
        assert!(p.invalid[0].reason.contains("sidebar.sort"));
        assert_eq!(paths(&p.added), ["sidebar", "spaceTemplates"]);
        assert_eq!(after["sidebar"], json!({ "agents": "priority" }));
        assert_eq!(after["spaceTemplates"].as_array().unwrap().len(), 1);
        assert!(!after.contains_key("theme"));
    }

    fn action(id: &str, harness: &str) -> Value {
        json!({ "id": id, "on": "issue", "prompt": "do {nr}", "harness": harness })
    }

    #[test]
    fn an_action_with_a_harness_of_the_file_is_imported() {
        let file = obj(json!({
            "harnesses": { "x": { "command": ["x"] } },
            "github": { "agentActions": [action("a", "x")] }
        }));
        for mode in [Mode::Merge, Mode::Replace] {
            let (p, after) = plan(&Map::new(), &file, mode, Scope::All);
            assert!(p.invalid.is_empty(), "{mode:?}: {:?}", p.invalid);
            assert_eq!(after, file, "{mode:?}");
        }
    }

    #[test]
    fn an_action_with_a_harness_of_the_current_config_is_merged() {
        let current = obj(json!({ "harnesses": { "x": { "command": ["x"] } } }));
        let file = obj(json!({
            "harnesses": { "y": { "command": ["y"] } },
            "github": { "agentActions": [action("a", "x")] }
        }));
        let (p, after) = plan(&current, &file, Mode::Merge, Scope::All);
        assert!(p.invalid.is_empty(), "{:?}", p.invalid);
        assert_eq!(
            after["harnesses"],
            json!({ "x": { "command": ["x"] }, "y": { "command": ["y"] } })
        );
        assert_eq!(
            after["github"],
            json!({ "agentActions": [action("a", "x")] })
        );
    }

    #[test]
    fn a_harness_of_the_current_config_does_not_count_on_replace() {
        let current = obj(json!({ "harnesses": { "x": { "command": ["x"] } } }));
        let file = obj(json!({ "github": { "agentActions": [action("a", "x")] } }));
        let (p, after) = plan(&current, &file, Mode::Replace, Scope::All);
        assert_eq!(rejected(&p.invalid), ["github.agentActions[0]"]);
        assert!(!after.contains_key("github"));
    }

    #[test]
    fn an_action_with_an_unknown_harness_is_still_invalid() {
        let current = obj(json!({ "harnesses": { "x": { "command": ["x"] } } }));
        let file = obj(json!({
            "harnesses": { "y": { "command": ["y"] } },
            "github": { "agentActions": [action("a", "y"), action("b", "nope")] }
        }));
        let (p, after) = plan(&current, &file, Mode::Merge, Scope::All);
        assert_eq!(rejected(&p.invalid), ["github.agentActions[1]"]);
        assert!(
            p.invalid[0].reason.contains("nope"),
            "{}",
            p.invalid[0].reason
        );
        assert_eq!(
            after["github"],
            json!({ "agentActions": [action("a", "y")] })
        );
    }

    #[test]
    fn an_empty_invalid_value_is_listed() {
        let (p, after) = plan(
            &Map::new(),
            &obj(json!({ "sidebar": [] })),
            Mode::Merge,
            Scope::All,
        );
        assert_eq!(rejected(&p.invalid), ["sidebar"]);
        assert!(after.is_empty());
    }

    #[test]
    fn unknown_keys_are_kept() {
        let file = obj(json!({ "console": { "prefix": "C-a" } }));
        let (p, _) = plan(&Map::new(), &file, Mode::Merge, Scope::All);
        assert_eq!(paths(&p.added), ["console"]);
        assert!(p.invalid.is_empty());
    }

    #[test]
    fn trusted_projects_are_skipped_and_kept_on_replace() {
        let current = obj(json!({ "theme": "dark", "trustedProjects": { "/mine": "1" } }));
        let file = obj(json!({ "trustedProjects": { "/theirs": "2" } }));
        let (p, after) = plan(&current, &file, Mode::Replace, Scope::All);
        assert_eq!(rejected(&p.skipped), ["trustedProjects"]);
        assert_eq!(paths(&p.removed), ["theme"]);
        assert!(p.added.is_empty() && p.changed.is_empty());
        assert!(!after.contains_key("trustedProjects"));
    }

    #[test]
    fn template_import_merges_by_name_and_skips_other_keys() {
        let current = obj(json!({ "spaceTemplates": [
            { "name": "a", "match": { "dir": "~/a" }, "agent": "claude" },
            { "name": "b", "match": { "dir": "~/b" }, "agent": "claude" }
        ] }));
        let file = obj(json!({
            "theme": "light",
            "spaceTemplates": [
                { "name": "b", "match": { "dir": "~/b" }, "agent": "copilot" },
                { "name": "c", "match": { "dir": "~/c" }, "agent": "claude" }
            ]
        }));
        let (p, after) = plan(&current, &file, Mode::Replace, Scope::Template);
        assert_eq!(rejected(&p.skipped), ["theme"]);
        assert_eq!(paths(&p.changed), ["spaceTemplates[b]"]);
        assert_eq!(paths(&p.added), ["spaceTemplates[c]"]);
        assert!(p.removed.is_empty());
        let names: Vec<_> = after["spaceTemplates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, ["a", "b", "c"]);
    }

    #[test]
    fn a_bare_template_object_imports_as_a_template() {
        let file = obj(serde_json::from_str(TEMPLATE_A).unwrap());
        let (p, _) = plan(&Map::new(), &file, Mode::Merge, Scope::Template);
        assert_eq!(paths(&p.added), ["spaceTemplates"]);
        assert!(p.skipped.is_empty() && p.invalid.is_empty());
    }

    #[test]
    fn replace_lists_removed_templates_by_name() {
        let current = obj(json!({ "spaceTemplates": [ { "name": "a", "agent": "claude" } ] }));
        let file = obj(json!({ "spaceTemplates": [ { "name": "b", "agent": "claude" } ] }));
        let (p, _) = plan(&current, &file, Mode::Replace, Scope::All);
        assert_eq!(paths(&p.removed), ["spaceTemplates[a]"]);
        assert_eq!(paths(&p.added), ["spaceTemplates[b]"]);
    }

    #[test]
    fn apply_keeps_comments_and_trusted_projects_text() {
        let text = r#"{
  // keep me
  "theme": "dark",
  "trustedProjects": {
    // trust comment
    "/repo": "abc"
  },
  "sidebar": { /* inline */ "layout": "tree", "agents": "list" }
}
"#;
        let file = obj(
            json!({ "theme": "light", "sidebar": { "layout": "split" }, "trustedProjects": {} }),
        );
        let current = parse_object(text).unwrap();
        let (p, after) = plan(&current, &file, Mode::Replace, Scope::All);
        let next = apply_text(text, &current, &p, &after).unwrap();
        assert!(next.contains("// keep me"));
        assert!(
            next.contains(
                "\"trustedProjects\": {\n    // trust comment\n    \"/repo\": \"abc\"\n  }"
            )
        );
        assert!(next.contains("/* inline */"));
        assert_eq!(
            Value::Object(parse_object(&next).unwrap()),
            json!({
                "theme": "light",
                "trustedProjects": { "/repo": "abc" },
                "sidebar": { "layout": "split" }
            })
        );
    }

    #[test]
    fn apply_writes_the_section_whole_where_a_comment_blocks_the_edit() {
        let text = "{ \"sidebar\": { \"layout\": \"tree\", /* c */ \"agents\": \"list\" } }";
        let current = parse_object(text).unwrap();
        let file = obj(json!({ "sidebar": { "layout": "tree" } }));
        let (p, after) = plan(&current, &file, Mode::Replace, Scope::All);
        let next = apply_text(text, &current, &p, &after).unwrap();
        assert_eq!(
            Value::Object(parse_object(&next).unwrap()),
            json!({ "sidebar": { "layout": "tree" } })
        );
    }

    #[test]
    fn replace_removes_a_last_top_level_key_with_a_comment_above_it() {
        let text = "{\n  \"a\": 1,\n  // about b\n  \"b\": 2\n}\n";
        let current = parse_object(text).unwrap();
        let file = obj(json!({ "a": 1 }));
        let (p, after) = plan(&current, &file, Mode::Replace, Scope::All);
        assert_eq!(paths(&p.removed), ["b"]);
        let next = apply_text(text, &current, &p, &after).unwrap();
        assert_eq!(next, "{\n  \"a\": 1\n  // about b\n}\n");
    }

    #[test]
    fn replace_removes_a_last_nested_key_with_a_comment_above_it_and_keeps_trust() {
        let text = r#"{
  "trustedProjects": { "/repo": "abc" },
  "sidebar": {
    "layout": "tree",
    // about agents
    "agents": "list"
  }
}
"#;
        let current = parse_object(text).unwrap();
        let file = obj(json!({ "sidebar": { "layout": "tree" } }));
        let (p, after) = plan(&current, &file, Mode::Replace, Scope::All);
        let next = apply_text(text, &current, &p, &after).unwrap();
        assert_eq!(
            next,
            r#"{
  "trustedProjects": { "/repo": "abc" },
  "sidebar": {
    "layout": "tree"
    // about agents
  }
}
"#
        );
    }

    #[test]
    fn a_second_default_template_is_not_merged() {
        let current = obj(json!({ "spaceTemplates": [ { "name": "d", "agent": "claude" } ] }));
        let file = obj(json!({ "spaceTemplates": [ { "name": "e", "agent": "copilot" } ] }));
        let (p, after) = plan(&current, &file, Mode::Merge, Scope::Template);
        assert_eq!(rejected(&p.invalid), ["spaceTemplates[e]"]);
        assert!(p.added.is_empty());
        assert_eq!(after["spaceTemplates"], current["spaceTemplates"]);
    }

    #[test]
    fn import_apply_writes_the_config_and_plan_writes_nothing() {
        let home = temp_home("apply");
        let config = config_path(&home);
        std::fs::write(&config, "{ \"theme\": \"dark\" }\n").unwrap();
        let file = home.join("import.json");
        std::fs::write(
            &file,
            r#"{ "theme": "light", "theme2": 1, "trustedProjects": { "/x": "y" } }"#,
        )
        .unwrap();

        let p = import_plan(&home, &file, Mode::Merge, Scope::All).unwrap();
        assert_eq!(paths(&p.changed), ["theme"]);
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "{ \"theme\": \"dark\" }\n"
        );

        let done = import_apply(&home, &file, Mode::Merge, Scope::All).unwrap();
        assert_eq!(done, p);
        let written = parse_object(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(
            Value::Object(written),
            json!({ "theme": "light", "theme2": 1 })
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn import_apply_without_a_config_creates_it() {
        let home = temp_home("fresh");
        let file = home.join("import.json");
        std::fs::write(&file, r#"{ "theme": "light" }"#).unwrap();
        import_apply(&home, &file, Mode::Replace, Scope::All).unwrap();
        let written = parse_object(&std::fs::read_to_string(config_path(&home)).unwrap()).unwrap();
        assert_eq!(Value::Object(written), json!({ "theme": "light" }));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn import_refuses_bad_files() {
        let home = temp_home("bad");
        let missing = home.join("missing.json");
        assert!(
            import_plan(&home, &missing, Mode::Merge, Scope::All)
                .unwrap_err()
                .contains("cannot read")
        );
        assert!(
            import_plan(&home, &home, Mode::Merge, Scope::All)
                .unwrap_err()
                .contains("is not a file")
        );
        let big = home.join("big.json");
        std::fs::write(&big, vec![b' '; (MAX_IMPORT_BYTES + 1) as usize]).unwrap();
        assert!(
            import_plan(&home, &big, Mode::Merge, Scope::All)
                .unwrap_err()
                .contains("1 MiB")
        );
        let list = home.join("list.json");
        std::fs::write(&list, "[]").unwrap();
        assert!(
            import_plan(&home, &list, Mode::Merge, Scope::All)
                .unwrap_err()
                .contains("not a JSON object")
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn export_writes_json_files_only() {
        let home = temp_home("export");
        std::fs::write(
            config_path(&home),
            r#"{ "theme": "dark", "trustedProjects": { "/r": "h" } }"#,
        )
        .unwrap();
        let out = home.join("out.json");
        export(&home, &out, None).unwrap();
        let written: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        assert_eq!(written, json!({ "theme": "dark" }));
        assert!(
            export(&home, &home.join("out.txt"), None)
                .unwrap_err()
                .contains(".json")
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
