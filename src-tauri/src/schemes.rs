//! User color schemes: WezTerm color scheme TOML files in `~/.config/poimen/themes/`.
//!
//! Files with a `[colors]` table (`ansi`, `brights`, `foreground`, `background`, optional
//! `cursor_bg`, `selection_bg`, `selection_fg`) and an optional `[metadata] name` are parsed into the
//! scheme model of the frontend (`core/color-scheme.ts`). Merging with the bundled schemes (a user
//! scheme with the same name wins) and name resolution happen in the frontend.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;

/// A scheme in the wire shape of the frontend's `ColorScheme`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Scheme {
    pub name: String,
    pub label: String,
    pub background: String,
    pub foreground: String,
    pub cursor: String,
    pub selection_background: String,
    pub selection_foreground: String,
    pub ansi: Vec<String>,
}

/// The loaded schemes and one warning per skipped file.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Schemes {
    pub schemes: Vec<Scheme>,
    pub warnings: Vec<String>,
}

pub fn themes_dir(home: &Path) -> PathBuf {
    home.join(".config/poimen/themes")
}

/// `#rrggbb` (also `#rgb`) as lowercase `#rrggbb`.
fn color(value: &str) -> Option<String> {
    let hex = value.trim().strip_prefix('#')?;
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    match hex.len() {
        6 => Some(format!("#{}", hex.to_ascii_lowercase())),
        3 => Some(format!(
            "#{}",
            hex.chars()
                .flat_map(|c| [c, c])
                .collect::<String>()
                .to_ascii_lowercase()
        )),
        _ => None,
    }
}

fn color_at(colors: &toml::Table, key: &str) -> Result<Option<String>, String> {
    match colors.get(key) {
        None => Ok(None),
        Some(toml::Value::String(s)) => color(s)
            .map(Some)
            .ok_or_else(|| format!("{key}: \"{s}\" is not a #rrggbb color")),
        Some(_) => Err(format!("{key} must be a color string")),
    }
}

fn palette(colors: &toml::Table, key: &str) -> Result<Vec<String>, String> {
    let list = colors
        .get(key)
        .ok_or_else(|| format!("missing {key}"))?
        .as_array()
        .ok_or_else(|| format!("{key} must be a list"))?;
    if list.len() != 8 {
        return Err(format!("{key} needs 8 colors, found {}", list.len()));
    }
    list.iter()
        .map(|v| {
            v.as_str()
                .and_then(color)
                .ok_or_else(|| format!("{key}: {v} is not a #rrggbb color"))
        })
        .collect()
}

/// Lowercase, runs of other characters become one `-`: `Tokyo Night_Storm` -> `tokyo-night-storm`.
fn kebab(stem: &str) -> String {
    stem.to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Parses one WezTerm color scheme; `stem` (the file name without extension) names it when the
/// file has no `[metadata] name`.
pub fn parse_scheme(text: &str, stem: &str) -> Result<Scheme, String> {
    let doc: toml::Table = text
        .parse()
        .map_err(|e: toml::de::Error| e.message().to_string())?;
    let colors = doc
        .get("colors")
        .and_then(|c| c.as_table())
        .ok_or("missing [colors] table")?;
    let mut ansi = palette(colors, "ansi")?;
    ansi.extend(palette(colors, "brights")?);
    let background = color_at(colors, "background")?.ok_or("missing background")?;
    let foreground = color_at(colors, "foreground")?.ok_or("missing foreground")?;
    let cursor = color_at(colors, "cursor_bg")?.unwrap_or_else(|| foreground.clone());
    let selection_background =
        color_at(colors, "selection_bg")?.unwrap_or_else(|| foreground.clone());
    let selection_foreground =
        color_at(colors, "selection_fg")?.unwrap_or_else(|| background.clone());
    let name = doc
        .get("metadata")
        .and_then(|m| m.get("name"))
        .and_then(|n| n.as_str())
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(String::from)
        .unwrap_or_else(|| kebab(stem));
    if name.is_empty() {
        return Err("no usable name (set [metadata] name)".into());
    }
    Ok(Scheme {
        label: name.clone(),
        name,
        background,
        foreground,
        cursor,
        selection_background,
        selection_foreground,
        ansi,
    })
}

/// Reads every `*.toml` of `dir` (sorted by file name). A missing directory is no schemes; an
/// invalid file or a repeated name is skipped with a warning naming the file.
pub fn load_dir(dir: &Path) -> Schemes {
    let mut out = Schemes::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("toml"))
        })
        .collect();
    files.sort();
    for path in files {
        let file = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let stem = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let parsed = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| parse_scheme(&text, &stem));
        match parsed {
            Ok(s) if out.schemes.iter().any(|o| o.name == s.name) => out.warnings.push(format!(
                "color scheme {file}: name \"{}\" is already used by an earlier file; skipped",
                s.name
            )),
            Ok(s) => out.schemes.push(s),
            Err(e) => out
                .warnings
                .push(format!("color scheme {file}: {e}; skipped")),
        }
    }
    out
}

pub fn load(home: &Path) -> Schemes {
    load_dir(&themes_dir(home))
}

