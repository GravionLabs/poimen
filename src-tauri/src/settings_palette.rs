//! `palette` of the user config (#1218): which detected sources the palette shows and which single
//! detected entries the user hid per repository. User settings only, never written to the
//! repository. `hidden` is keyed by the repository root.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value;

/// One hidden detected entry: the `source` of the detected entry (`npm`, `skills`) and its name.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Hidden {
    pub source: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Palette {
    pub scripts: bool,
    pub skills: bool,
    /// Repository root to its hidden entries.
    pub hidden: BTreeMap<String, Vec<Hidden>>,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            scripts: true,
            skills: true,
            hidden: BTreeMap::new(),
        }
    }
}

/// Resolves the value of the `palette` key; an invalid field warns and keeps its default.
pub fn resolve(value: &Value, warnings: &mut Vec<String>) -> Palette {
    let mut out = Palette::default();
    let Some(obj) = value.as_object() else {
        warnings.push("palette must be an object".into());
        return out;
    };
    match obj.get("sources") {
        None | Some(Value::Null) => {}
        Some(Value::Object(sources)) => {
            for (key, v) in sources {
                match (key.as_str(), v) {
                    ("scripts", Value::Bool(b)) => out.scripts = *b,
                    ("skills", Value::Bool(b)) => out.skills = *b,
                    ("scripts" | "skills", _) => {
                        warnings.push(format!("palette.sources.{key} must be true or false"));
                    }
                    _ => warnings.push(format!("palette.sources.{key} is not a known source")),
                }
            }
        }
        Some(_) => warnings.push("palette.sources must be an object".into()),
    }
    match obj.get("hidden") {
        None | Some(Value::Null) => {}
        Some(Value::Object(repos)) => {
            for (root, list) in repos {
                let hidden = parse_hidden(root, list, warnings);
                if !hidden.is_empty() {
                    out.hidden.insert(root.clone(), hidden);
                }
            }
        }
        Some(_) => warnings.push("palette.hidden must be an object".into()),
    }
    out
}

fn parse_hidden(root: &str, list: &Value, warnings: &mut Vec<String>) -> Vec<Hidden> {
    let Some(items) = list.as_array() else {
        warnings.push(format!("palette.hidden[{root:?}] must be a list"));
        return Vec::new();
    };
    let mut out: Vec<Hidden> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let field = |k: &str| {
            item.get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
        };
        match (field("source"), field("name")) {
            (Some(source), Some(name)) => {
                let h = Hidden {
                    source: source.to_owned(),
                    name: name.to_owned(),
                };
                if !out.contains(&h) {
                    out.push(h);
                }
            }
            _ => warnings.push(format!(
                "palette.hidden[{root:?}][{i}] needs a source and a name; ignored"
            )),
        }
    }
    out
}

/// Returns `text` with `palette` replaced by `palette` after validating it like the loader does
/// (any warning rejects the write); other keys and comments stay.
pub fn set_text(text: &str, palette: &Value) -> Result<String, String> {
    let mut warnings = Vec::new();
    resolve(palette, &mut warnings);
    if !warnings.is_empty() {
        return Err(format!("palette: {}", warnings.join("; ")));
    }
    poimen_core::settings::set_key_text(text, "palette", &palette.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::resolve as load;

    fn palette(json: &str) -> crate::settings::Settings {
        load(Some(json), || None)
    }

    #[test]
    fn defaults_show_both_sources_and_hide_nothing() {
        let s = palette("{}");
        assert_eq!(s.palette, Palette::default());
        assert!(s.warnings.is_empty());
    }

    #[test]
    fn sources_and_hidden_entries_parse() {
        let s = palette(
            r#"{"palette":{"sources":{"scripts":false},"hidden":{"/r":[{"source":"npm","name":"x"},{"source":"npm","name":"x"}]}}}"#,
        );
        assert!(!s.palette.scripts && s.palette.skills);
        assert_eq!(s.palette.hidden["/r"].len(), 1);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    }

    #[test]
    fn invalid_values_warn_and_keep_defaults() {
        assert_eq!(palette(r#"{"palette":3}"#).warnings.len(), 1);
        let s = palette(
            r#"{"palette":{"sources":{"scripts":"no","tabs":true},"hidden":{"/r":[{"source":"","name":"x"}],"/q":5}}}"#,
        );
        assert_eq!(s.palette, Palette::default());
        assert_eq!(s.warnings.len(), 4, "{:?}", s.warnings);
    }

    #[test]
    fn set_text_round_trips_and_refuses_invalid() {
        let value = serde_json::json!({
            "sources": {"scripts": true, "skills": false},
            "hidden": {"/r": [{"source": "npm", "name": "build"}]}
        });
        let out = set_text("{\"theme\": \"dark\"}", &value).unwrap();
        let back = palette(&out);
        assert_eq!(back.theme, "dark");
        assert!(!back.palette.skills);
        assert_eq!(back.palette.hidden["/r"][0].name, "build");
        assert!(set_text("{}", &serde_json::json!({"sources": {"scripts": 1}})).is_err());
    }

    #[test]
    fn import_carries_the_palette_key() {
        use crate::settings_transfer::{Mode, Scope, plan};
        let file = serde_json::json!({"palette": {"sources": {"skills": false}}});
        let (p, after) = plan(
            &Default::default(),
            file.as_object().unwrap(),
            Mode::Merge,
            Scope::All,
        );
        assert!(p.invalid.is_empty(), "{:?}", p.invalid);
        assert_eq!(after["palette"], file["palette"]);
    }
}