/// Names and modification times of the files in the directory.
fn stamp(dir: &Path) -> Vec<(String, Option<SystemTime>)> {
    let mut v: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .map(|e| {
            (
                e.file_name().to_string_lossy().into_owned(),
                e.metadata().and_then(|m| m.modified()).ok(),
            )
        })
        .collect();
    v.sort();
    v
}

/// Runs forever on the calling thread: polls the themes directory and calls `emit` on every change.
pub fn watch(home: &Path, interval: Duration, emit: impl Fn(&Schemes)) {
    let dir = themes_dir(home);
    let mut last = stamp(&dir);
    loop {
        std::thread::sleep(interval);
        let now = stamp(&dir);
        if now != last {
            last = now;
            emit(&load_dir(&dir));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r##"
[colors]
foreground = "#F8F8F2"
background = "#282a36"
cursor_bg = "#ff0000"
selection_bg = "#44475a"
selection_fg = "#ffffff"
ansi = ["#000000", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#bfbfbf"]
brights = ["#4d4d4d", "#ff6e67", "#5af78e", "#f4f99d", "#caa9fa", "#ff92d0", "#9aedfe", "#e6e6e6"]

[metadata]
name = "Dracula (custom)"
"##;

    const MINIMAL: &str = r##"
[colors]
foreground = "#eee"
background = "#111111"
ansi = ["#000000", "#111111", "#222222", "#333333", "#444444", "#555555", "#666666", "#777777"]
brights = ["#888888", "#999999", "#aaaaaa", "#bbbbbb", "#cccccc", "#dddddd", "#eeeeee", "#ffffff"]
"##;

    #[test]
    fn parses_a_full_file() {
        let s = parse_scheme(FULL, "ignored").unwrap();
        assert_eq!(s.name, "Dracula (custom)");
        assert_eq!(s.label, "Dracula (custom)");
        assert_eq!(s.foreground, "#f8f8f2");
        assert_eq!(s.background, "#282a36");
        assert_eq!(s.cursor, "#ff0000");
        assert_eq!(s.selection_background, "#44475a");
        assert_eq!(s.selection_foreground, "#ffffff");
        assert_eq!(s.ansi.len(), 16);
        assert_eq!(s.ansi[5], "#ff79c6");
        assert_eq!(s.ansi[8], "#4d4d4d");
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["selectionBackground"], "#44475a");
    }

    #[test]
    fn missing_optional_keys_use_defaults() {
        let s = parse_scheme(MINIMAL, "my_scheme").unwrap();
        assert_eq!(s.name, "my-scheme");
        assert_eq!(s.foreground, "#eeeeee");
        assert_eq!(s.cursor, "#eeeeee");
        assert_eq!(s.selection_background, "#eeeeee");
        assert_eq!(s.selection_foreground, "#111111");
    }

    #[test]
    fn bad_input_is_an_error() {
        let bad = |text: &str| parse_scheme(text, "x").unwrap_err();
        assert!(
            bad(&MINIMAL.replace("background = \"#111111\"", "background = \"red\""))
                .contains("background")
        );
        assert!(bad(&MINIMAL.replace("\"#777777\"", "\"#77777\"")).contains("ansi"));
        assert!(bad(&MINIMAL.replace(", \"#ffffff\"]", "]")).contains("brights needs 8"));
        assert!(bad("[colors]\nforeground = \"#fff\"").contains("ansi"));
        assert!(bad("[other]\na = 1").contains("[colors]"));
        assert!(!bad("colors = [").is_empty());
        assert!(bad(&MINIMAL.replace("\"#eee\"", "3")).contains("foreground"));
    }

    #[test]
    fn name_falls_back_to_the_kebab_case_file_stem() {
        assert_eq!(
            parse_scheme(MINIMAL, "Tokyo Night_Storm").unwrap().name,
            "tokyo-night-storm"
        );
        let blank = MINIMAL.to_string() + "[metadata]\nname = \"  \"\n";
        assert_eq!(parse_scheme(&blank, "Zenburn").unwrap().name, "zenburn");
        assert!(parse_scheme(MINIMAL, "___").unwrap_err().contains("name"));
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("poimen-schemes-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_dir_skips_invalid_and_repeated_files_with_a_warning() {
        let dir = temp_dir("load");
        std::fs::write(dir.join("a-good.toml"), MINIMAL).unwrap();
        std::fs::write(dir.join("b-broken.toml"), "colors = [").unwrap();
        let again = MINIMAL.to_string() + "[metadata]\nname = \"a-good\"\n";
        std::fs::write(dir.join("c-again.toml"), again).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let got = load_dir(&dir);
        assert_eq!(got.schemes.len(), 1);
        assert_eq!(got.schemes[0].name, "a-good");
        assert_eq!(got.warnings.len(), 2, "{:?}", got.warnings);
        assert!(got.warnings[0].contains("b-broken.toml"));
        assert!(got.warnings[1].contains("c-again.toml"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_directory_has_no_schemes() {
        let got = load(Path::new("/nonexistent/poimen-home"));
        assert!(got.schemes.is_empty() && got.warnings.is_empty());
    }

    #[test]
    fn stamp_changes_when_a_file_appears() {
        let dir = temp_dir("stamp");
        let before = stamp(&dir);
        std::fs::write(dir.join("x.toml"), MINIMAL).unwrap();
        assert_ne!(before, stamp(&dir));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
