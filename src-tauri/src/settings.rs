//! User settings file (`~/.config/poimen/config.json`) and the optional Ghostty config.
//!
//! Preferences only: UI state (sidebar width, open tabs) lives in the frontend UI state.
//! Invalid entries produce a warning and fall back to the default. The only write is `save_theme`
//! (the toolbar button), which edits the `theme` value in place and keeps everything else.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde::Serialize;
use serde_json::Value;

pub use poimen_core::settings::{
    SidebarRows, SidebarSort, config_path, parse_space_templates, parse_tasks,
    parse_trusted_projects, resolve_group_worktrees, resolve_sidebar_rows, resolve_sidebar_sort,
    revoke_project, trust_project,
};
use poimen_core::settings::{
    find_value, remove_path_text, root_open, save_with, set_key_text, set_path_text,
};

pub const DEFAULT_FONT_SIZE: f64 = if cfg!(windows) { 14.0 } else { 13.0 };
const MIN_FONT_SIZE: f64 = 6.0;
const MAX_FONT_SIZE: f64 = 72.0;
pub const DEFAULT_UI_FONT_SIZE: f64 = 13.0;
const MIN_UI_FONT_SIZE: f64 = 10.0;
const MAX_UI_FONT_SIZE: f64 = 20.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Colors {
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub cursor: Option<String>,
    pub selection_background: Option<String>,
    pub selection_foreground: Option<String>,
    /// ANSI colors 0-15 (`palette = N=#rrggbb`); empty when Ghostty sets none, else 16 entries.
    pub palette: Vec<Option<String>>,
}

/// The `colorSchema` setting: one scheme name, or a light/dark pair picked by `theme`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ColorSchema {
    Name(String),
    Pair { light: String, dark: String },
}

/// One of the `sounds` entries: off, the built-in tone, or the path of an audio file.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SoundSetting {
    #[default]
    Off,
    Builtin,
    File(String),
}

impl Serialize for SoundSetting {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Off => serializer.serialize_bool(false),
            Self::Builtin => serializer.serialize_bool(true),
            Self::File(path) => serializer.serialize_str(path),
        }
    }
}

/// The `sounds` setting: what plays when an agent is blocked or done (#724).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct Sounds {
    pub blocked: SoundSetting,
    pub done: SoundSetting,
}

/// Largest audio file `sound_file` reads.
pub const MAX_SOUND_BYTES: u64 = 1024 * 1024;

fn resolve_sounds(value: &Value, warnings: &mut Vec<String>) -> Sounds {
    let mut sounds = Sounds::default();
    let Some(obj) = value.as_object() else {
        warnings.push("sounds must be an object".into());
        return sounds;
    };
    for (key, slot) in [("blocked", &mut sounds.blocked), ("done", &mut sounds.done)] {
        match obj.get(key) {
            None => {}
            Some(Value::Bool(false)) => {}
            Some(Value::Bool(true)) => *slot = SoundSetting::Builtin,
            Some(Value::String(p)) if !p.trim().is_empty() => {
                *slot = SoundSetting::File(p.trim().to_owned());
            }
            Some(other) => warnings.push(format!(
                "sounds.{key} {other} must be true, false or the path of an audio file"
            )),
        }
    }
    sounds
}

/// Reads the audio file of `sounds.<kind>` for the `sound_file` command: the path comes from the
/// settings below `home`, never from the caller. A regular file up to 1 MiB; `~/` is the home.
pub fn read_sound_file(home: &Path, kind: &str) -> Result<Vec<u8>, String> {
    let sounds = load(home).sounds;
    let setting = match kind {
        "blocked" => sounds.blocked,
        "done" => sounds.done,
        other => return Err(format!("unknown sound {other:?}")),
    };
    let SoundSetting::File(path) = setting else {
        return Err(format!("sounds.{kind} is not an audio file"));
    };
    let full = match path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        Some(rest) => home.join(rest),
        None => PathBuf::from(&path),
    };
    let meta = std::fs::metadata(&full).map_err(|e| format!("cannot read {path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path} is not a regular file"));
    }
    if meta.len() > MAX_SOUND_BYTES {
        return Err(format!("{path} is larger than 1 MiB"));
    }
    std::fs::read(&full).map_err(|e| format!("cannot read {path}: {e}"))
}

/// The resolved settings sent to the frontend (event `settings`, command `get_settings`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// `light`, `dark` or `system`.
    pub theme: String,
    /// The `colorSchema`: a name or a light/dark pair; `None` keeps the `poimen` scheme that follows
    /// `theme`. Names are resolved in the frontend.
    pub color_schema: Option<ColorSchema>,
    /// `None` keeps the app's default monospace stack.
    pub font_family: Option<String>,
    pub font_size: f64,
    /// App UI font family (`ui.fontFamily`); `None` keeps the default `--font-ui`.
    pub ui_font_family: Option<String>,
    /// App UI font size in px (`ui.fontSize`).
    pub ui_font_size: f64,
    /// Terminal colors taken from Ghostty; `None` keeps the theme colors.
    pub colors: Option<Colors>,
    /// Open panes for input when no other client controls them (`terminal.autoControl`).
    pub auto_control: bool,
    /// Accelerator per action id (see `ACTIONS`), defaults merged with the valid `shortcuts`
    /// overrides; an empty string means unbound.
    pub shortcuts: BTreeMap<String, String>,
    /// Lines and tokens of the sidebar rows from `sidebar.rows`; types left `None` use the default.
    pub sidebar_rows: SidebarRows,
    /// `sidebar.layout`: `tree` (default) or `split`.
    pub sidebar_layout: String,
    /// `sidebar.agents`: `list` (default) or `priority`, the order of the Agents section (#722).
    pub sidebar_agents: String,
    /// `sidebar.sort`: `manual` (default), `name`, `attention` or `recent`.
    pub sidebar_sort: SidebarSort,
    /// `sidebar.groupWorktrees`: list linked worktrees below the space of their repository.
    pub sidebar_group_worktrees: bool,
    /// `sidebar.showBaseBranch`: show the branch token also on the base branch (default false).
    pub sidebar_show_base_branch: bool,
    /// Roots of the explorer (`explorer.roots`, a leading `~` is the home directory).
    pub explorer_roots: Vec<String>,
    /// Whether the explorer shows hidden directories (`explorer.showHidden`).
    pub explorer_show_hidden: bool,
    /// Lines of scrollback xterm.js keeps on an emulated stream (`terminal.scrollback`).
    pub scrollback: u32,
    /// The pane layout of a new space (`spaces.layout`); `None` uses the built-in default.
    pub spaces_layout: Option<crate::layout::Layout>,
    /// The space templates (`spaceTemplates`); invalid entries are skipped with a warning.
    pub space_templates: Vec<crate::templates::Template>,
    /// Repository root to the SHA-256 of the project file the user trusted (`trustedProjects`).
    pub trusted_projects: BTreeMap<String, String>,
    /// The task modes (`tasks`); the default is one `agent` mode.
    pub tasks: crate::tasks::TaskModes,
    /// The agent harnesses (`harnesses`); the defaults are `claude` and `copilot`.
    pub harnesses: Vec<crate::harness::Harness>,
    /// The user's `github.agentActions` entries (merged over the defaults by
    /// `get_agent_actions`); not sent to the frontend.
    #[serde(skip)]
    pub agent_actions: Vec<crate::agent_actions::Spec>,
    /// The remote machines (`machines`); invalid entries are skipped with a warning.
    pub machines: Vec<crate::machines::MachineConfig>,
    /// What plays when an agent is blocked or done (`sounds`).
    pub sounds: Sounds,
    /// `agents.claude.classicRenderer`: new poimen terminals set
    /// `CLAUDE_CODE_DISABLE_ALTERNATE_SCREEN=1` (default false).
    pub claude_classic_renderer: bool,
    /// The home directory `~` stands for; set by `load` so the frontend can compare spellings.
    pub home: Option<String>,
    pub warnings: Vec<String>,
}

/// Default and upper bound of `terminal.scrollback`.
pub const DEFAULT_SCROLLBACK: u32 = 10_000;
const MAX_SCROLLBACK: u32 = 1_000_000;

/// Menu/shortcut actions with their default accelerators (VS Code style). `CmdOrCtrl` is Cmd on
/// macOS and Ctrl elsewhere.
pub const ACTIONS: &[(&str, &str)] = &[
    ("newTerminal", "CmdOrCtrl+T"),
    ("closeTab", "CmdOrCtrl+W"),
    ("closeAll", "CmdOrCtrl+Shift+W"),
    ("refresh", "CmdOrCtrl+R"),
    ("toggleSidebar", "CmdOrCtrl+B"),
    ("toggleOutput", "CmdOrCtrl+J"),
    ("splitRight", "CmdOrCtrl+\\"),
    ("splitDown", "CmdOrCtrl+Shift+\\"),
    ("zoomIn", "CmdOrCtrl+="),
    ("zoomOut", "CmdOrCtrl+-"),
    ("zoomReset", "CmdOrCtrl+0"),
    ("commandPalette", "CmdOrCtrl+Shift+P"),
    ("jump", "CmdOrCtrl+K"),
    ("nextWaiting", "CmdOrCtrl+Shift+A"),
    ("nextTab", "CmdOrCtrl+Alt+Right"),
    ("previousTab", "CmdOrCtrl+Alt+Left"),
    ("showShortcuts", "CmdOrCtrl+/"),
    ("find", "CmdOrCtrl+Shift+F"),
];

pub fn default_shortcuts() -> BTreeMap<String, String> {
    ACTIONS
        .iter()
        .map(|(id, acc)| ((*id).to_owned(), (*acc).to_owned()))
        .collect()
}

/// Canonical form of an accelerator such as `cmd+shift+w` -> `Cmd+Shift+W`; `None` when invalid.
/// A shortcut needs a Cmd, Ctrl or Alt modifier, or is a function key.
pub fn normalize_accelerator(text: &str) -> Option<String> {
    let parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let (key, mods) = parts.split_last()?;
    let (mut cmd_or_ctrl, mut cmd, mut ctrl, mut alt, mut shift) =
        (false, false, false, false, false);
    for m in mods {
        match m.to_ascii_lowercase().as_str() {
            "cmdorctrl" | "commandorcontrol" | "mod" => cmd_or_ctrl = true,
            "cmd" | "command" | "meta" | "super" => cmd = true,
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            _ => return None,
        }
    }
    let lower = key.to_ascii_lowercase();
    let named = match lower.as_str() {
        "tab" => Some("Tab"),
        "enter" | "return" => Some("Enter"),
        "space" => Some("Space"),
        "escape" | "esc" => Some("Escape"),
        "left" | "arrowleft" => Some("Left"),
        "right" | "arrowright" => Some("Right"),
        "up" | "arrowup" => Some("Up"),
        "down" | "arrowdown" => Some("Down"),
        "pageup" => Some("PageUp"),
        "pagedown" => Some("PageDown"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "backspace" => Some("Backspace"),
        "delete" => Some("Delete"),
        "plus" => Some("Plus"),
        _ => None,
    };
    let function_key = lower
        .strip_prefix('f')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=12).contains(&n));
    let key = if let Some(n) = named {
        n.to_owned()
    } else if function_key {
        lower.to_uppercase()
    } else if key.chars().count() == 1 && !key.chars().all(char::is_whitespace) && *key != "+" {
        key.to_uppercase()
    } else {
        return None;
    };
    if !(cmd_or_ctrl || cmd || ctrl || alt || function_key) {
        return None;
    }
    let mut out = String::new();
    for (on, name) in [
        (cmd_or_ctrl, "CmdOrCtrl"),
        (cmd, "Cmd"),
        (ctrl, "Ctrl"),
        (alt, "Alt"),
        (shift, "Shift"),
    ] {
        if on {
            out.push_str(name);
            out.push('+');
        }
    }
    out.push_str(&key);
    Some(out)
}

/// Defaults overlaid with the `shortcuts` object; unknown actions, invalid accelerators and
/// accelerators already used by another action warn and keep the default.
fn resolve_shortcuts(value: &Value, warnings: &mut Vec<String>) -> BTreeMap<String, String> {
    let mut out = default_shortcuts();
    let Some(map) = value.as_object() else {
        warnings.push("shortcuts must be an object".into());
        return out;
    };
    for key in map.keys() {
        if !ACTIONS.iter().any(|(id, _)| id == key) {
            warnings.push(format!("shortcuts.{key} is not a known action; ignored"));
        }
    }
    let mut chosen: Vec<(&str, &str, String)> = Vec::new();
    for (id, default) in ACTIONS {
        let mut acc = (*default).to_owned();
        match map.get(*id) {
            None => {}
            Some(Value::String(s)) if s.trim().is_empty() => acc = String::new(),
            Some(Value::String(s)) => match normalize_accelerator(s) {
                Some(n) => acc = n,
                None => warnings.push(format!(
                    "shortcuts.{id} \"{s}\" is not a valid shortcut; using {default}"
                )),
            },
            Some(other) => warnings.push(format!(
                "shortcuts.{id} {other} must be a string; using {default}"
            )),
        }
        chosen.push((id, default, acc));
    }
    // An override that collides with another action's shortcut loses to the default.
    let all: Vec<String> = chosen.iter().map(|(_, _, a)| a.clone()).collect();
    for (id, default, acc) in chosen {
        let clash = !acc.is_empty() && all.iter().filter(|a| **a == acc).count() > 1;
        if clash && acc != default {
            warnings.push(format!(
                "shortcuts.{id} {acc} is already used by another action; using {default}"
            ));
            out.insert(id.to_owned(), default.to_owned());
        } else {
            out.insert(id.to_owned(), acc);
        }
    }
    out
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            color_schema: None,
            font_family: None,
            font_size: DEFAULT_FONT_SIZE,
            ui_font_family: None,
            ui_font_size: DEFAULT_UI_FONT_SIZE,
            colors: None,
            auto_control: true,
            shortcuts: default_shortcuts(),
            sidebar_rows: SidebarRows::default(),
            sidebar_layout: "tree".into(),
            sidebar_agents: "list".into(),
            sidebar_sort: SidebarSort::Manual,
            sidebar_group_worktrees: true,
            sidebar_show_base_branch: false,
            explorer_roots: vec!["~".into()],
            explorer_show_hidden: false,
            scrollback: DEFAULT_SCROLLBACK,
            spaces_layout: None,
            space_templates: Vec::new(),
            trusted_projects: BTreeMap::new(),
            tasks: crate::tasks::TaskModes::default(),
            harnesses: crate::harness::default_harnesses(),
            agent_actions: Vec::new(),
            machines: Vec::new(),
            sounds: Sounds::default(),
            claude_classic_renderer: false,
            home: None,
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct Ghostty {
    pub font_family: Option<String>,
    pub font_size: Option<f64>,
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub cursor: Option<String>,
    pub selection_background: Option<String>,
    pub selection_foreground: Option<String>,
    /// Empty, or 16 entries once a `palette` line was read.
    pub palette: Vec<Option<String>>,
}

fn valid_size(size: f64) -> bool {
    size.is_finite() && (MIN_FONT_SIZE..=MAX_FONT_SIZE).contains(&size)
}

fn valid_ui_size(size: f64) -> bool {
    size.is_finite() && (MIN_UI_FONT_SIZE..=MAX_UI_FONT_SIZE).contains(&size)
}

fn hex_color(value: &str) -> Option<String> {
    let hex = value.trim().trim_start_matches('#');
    (matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| format!("#{hex}"))
}

/// Reads the keys poimen uses from a Ghostty config (`key = value`, `#` comments).
pub fn parse_ghostty(text: &str) -> Ghostty {
    let mut out = Ghostty::default();
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once('=') else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        match key.trim() {
            // The first family wins; later ones are fallbacks.
            "font-family" if out.font_family.is_none() && !value.is_empty() => {
                out.font_family = Some(value.to_owned());
            }
            "font-size" => out.font_size = value.parse().ok().filter(|s| valid_size(*s)),
            "background" => out.background = hex_color(value),
            "foreground" => out.foreground = hex_color(value),
            "cursor-color" => out.cursor = hex_color(value),
            "selection-background" => out.selection_background = hex_color(value),
            "selection-foreground" => out.selection_foreground = hex_color(value),
            "palette" => {
                if let Some((index, color)) = value.split_once('=')
                    && let Ok(index) = index.trim().parse::<usize>()
                    && index < 16
                    && let Some(color) = hex_color(color.trim().trim_matches('"'))
                {
                    out.palette.resize(16, None);
                    out.palette[index] = Some(color);
                }
            }
            _ => {}
        }
    }
    out
}

/// `agents.claude.classicRenderer`; anything but a boolean warns and keeps the default.
fn resolve_agents(value: &Value, s: &mut Settings) {
    let Some(agents) = value.as_object() else {
        s.warnings.push("agents must be an object".into());
        return;
    };
    let Some(claude) = agents.get("claude") else {
        return;
    };
    let Some(claude) = claude.as_object() else {
        s.warnings.push("agents.claude must be an object".into());
        return;
    };
    match claude.get("classicRenderer") {
        None => {}
        Some(Value::Bool(b)) => s.claude_classic_renderer = *b,
        Some(_) => s
            .warnings
            .push("agents.claude.classicRenderer must be true or false".into()),
    }
}

/// `explorer.roots` (non-empty list of absolute or `~` paths) and `explorer.showHidden`; invalid
/// entries warn and keep the default.
fn resolve_explorer(value: &Value, s: &mut Settings) {
    let Some(obj) = value.as_object() else {
        s.warnings.push("explorer must be an object".into());
        return;
    };
    match obj.get("roots") {
        None => {}
        Some(Value::Array(items)) => {
            let roots: Option<Vec<String>> = items
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::trim)
                        .filter(|p| is_root_path(p))
                        .map(str::to_owned)
                })
                .collect();
            match roots {
                Some(r) if !r.is_empty() => s.explorer_roots = r,
                _ => s.warnings.push(
                    "explorer.roots must be a non-empty list of absolute paths (or ~); using the home directory"
                        .into(),
                ),
            }
        }
        Some(_) => s
            .warnings
            .push("explorer.roots must be a list; using the home directory".into()),
    }
    match obj.get("showHidden") {
        None => {}
        Some(Value::Bool(b)) => s.explorer_show_hidden = *b,
        Some(_) => s
            .warnings
            .push("explorer.showHidden must be true or false".into()),
    }
}

/// `spaces.layout` (the layout-bootstrap schema); an invalid one warns and keeps the default.
fn resolve_spaces(value: &Value, s: &mut Settings) {
    let Some(obj) = value.as_object() else {
        s.warnings.push("spaces must be an object".into());
        return;
    };
    if let Some(layout) = obj.get("layout") {
        match crate::layout::parse_layout_value(layout) {
            Ok(l) => s.spaces_layout = Some(l),
            Err(e) => s
                .warnings
                .push(format!("spaces.layout: {e}; using the default layout")),
        }
    }
}

/// `trustedProjects`: root to hash; entries that are not strings are skipped with a warning.
fn resolve_trusted_projects(value: &Value, s: &mut Settings) {
    s.trusted_projects = parse_trusted_projects(value, &mut s.warnings);
}

/// `tasks` of the user config: `modes[]` and `worktreeDir` over the defaults.
fn resolve_tasks(value: &Value, s: &mut Settings) {
    let parsed = parse_tasks(value, "using the default agent mode", &mut s.warnings);
    if let Some(dir) = parsed.worktree_dir {
        s.tasks.worktree_dir = dir;
    }
    if let Some(modes) = parsed.modes {
        s.tasks.modes = modes;
    }
}

/// `github.agentActions`: invalid entries and unknown harnesses warn and are skipped.
fn resolve_github(value: &Value, s: &mut Settings) {
    let Some(obj) = value.as_object() else {
        s.warnings.push("github must be an object".into());
        return;
    };
    if let Some(actions) = obj.get("agentActions") {
        let at = "github.agentActions";
        let parsed = crate::agent_actions::parse(actions, at, &mut s.warnings);
        s.agent_actions =
            crate::agent_actions::check_harnesses(parsed, &s.harnesses, at, &mut s.warnings);
    }
}

/// Resolves the settings from the file contents. `read_ghostty` is only called when the
/// `terminal.ghostty` flag is set.
pub fn resolve(json: Option<&str>, read_ghostty: impl FnOnce() -> Option<String>) -> Settings {
    let mut s = Settings::default();
    let Some(json) = json else { return s };
    let root: Value = match serde_json::from_str(json) {
        Ok(v @ Value::Object(_)) => v,
        _ => {
            s.warnings
                .push("config.json is not a valid JSON object; using defaults".into());
            return s;
        }
    };

    match root.get("theme") {
        None => {}
        Some(Value::String(t)) if matches!(t.as_str(), "light" | "dark" | "system") => {
            s.theme = t.clone();
        }
        Some(other) => s.warnings.push(format!(
            "theme {other} must be \"light\", \"dark\" or \"system\""
        )),
    }

    match root.get("colorSchema") {
        None => {}
        // The herdr scheme followed herdr's own config; herdr is no longer supported (#174).
        Some(Value::String(name)) if name.trim() == "herdr" => {}
        Some(Value::String(name)) if !name.trim().is_empty() => {
            s.color_schema = Some(ColorSchema::Name(name.trim().to_owned()));
        }
        Some(Value::Object(pair)) => {
            let side = |key: &str| match pair.get(key) {
                Some(Value::String(n)) if !n.trim().is_empty() => Some(n.trim().to_owned()),
                _ => None,
            };
            match (side("light"), side("dark")) {
                (Some(light), Some(dark)) => {
                    s.color_schema = Some(ColorSchema::Pair { light, dark });
                }
                _ => s.warnings.push(
                    "colorSchema pair needs \"light\" and \"dark\" scheme names; using poimen"
                        .to_owned(),
                ),
            }
        }
        Some(other) => s.warnings.push(format!(
            "colorSchema {other} must be a scheme name or a {{\"light\", \"dark\"}} pair; using poimen"
        )),
    }

    if let Some(value) = root.get("shortcuts") {
        s.shortcuts = resolve_shortcuts(value, &mut s.warnings);
    }

    if let Some(value) = root.get("sidebar") {
        s.sidebar_rows = resolve_sidebar_rows(value, &mut s.warnings);
        s.sidebar_sort = resolve_sidebar_sort(value, &mut s.warnings);
        s.sidebar_group_worktrees = resolve_group_worktrees(value, &mut s.warnings);
        match value.get("layout") {
            None => {}
            Some(Value::String(l)) if matches!(l.as_str(), "tree" | "split") => {
                s.sidebar_layout = l.clone();
            }
            Some(other) => s.warnings.push(format!(
                "sidebar.layout {other} must be \"tree\" or \"split\"; using tree"
            )),
        }
        match value.get("agents") {
            None => {}
            Some(Value::String(l)) if matches!(l.as_str(), "list" | "priority") => {
                s.sidebar_agents = l.clone();
            }
            Some(other) => s.warnings.push(format!(
                "sidebar.agents {other} must be \"list\" or \"priority\"; using list"
            )),
        }
        match value.get("showBaseBranch") {
            None => {}
            Some(Value::Bool(b)) => s.sidebar_show_base_branch = *b,
            Some(_) => s
                .warnings
                .push("sidebar.showBaseBranch must be true or false; using false".into()),
        }
    }

    if let Some(value) = root.get("explorer") {
        resolve_explorer(value, &mut s);
    }

    if let Some(value) = root.get("trustedProjects") {
        resolve_trusted_projects(value, &mut s);
    }
    if let Some(value) = root.get("spaces") {
        resolve_spaces(value, &mut s);
    }
    if let Some(value) = root.get("spaceTemplates") {
        s.space_templates = parse_space_templates(value, &mut s.warnings);
    }
    if let Some(value) = root.get("tasks") {
        resolve_tasks(value, &mut s);
    }
    if let Some(value) = root.get("harnesses") {
        s.harnesses = crate::harness::resolve(value, &mut s.warnings);
    }
    if let Some(value) = root.get("github") {
        resolve_github(value, &mut s);
    }
    if let Some(value) = root.get("machines") {
        s.machines = crate::machines::parse(value, &mut s.warnings);
    }
    if let Some(value) = root.get("sounds") {
        s.sounds = resolve_sounds(value, &mut s.warnings);
    }
    if let Some(value) = root.get("agents") {
        resolve_agents(value, &mut s);
    }

    if let Some(ui) = root.get("ui") {
        if let Some(ui) = ui.as_object() {
            match ui.get("fontFamily") {
                None | Some(Value::Null) => {}
                Some(Value::String(f)) if !f.trim().is_empty() => {
                    s.ui_font_family = Some(f.trim().to_owned());
                }
                Some(other) => s
                    .warnings
                    .push(format!("ui.fontFamily {other} must be a non-empty string")),
            }
            if let Some(v) = ui.get("fontSize") {
                match v.as_f64().filter(|n| valid_ui_size(*n)) {
                    Some(n) => s.ui_font_size = n,
                    None => s.warnings.push(format!(
                        "ui.fontSize {v} must be a number from {MIN_UI_FONT_SIZE} to {MAX_UI_FONT_SIZE}"
                    )),
                }
            }
        } else {
            s.warnings.push("ui must be an object".into());
        }
    }

    let mut use_ghostty = false;
    let mut explicit_family = false;
    let mut explicit_size = false;
    if let Some(term) = root.get("terminal") {
        if let Some(term) = term.as_object() {
            match term.get("fontFamily") {
                None => {}
                Some(Value::String(f)) if !f.trim().is_empty() => {
                    s.font_family = Some(f.trim().to_owned());
                    explicit_family = true;
                }
                Some(other) => s.warnings.push(format!(
                    "terminal.fontFamily {other} must be a non-empty string"
                )),
            }
            if let Some(v) = term.get("fontSize") {
                match v.as_f64().filter(|n| valid_size(*n)) {
                    Some(n) => {
                        s.font_size = n;
                        explicit_size = true;
                    }
                    None => s.warnings.push(format!(
                        "terminal.fontSize {v} must be a number from {MIN_FONT_SIZE} to {MAX_FONT_SIZE}"
                    )),
                }
            }
            match term.get("scrollback") {
                None => {}
                Some(v) => match v.as_u64().filter(|n| *n <= u64::from(MAX_SCROLLBACK)) {
                    Some(n) => s.scrollback = n as u32,
                    None => s.warnings.push(format!(
                        "terminal.scrollback {v} must be a whole number from 0 to {MAX_SCROLLBACK}; using {DEFAULT_SCROLLBACK}"
                    )),
                },
            }
            match term.get("autoControl") {
                None => {}
                Some(Value::Bool(b)) => s.auto_control = *b,
                Some(other) => s.warnings.push(format!(
                    "terminal.autoControl {other} must be true or false"
                )),
            }
            match term.get("ghostty") {
                None => {}
                Some(Value::Bool(b)) => use_ghostty = *b,
                Some(other) => s
                    .warnings
                    .push(format!("terminal.ghostty {other} must be true or false")),
            }
        } else {
            s.warnings.push("terminal must be an object".into());
        }
    }

    if use_ghostty {
        match read_ghostty() {
            None => s
                .warnings
                .push("Ghostty config not found; ignoring terminal.ghostty".into()),
            Some(text) => {
                let g = parse_ghostty(&text);
                // Explicit poimen entries win over Ghostty.
                if !explicit_family && g.font_family.is_some() {
                    s.font_family = g.font_family;
                }
                if !explicit_size && let Some(size) = g.font_size {
                    s.font_size = size;
                }
                if g.background.is_some()
                    || g.foreground.is_some()
                    || g.cursor.is_some()
                    || g.selection_background.is_some()
                    || g.selection_foreground.is_some()
                    || !g.palette.is_empty()
                {
                    s.colors = Some(Colors {
                        background: g.background,
                        foreground: g.foreground,
                        cursor: g.cursor,
                        selection_background: g.selection_background,
                        selection_foreground: g.selection_foreground,
                        palette: g.palette,
                    });
                }
            }
        }
    }
    s
}

/// Ghostty's config. Ghostty has no Windows build, so this is intentionally Unix-only.
pub fn ghostty_path(home: &Path) -> PathBuf {
    home.join(".config").join("ghostty").join("config")
}

/// The keys of an older config that poimen ignores since the herdr backend is gone (#174): the
/// `backend` and `herdr` settings and `colorSchema: "herdr"`. Each one is named once in a log
/// line; none is an error.
pub fn ignored_herdr_keys(text: &str) -> Vec<&'static str> {
    let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };
    let mut keys = Vec::new();
    if root.contains_key("backend") {
        keys.push("backend");
    }
    if root.contains_key("herdr") {
        keys.push("herdr");
    }
    if root
        .get("colorSchema")
        .and_then(Value::as_str)
        .map(str::trim)
        == Some("herdr")
    {
        keys.push("colorSchema \"herdr\"");
    }
    keys
}

/// Reads and resolves the settings below `home`; a missing file means defaults.
pub fn load(home: &Path) -> Settings {
    let json = match std::fs::read_to_string(config_path(home)) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            let mut s = Settings {
                home: Some(home.to_string_lossy().into_owned()),
                ..Settings::default()
            };
            s.warnings.push(format!("cannot read config.json: {e}"));
            return s;
        }
    };
    let mut s = resolve(json.as_deref(), || {
        std::fs::read_to_string(ghostty_path(home)).ok()
    });
    if let Some(text) = &json {
        for key in ignored_herdr_keys(text) {
            eprintln!("poimen: ignoring {key} in config.json: the herdr backend was removed");
        }
    }
    s.home = Some(home.to_string_lossy().into_owned());
    s
}

/// Returns `text` with the top-level `theme` set to `theme`, touching nothing else: other keys,
/// comments and formatting stay. A missing `theme` is inserted as the first entry.
pub fn set_theme_text(text: &str, theme: &str) -> Result<String, String> {
    if !matches!(theme, "light" | "dark" | "system") {
        return Err(format!("invalid theme {theme:?}"));
    }
    set_key_text(text, "theme", &format!("\"{theme}\""))
}

/// Returns `text` with `colorSchema` set to a scheme name or a `{light, dark}` pair, like
/// `set_theme_text`: only that key changes, comments and the rest stay.
pub fn set_color_schema_text(text: &str, schema: &serde_json::Value) -> Result<String, String> {
    let name = |v: Option<&serde_json::Value>| {
        v.and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .is_some()
    };
    let valid = match schema {
        serde_json::Value::String(_) => name(Some(schema)),
        serde_json::Value::Object(o) => o.len() == 2 && name(o.get("light")) && name(o.get("dark")),
        _ => false,
    };
    if !valid {
        return Err("colorSchema must be a name or a {light, dark} pair of names".into());
    }
    set_key_text(text, "colorSchema", &schema.to_string())
}

/// Returns `text` with `sidebar.layout` set to `tree` or `split`, touching nothing else. A missing
/// `sidebar` object is created; a `sidebar` that is not an object is an error.
pub fn set_sidebar_layout_text(text: &str, layout: &str) -> Result<String, String> {
    if !matches!(layout, "tree" | "split") {
        return Err(format!("invalid sidebar layout {layout:?}"));
    }
    set_path_text(text, &["sidebar", "layout"], &format!("\"{layout}\""))
}

/// Returns `text` with `sidebar.agents` set to `list` or `priority`, touching nothing else.
pub fn set_sidebar_agents_text(text: &str, mode: &str) -> Result<String, String> {
    if !matches!(mode, "list" | "priority") {
        return Err(format!("invalid sidebar agents mode {mode:?}"));
    }
    set_path_text(text, &["sidebar", "agents"], &format!("\"{mode}\""))
}

/// Returns `text` with `sidebar.sort` set to one of its four values, touching nothing else.
pub fn set_sidebar_sort_text(text: &str, sort: &str) -> Result<String, String> {
    let Some(sort) = SidebarSort::parse(sort) else {
        return Err(format!("invalid sidebar sort {sort:?}"));
    };
    set_path_text(
        text,
        &["sidebar", "sort"],
        &format!("\"{}\"", sort.as_str()),
    )
}

/// Returns `text` with `spaces.layout` set to `layout` after validating it like the settings
/// loader does; other keys, comments and formatting stay.
pub fn set_spaces_layout_text(text: &str, layout: &Value) -> Result<String, String> {
    crate::layout::parse_layout_value(layout).map_err(|e| format!("spaces.layout: {e}"))?;
    set_path_text(text, &["spaces", "layout"], &layout.to_string())
}

/// Returns `text` without `spaces.layout`, so new spaces use the built-in default again.
pub fn remove_spaces_layout_text(text: &str) -> Result<String, String> {
    remove_path_text(text, &["spaces", "layout"])
}

/// Returns `text` with `spaceTemplates` replaced by `templates` after validating it like the
/// settings loader does (any warning rejects the write); other keys and comments stay.
pub fn set_space_templates_text(text: &str, templates: &Value) -> Result<String, String> {
    let mut warnings = Vec::new();
    let parsed = parse_space_templates(templates, &mut warnings);
    if warnings.is_empty() && parsed.len() != templates.as_array().map_or(0, Vec::len) {
        warnings.push("a template was not accepted".into());
    }
    if !warnings.is_empty() {
        return Err(format!("spaceTemplates: {}", warnings.join("; ")));
    }
    set_key_text(text, "spaceTemplates", &templates.to_string())
}

/// A Windows absolute path: drive letter, colon, separator (`C:\x`, `c:/x`).
fn is_windows_absolute(path: &str) -> bool {
    let b = path.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'\\' | b'/')
}

/// An explorer root as stored: absolute (`/srv`, `C:\x`, `C:/x`), `~`, or below it (`~/x`, `~\x`).
fn is_root_path(path: &str) -> bool {
    path == "~"
        || path.starts_with("~/")
        || path.starts_with("~\\")
        || path.starts_with('/')
        || is_windows_absolute(path)
}

/// Writes a path below `home` as `~` or `~/…`; other paths stay as they are. With a Windows home
/// (`C:\Users\me`) both separators match and the comparison ignores ASCII case.
pub fn tilde_path(path: &str, home: &Path) -> String {
    let original = path.trim();
    let home = home.to_string_lossy();
    let windows = is_windows_absolute(&home);
    let norm = |s: &str| {
        if windows {
            s.replace('\\', "/")
        } else {
            s.to_owned()
        }
    };
    let (path, home) = (norm(original), norm(&home));
    let home = home.trim_end_matches('/');
    if home.is_empty() {
        return original.to_owned();
    }
    let rest = if windows {
        path.get(..home.len())
            .filter(|head| head.eq_ignore_ascii_case(home))
            .map(|_| &path[home.len()..])
    } else {
        path.strip_prefix(home)
    };
    match rest {
        Some("") | Some("/") => "~".to_owned(),
        Some(rest) if rest.starts_with('/') => format!("~{}", rest.trim_end_matches('/')),
        _ => original.to_owned(),
    }
}

/// Returns `text` with `explorer.roots` set to `roots`, touching nothing else. Every root must be
/// absolute or start with `~`, and the list must not be empty.
pub fn set_explorer_roots_text(text: &str, roots: &[String]) -> Result<String, String> {
    if roots.is_empty() {
        return Err("explorer.roots needs at least one folder".into());
    }
    if let Some(bad) = roots.iter().find(|p| !is_root_path(p)) {
        return Err(format!("{bad:?} is not an absolute path"));
    }
    let value = serde_json::to_string(roots).map_err(|e| e.to_string())?;
    set_path_text(text, &["explorer", "roots"], &value)
}

/// Returns `text` with `agents.claude.classicRenderer` set, touching nothing else.
pub fn set_claude_classic_renderer_text(text: &str, on: bool) -> Result<String, String> {
    set_path_text(
        text,
        &["agents", "claude", "classicRenderer"],
        &on.to_string(),
    )
}

/// Returns `text` with `explorer.showHidden` set, touching nothing else.
pub fn set_explorer_show_hidden_text(text: &str, show: bool) -> Result<String, String> {
    set_path_text(text, &["explorer", "showHidden"], &show.to_string())
}

/// Writes the `theme` setting below `home` (creating the file when missing). The watcher then
/// emits the new settings like for any external edit.
pub fn save_theme(home: &Path, theme: &str) -> Result<(), String> {
    save_with(home, |text| set_theme_text(text, theme))
}

/// Writes the `colorSchema` setting below `home`, like `save_theme`.
pub fn save_color_schema(home: &Path, schema: &serde_json::Value) -> Result<(), String> {
    save_with(home, |text| set_color_schema_text(text, schema))
}

/// Writes `sidebar.agents` below `home`, like `save_sidebar_sort`.
pub fn save_sidebar_agents(home: &Path, mode: &str) -> Result<(), String> {
    save_with(home, |text| set_sidebar_agents_text(text, mode))
}

/// Writes `sidebar.sort` below `home`, like `save_sidebar_layout`.
pub fn save_sidebar_sort(home: &Path, sort: &str) -> Result<(), String> {
    save_with(home, |text| set_sidebar_sort_text(text, sort))
}

/// Writes `sidebar.layout` below `home`, like `save_theme`.
pub fn save_sidebar_layout(home: &Path, layout: &str) -> Result<(), String> {
    save_with(home, |text| set_sidebar_layout_text(text, layout))
}

/// Writes `explorer.roots` below `home`, like `save_theme`; paths under `home` become `~/…`.
pub fn save_explorer_roots(home: &Path, roots: &[String]) -> Result<(), String> {
    let mut shortened: Vec<String> = Vec::new();
    for root in roots.iter().map(|r| tilde_path(r, home)) {
        if !shortened.contains(&root) {
            shortened.push(root);
        }
    }
    let roots = shortened;
    save_with(home, |text| set_explorer_roots_text(text, &roots))
}

/// Returns `text` with `terminal.fontFamily` set; an empty family removes the key.
pub fn set_terminal_font_family_text(text: &str, family: &str) -> Result<String, String> {
    set_font_family_text(text, "terminal", family)
}

/// Returns `text` with `terminal.fontSize` set (`MIN_FONT_SIZE`..`MAX_FONT_SIZE`).
pub fn set_terminal_font_size_text(text: &str, size: f64) -> Result<String, String> {
    if !valid_size(size) {
        return Err(format!(
            "terminal.fontSize {size} must be a number from {MIN_FONT_SIZE} to {MAX_FONT_SIZE}"
        ));
    }
    set_path_text(text, &["terminal", "fontSize"], &size.to_string())
}

/// Returns `text` without `terminal.fontSize`: the size is the platform (or Ghostty) default again.
pub fn clear_terminal_font_size_text(text: &str) -> Result<String, String> {
    remove_path_text(text, &["terminal", "fontSize"])
}

/// Returns `text` with `ui.fontFamily` set; an empty family removes the key.
pub fn set_ui_font_family_text(text: &str, family: &str) -> Result<String, String> {
    set_font_family_text(text, "ui", family)
}

/// Returns `text` with `ui.fontSize` set (10 to 20).
pub fn set_ui_font_size_text(text: &str, size: f64) -> Result<String, String> {
    if !valid_ui_size(size) {
        return Err(format!(
            "ui.fontSize {size} must be a number from {MIN_UI_FONT_SIZE} to {MAX_UI_FONT_SIZE}"
        ));
    }
    set_path_text(text, &["ui", "fontSize"], &size.to_string())
}

fn set_font_family_text(text: &str, section: &str, family: &str) -> Result<String, String> {
    let family = family.trim();
    if family.is_empty() {
        return remove_path_text(text, &[section, "fontFamily"]);
    }
    let value = serde_json::to_string(family).map_err(|e| e.to_string())?;
    set_path_text(text, &[section, "fontFamily"], &value)
}

/// Writes `terminal.fontFamily` below `home`, like `save_theme`.
pub fn save_terminal_font_family(home: &Path, family: &str) -> Result<(), String> {
    save_with(home, |t| set_terminal_font_family_text(t, family))
}

/// Writes `terminal.fontSize` below `home`, like `save_theme`.
pub fn save_terminal_font_size(home: &Path, size: f64) -> Result<(), String> {
    save_with(home, |t| set_terminal_font_size_text(t, size))
}

/// Removes `terminal.fontSize` below `home`, like `save_theme`.
pub fn clear_terminal_font_size(home: &Path) -> Result<(), String> {
    save_with(home, clear_terminal_font_size_text)
}

/// Writes `ui.fontFamily` below `home`, like `save_theme`.
pub fn save_ui_font_family(home: &Path, family: &str) -> Result<(), String> {
    save_with(home, |t| set_ui_font_family_text(t, family))
}

/// Writes `ui.fontSize` below `home`, like `save_theme`.
pub fn save_ui_font_size(home: &Path, size: f64) -> Result<(), String> {
    save_with(home, |t| set_ui_font_size_text(t, size))
}

/// Returns `text` with `sounds.<kind>` set to `value` (`false`, `true` or a path string).
pub fn set_sound_text(text: &str, kind: &str, value: &Value) -> Result<String, String> {
    if !matches!(kind, "blocked" | "done") {
        return Err(format!("unknown sound {kind:?}"));
    }
    match value {
        Value::Bool(_) => {}
        Value::String(p) if !p.trim().is_empty() => {}
        _ => return Err("a sound is true, false or the path of an audio file".into()),
    }
    set_path_text(text, &["sounds", kind], &value.to_string())
}

/// Writes `sounds.<kind>` below `home`, like `save_theme`.
pub fn save_sound(home: &Path, kind: &str, value: &Value) -> Result<(), String> {
    save_with(home, |text| set_sound_text(text, kind, value))
}

/// Writes `agents.claude.classicRenderer` below `home`, like `save_theme`.
pub fn save_claude_classic_renderer(home: &Path, on: bool) -> Result<(), String> {
    save_with(home, |text| set_claude_classic_renderer_text(text, on))
}

/// Writes `explorer.showHidden` below `home`, like `save_theme`.
pub fn save_explorer_show_hidden(home: &Path, show: bool) -> Result<(), String> {
    save_with(home, |text| set_explorer_show_hidden_text(text, show))
}

/// Writes `spaces.layout` below `home`, like `save_theme`.
pub fn save_spaces_layout(home: &Path, layout: &Value) -> Result<(), String> {
    save_with(home, |text| set_spaces_layout_text(text, layout))
}

/// Writes `spaceTemplates` below `home`, like `save_theme`.
pub fn save_space_templates(home: &Path, templates: &Value) -> Result<(), String> {
    save_with(home, |text| set_space_templates_text(text, templates))
}

/// Whether the config text has `spaces.layout` or `spaceTemplates` (any value): the first-start
/// defaults are never written over them.
fn has_seeded_keys(text: &str) -> Result<bool, String> {
    let root = root_open(text)?;
    if find_value(text, root, "spaceTemplates").is_some() {
        return Ok(true);
    }
    Ok(match find_value(text, root, "spaces") {
        Some((start, _)) if text.as_bytes().get(start) == Some(&b'{') => {
            find_value(text, start, "layout").is_some()
        }
        _ => false,
    })
}

/// Writes the first-start defaults (`spaces.layout`, `spaceTemplates`) below `home` when the config
/// has neither key (a missing file counts). Other keys and comments stay. `Ok(true)` when written.
pub fn seed_defaults(home: &Path, layout: &Value, templates: &Value) -> Result<bool, String> {
    let written = std::cell::Cell::new(false);
    save_with(home, |text| {
        if has_seeded_keys(text)? {
            return Ok(text.to_owned());
        }
        written.set(true);
        set_spaces_layout_text(text, layout)
            .and_then(|next| set_space_templates_text(&next, templates))
    })?;
    Ok(written.get())
}

/// Removes `spaces.layout` below `home`; a missing file stays missing.
pub fn remove_spaces_layout(home: &Path) -> Result<(), String> {
    if !config_path(home).exists() {
        return Ok(());
    }
    save_with(home, remove_spaces_layout_text)
}

fn stamp(home: &Path) -> [Option<SystemTime>; 2] {
    [config_path(home), ghostty_path(home)]
        .map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
}

/// Runs forever on the calling thread: polls the config files (poimen, Ghostty) and calls `emit` on every change.
pub fn watch(home: &Path, interval: Duration, emit: impl Fn(&Settings)) {
    let mut last = stamp(home);
    loop {
        std::thread::sleep(interval);
        let now = stamp(home);
        if now != last {
            last = now;
            emit(&load(home));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::CommandSpec;

    fn none() -> Option<String> {
        None
    }

    fn rows(json: &str) -> Settings {
        resolve(Some(json), none)
    }

    #[test]
    fn machines_load_and_an_invalid_one_is_a_warning() {
        let s = rows(
            r#"{"machines":[{"name":"box","ssh":"box"},{"name":"bad","ssh":"-x"}],
                "theme":"dark"}"#,
        );
        assert_eq!(s.theme, "dark");
        let names: Vec<&str> = s.machines.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["box"]);
        assert_eq!(s.machines[0].repos_dir, "~/Source/{owner}/{repo}");
        assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
        assert!(
            s.warnings[0].starts_with("machines[1]:"),
            "{}",
            s.warnings[0]
        );
        let wire = serde_json::to_value(&s).unwrap();
        assert_eq!(wire["machines"][0]["reposDir"], "~/Source/{owner}/{repo}");
    }

    #[test]
    fn space_templates_parse_and_invalid_entries_warn_and_are_skipped() {
        let s = rows(
            r#"{"spaceTemplates":[
            {"name":"a","match":{"dir":"~/src/a"},"agent":"claude","model":"opus",
             "layout":{"panes":[{"name":"main"}]}},
            {"name":"a","match":{"dir":"/x"},"agent":"claude"},
            {"match":{"dir":"/x"},"agent":"claude"},
            {"name":"rel","match":{"dir":"src/a"},"agent":"claude"},
            {"name":"noagent","match":{"dir":"/x"}},
            {"name":"nodir","agent":"claude"},
            {"name":"badmodel","match":{"dir":"/x"},"agent":"claude","model":3},
            {"name":"badlayout","match":{"dir":"/x"},"agent":"claude","layout":{"panes":[]}},
            "text"]}"#,
        );
        // `nodir` has no `match`: it is the default template.
        assert_eq!(s.space_templates.len(), 2);
        let t = &s.space_templates[0];
        assert_eq!((t.name.as_str(), t.dir.as_deref()), ("a", Some("~/src/a")));
        let d = &s.space_templates[1];
        assert_eq!(
            (d.name.as_str(), d.dir.is_none(), d.remote.is_none()),
            ("nodir", true, true)
        );
        assert_eq!(t.model.as_deref(), Some("opus"));
        assert_eq!(t.layout.as_ref().unwrap().panes[0].name, "main");
        assert_eq!(s.warnings.len(), 7, "{:?}", s.warnings);
        assert!(s.warnings[0].contains("duplicate"));
        assert_eq!(rows("{\"spaceTemplates\":1}").space_templates.len(), 0);
        assert!(resolve(None, none).space_templates.is_empty());
    }

    #[test]
    fn space_template_remote_match_parses_and_validates() {
        use poimen_core::remote::Remote;
        let s = rows(
            r#"{"spaceTemplates":[
            {"name":"gh","match":{"remote":{"provider":"github","owner":"GravionLabs"}},"agent":"claude"},
            {"name":"az","match":{"dir":"~/w","remote":{"provider":"azure-devops","organization":"acme","project":"FLEX Business"}},"agent":"claude"},
            {"name":"noowner","match":{"remote":{"provider":"github"}},"agent":"claude"},
            {"name":"noproject","match":{"remote":{"provider":"azure-devops","organization":"o"}},"agent":"claude"},
            {"name":"prov","match":{"remote":{"provider":"gitlab","owner":"o"}},"agent":"claude"},
            {"name":"notobj","match":{"remote":"github"},"agent":"claude"},
            {"name":"empty","match":{},"agent":"claude"}]}"#,
        );
        // `empty` (`match: {}`) is the default template.
        assert_eq!(s.space_templates.len(), 3, "{:?}", s.warnings);
        assert_eq!(s.space_templates[2].name, "empty");
        let (gh, az) = (&s.space_templates[0], &s.space_templates[1]);
        assert_eq!(gh.dir, None);
        assert_eq!(
            gh.remote,
            Some(Remote::GitHub {
                owner: "GravionLabs".into()
            })
        );
        assert_eq!(az.dir.as_deref(), Some("~/w"));
        assert_eq!(
            az.remote,
            Some(Remote::AzureDevOps {
                organization: "acme".into(),
                project: "FLEX Business".into()
            })
        );
        assert_eq!(s.warnings.len(), 4, "{:?}", s.warnings);
        assert!(s.warnings[0].contains("owner"));
    }

    #[test]
    fn only_one_default_space_template_is_kept() {
        let s = rows(
            r#"{"spaceTemplates":[
            {"name":"one","agent":"claude"},
            {"name":"two","match":{},"agent":"copilot"}]}"#,
        );
        assert_eq!(s.space_templates.len(), 1);
        assert_eq!(s.space_templates[0].name, "one");
        assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
        assert!(s.warnings[0].contains("default template"));
    }

    #[test]
    fn claude_classic_renderer_defaults_off_and_parses_tolerantly() {
        assert!(!resolve(None, none).claude_classic_renderer);
        let on = rows(r#"{"agents":{"claude":{"classicRenderer":true}}}"#);
        assert!(on.claude_classic_renderer);
        assert!(on.warnings.is_empty());
        for bad in [
            r#"{"agents":{"claude":{"classicRenderer":"yes"}}}"#,
            r#"{"agents":{"claude":3}}"#,
            r#"{"agents":[]}"#,
        ] {
            let s = rows(bad);
            assert!(!s.claude_classic_renderer, "{bad}");
            assert_eq!(s.warnings.len(), 1, "{bad}: {:?}", s.warnings);
        }
        assert!(rows(r#"{"agents":{}}"#).warnings.is_empty());
    }

    #[test]
    fn writing_the_classic_renderer_keeps_the_rest_of_the_file() {
        let out = set_claude_classic_renderer_text("{\"theme\": \"dark\"}", true).unwrap();
        let s = rows(&out);
        assert!(s.claude_classic_renderer);
        assert_eq!(s.theme, "dark");
        let commented = "{\n  // mine\n  \"theme\": \"dark\"\n}";
        let out = set_claude_classic_renderer_text(commented, true).unwrap();
        assert!(out.contains("// mine"), "{out}");
    }

    #[test]
    fn explorer_settings_default_and_override() {
        let d = resolve(None, none);
        assert_eq!(d.explorer_roots, ["~"]);
        assert!(!d.explorer_show_hidden);
        let s = rows(r#"{"explorer":{"roots":["/srv","~/code"],"showHidden":true}}"#);
        assert_eq!(s.explorer_roots, ["/srv", "~/code"]);
        assert!(s.explorer_show_hidden);
        assert!(s.warnings.is_empty());
    }

    #[test]
    fn github_agent_actions_resolve_with_warnings() {
        let d = rows("{}");
        assert!(d.agent_actions.is_empty());
        let s = rows(
            r#"{"harnesses":{"x":{"command":["x"]}},"github":{"agentActions":[
                {"id":"a","on":"issue","prompt":"do {nr}","harness":"x"},
                {"id":"b","on":"issue","prompt":"do","harness":"nope"},
                {"id":"c","on":"issue"}
            ]}}"#,
        );
        assert_eq!(s.agent_actions.len(), 1);
        assert_eq!(s.agent_actions[0].id, "a");
        assert_eq!(s.warnings.len(), 2, "{:?}", s.warnings);
        assert!(rows(r#"{"github":3}"#).warnings[0].starts_with("github must"));
    }

    #[test]
    fn harnesses_default_and_resolve_from_config() {
        let d = resolve(None, none);
        assert_eq!(d.harnesses.len(), 2);
        let s = rows(r#"{"harnesses":{"x":{"command":["x"]},"bad":{"command":[]}}}"#);
        assert_eq!(s.harnesses.len(), 3);
        assert_eq!(s.harnesses[2].id, "x");
        assert_eq!(s.warnings.len(), 1);
        assert!(s.warnings[0].starts_with("harnesses.bad:"));
    }

    #[test]
    fn tasks_default_to_one_agent_mode() {
        let s = resolve(None, || None);
        assert_eq!(s.tasks.modes.len(), 1);
        let m = &s.tasks.modes[0];
        assert_eq!(m.name, "agent");
        assert!(m.worktree);
        assert_eq!(
            (&m.harness, &m.model, &m.prompt, &m.command),
            (&None, &None, &None, &None)
        );
        assert_eq!(m.branch, "{type}/{nr}-{slug}");
        assert_eq!(m.cleanup, None);
        assert_eq!(s.tasks.worktree_dir, "../{repo}-{nr}");
        assert_eq!(resolve(Some(r#"{"tasks":{}}"#), || None).tasks, s.tasks);
    }

    #[test]
    fn pr_is_a_placeholder_of_cleanup_only() {
        let s = resolve(
            Some(
                r#"{"tasks":{"modes":[
                {"name":"a","command":["x"],"cleanup":["Remove-PbiWorktrees.ps1","-Pr","{pr}","-Issue","{nr}"]},
                {"name":"b","command":["run","{pr}"]}]}}"#,
            ),
            || None,
        );
        assert_eq!(s.warnings.len(), 1, "{:?}", s.warnings);
        assert!(s.warnings[0].contains("command: unknown placeholder {pr}"));
        assert_eq!(s.tasks.modes.len(), 1);
        assert_eq!(
            s.tasks.modes[0].cleanup,
            Some(CommandSpec::Argv(
                ["Remove-PbiWorktrees.ps1", "-Pr", "{pr}", "-Issue", "{nr}"]
                    .map(String::from)
                    .to_vec()
            ))
        );
    }

    #[test]
    fn tasks_parse_modes_and_worktree_dir() {
        let s = resolve(
            Some(
                r#"{"tasks":{"worktreeDir":"../wt/{nr}","modes":[
                {"name":"agent","harness":"copilot","model":"auto","prompt":"Fix #{nr}"},
                {"name":"quick","worktree":false,"command":["vim","{url}"],"branch":"x/{nr}","cleanup":["echo","{branch}"]}]}}"#,
            ),
            || None,
        );
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let a = &s.tasks.modes[0];
        assert_eq!(a.harness.as_deref(), Some("copilot"));
        assert_eq!(a.model.as_deref(), Some("auto"));
        assert_eq!(a.prompt.as_deref(), Some("Fix #{nr}"));
        assert_eq!(a.command, None);
        assert_eq!(s.tasks.worktree_dir, "../wt/{nr}");
        assert_eq!(s.tasks.modes.len(), 2);
        let q = &s.tasks.modes[1];
        assert!(!q.worktree);
        assert_eq!(q.branch, "x/{nr}");
        assert_eq!(
            q.command,
            Some(CommandSpec::Argv(vec!["vim".into(), "{url}".into()]))
        );
        assert_eq!(
            q.cleanup,
            Some(CommandSpec::Argv(vec!["echo".into(), "{branch}".into()]))
        );
        assert_eq!(s.tasks.modes[0].branch, "{type}/{nr}-{slug}");
    }

    #[test]
    fn tasks_invalid_entries_warn_and_the_rest_loads() {
        let s = resolve(
            Some(
                r#"{"theme":"dark","tasks":{"modes":[
                {"name":"a","command":["run"]},
                {"name":"a","command":["again"]},
                {"name":"bad","command":["run","{nope}"]},
                {"command":["x"]},
                {"name":"b","command":["ok"],"branch":"{prompt}"},
                {"name":"mixed","command":["ok"],"harness":"claude"},
                {"name":"empty","command":[]},
                {"name":"badprompt","prompt":"{prompt}"},
                {"name":"c","command":["ok"],"worktree":"yes"}]}}"#,
            ),
            || None,
        );
        assert_eq!(s.theme, "dark");
        let names: Vec<_> = s.tasks.modes.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["a", "c"]);
        assert_eq!(
            s.tasks.modes[0].command,
            Some(CommandSpec::Argv(vec!["run".into()]))
        );
        assert_eq!(s.warnings.len(), 8, "{:?}", s.warnings);
        assert!(s.warnings.iter().any(|w| w.contains("cannot be combined")));
        assert!(
            s.warnings
                .iter()
                .any(|w| w.contains("tasks.modes[badprompt].prompt"))
        );
        assert!(s.warnings.iter().any(|w| w.contains("duplicate")));
        assert!(s.warnings.iter().any(|w| w.contains("{nope}")));
        assert!(s.warnings.iter().any(|w| w.contains("{prompt}")));
    }

    #[test]
    fn a_string_command_and_cleanup_still_load_with_a_deprecation_warning_naming_the_mode() {
        let s = resolve(
            Some(
                r#"{"tasks":{"modes":[
                {"name":"old","command":"claude {prompt}","cleanup":"git branch -D {branch}"},
                {"name":"new","command":["claude","{prompt}"]}]}}"#,
            ),
            || None,
        );
        assert_eq!(
            s.warnings,
            [
                "tasks.modes.old: string command is deprecated, use harness + prompt or an argv list",
                "tasks.modes.old: string cleanup is deprecated, use an argv list"
            ]
        );
        assert_eq!(
            s.tasks.modes[0].command,
            Some(CommandSpec::Shell("claude {prompt}".into()))
        );
        assert_eq!(s.tasks.modes.len(), 2);
    }

    #[test]
    fn tasks_without_a_valid_mode_keep_the_default() {
        let s = resolve(
            Some(r#"{"tasks":{"worktreeDir":"../{bad}","modes":[{"command":["x"]}]}}"#),
            || None,
        );
        assert_eq!(s.tasks, crate::tasks::TaskModes::default());
        assert_eq!(s.warnings.len(), 3, "{:?}", s.warnings);
        assert_eq!(resolve(Some(r#"{"tasks":3}"#), || None).warnings.len(), 1);
    }

    #[test]
    fn spaces_layout_parses_and_invalid_ones_warn_and_fall_back() {
        let ok = resolve(
            Some(r#"{"spaces":{"layout":{"panes":[{"name":"main","command":"vim"}]}}}"#),
            || None,
        );
        assert!(ok.warnings.is_empty(), "{:?}", ok.warnings);
        assert_eq!(ok.spaces_layout.unwrap().panes[0].name, "main");
        for json in [
            r#"{"spaces":{"layout":{"panes":[]}}}"#,
            r#"{"spaces":{"layout":"x"}}"#,
            r#"{"spaces":3}"#,
        ] {
            let s = resolve(Some(json), || None);
            assert_eq!(s.warnings.len(), 1, "{json}");
            assert_eq!(s.spaces_layout, None, "{json}");
        }
        assert_eq!(resolve(None, || None).spaces_layout, None);
    }

    #[test]
    fn scrollback_defaults_to_10000_and_validates() {
        assert_eq!(rows("{}").scrollback, 10_000);
        assert_eq!(rows(r#"{"terminal":{"scrollback":500}}"#).scrollback, 500);
        assert_eq!(rows(r#"{"terminal":{"scrollback":0}}"#).scrollback, 0);
        for json in [
            r#"{"terminal":{"scrollback":-1}}"#,
            r#"{"terminal":{"scrollback":"many"}}"#,
            r#"{"terminal":{"scrollback":2000000}}"#,
            r#"{"terminal":{"scrollback":1.5}}"#,
        ] {
            let s = rows(json);
            assert_eq!(s.scrollback, 10_000, "{json}");
            assert!(
                s.warnings.iter().any(|w| w.contains("terminal.scrollback")),
                "{json}"
            );
        }
    }

    #[test]
    fn font_setters_keep_comments_and_other_keys() {
        let text = "{\n  // mine\n  \"theme\": \"dark\",\n  \"terminal\": { \"scrollback\": 5 }\n}";
        let out = set_terminal_font_size_text(text, 15.0).unwrap();
        assert!(out.contains("// mine") && out.contains("\"scrollback\": 5"));
        let out = set_terminal_font_family_text(&out, "Fira Code").unwrap();
        let out = set_ui_font_family_text(&out, "Inter, sans-serif").unwrap();
        let out = set_ui_font_size_text(&out, 14.0).unwrap();
        assert!(out.contains("// mine"));
        let s = resolve(Some(&out.replace("// mine", "")), || None);
        assert_eq!(s.font_family.as_deref(), Some("Fira Code"));
        assert_eq!(s.font_size, 15.0);
        assert_eq!(s.ui_font_family.as_deref(), Some("Inter, sans-serif"));
        assert_eq!(s.ui_font_size, 14.0);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    }

    #[test]
    fn empty_font_family_removes_the_key() {
        let text =
            r#"{"terminal":{"fontFamily":"Fira Code","fontSize":15},"ui":{"fontFamily":"Inter"}}"#;
        let out = set_terminal_font_family_text(text, "  ").unwrap();
        let out = set_ui_font_family_text(&out, "").unwrap();
        let s = resolve(Some(&out), || None);
        assert_eq!((s.font_family, s.font_size), (None, 15.0));
        assert_eq!(s.ui_font_family, None);
        assert!(!out.contains("fontFamily"));
    }

    #[test]
    fn clearing_the_terminal_font_size_removes_only_that_key() {
        let text = r#"{"terminal":{"fontFamily":"Fira Code","fontSize":15},"ui":{"fontSize":14}}"#;
        let out = clear_terminal_font_size_text(text).unwrap();
        assert!(!out.contains("\"fontSize\":15"), "{out}");
        let s = resolve(Some(&out), || None);
        assert_eq!(s.font_family.as_deref(), Some("Fira Code"));
        assert_eq!(s.ui_font_size, 14.0);
        // Absent key: nothing to remove, still valid.
        assert!(clear_terminal_font_size_text("{}").is_ok());
    }

    #[test]
    fn font_sizes_out_of_range_are_rejected() {
        assert!(set_terminal_font_size_text("{}", 5.0).is_err());
        assert!(set_terminal_font_size_text("{}", 73.0).is_err());
        assert!(set_terminal_font_size_text("{}", f64::NAN).is_err());
        assert!(set_ui_font_size_text("{}", 9.0).is_err());
        assert!(set_ui_font_size_text("{}", 21.0).is_err());
        assert!(set_ui_font_size_text("{}", 10.0).is_ok());
        assert!(set_ui_font_size_text("{}", 20.0).is_ok());
    }

    #[test]
    fn ui_settings_validate_with_warnings() {
        let d = Settings::default();
        assert_eq!((d.ui_font_family.clone(), d.ui_font_size), (None, 13.0));
        let s = resolve(Some(r#"{"ui":{"fontFamily":3,"fontSize":99}}"#), || None);
        assert_eq!((s.ui_font_family, s.ui_font_size), (None, 13.0));
        assert_eq!(s.warnings.len(), 2, "{:?}", s.warnings);
        let s = resolve(
            Some(r#"{"ui":{"fontFamily":null,"fontSize":12.5}}"#),
            || None,
        );
        assert_eq!((s.ui_font_family, s.ui_font_size), (None, 12.5));
        assert!(s.warnings.is_empty());
        assert_eq!(resolve(Some(r#"{"ui":3}"#), || None).warnings.len(), 1);
    }

    #[test]
    fn a_leftover_notifications_key_is_ignored_without_a_warning() {
        // System notifications were removed (#676); an old config keeps working silently.
        let s = rows(r#"{"notifications":{"blocked":false,"done":"no"}}"#);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
    }

    #[test]
    fn sounds_default_to_off_and_parse_true_false_and_paths() {
        assert_eq!(rows("{}").sounds, Sounds::default());
        let s = rows(r#"{"sounds":{"blocked":true,"done":"~/ding.wav"}}"#);
        assert_eq!(s.sounds.blocked, SoundSetting::Builtin);
        assert_eq!(s.sounds.done, SoundSetting::File("~/ding.wav".into()));
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let json = serde_json::to_value(&s.sounds).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"blocked": true, "done": "~/ding.wav"})
        );
    }

    #[test]
    fn other_sound_values_warn_and_stay_off() {
        let s = rows(r#"{"sounds":{"blocked":3,"done":""}}"#);
        assert_eq!(s.sounds, Sounds::default());
        assert_eq!(s.warnings.len(), 2, "{:?}", s.warnings);
        assert_eq!(rows(r#"{"sounds":true}"#).warnings.len(), 1);
    }

    #[test]
    fn set_sound_text_writes_one_key_and_refuses_other_values() {
        let out = set_sound_text("{\"theme\":\"dark\"}", "done", &Value::Bool(true)).unwrap();
        assert!(
            out.contains("\"done\": true") || out.contains("\"done\":true"),
            "{out}"
        );
        assert!(set_sound_text("{}", "other", &Value::Bool(true)).is_err());
        assert!(set_sound_text("{}", "done", &Value::from(3)).is_err());
    }

    fn sound_home(name: &str, config: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("poimen-sound-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
        std::fs::write(config_path(&home), config).unwrap();
        home
    }

    #[test]
    fn sound_file_reads_the_configured_file() {
        let home = sound_home("ok", r#"{"sounds":{"done":"~/ding.wav"}}"#);
        std::fs::write(home.join("ding.wav"), [1u8, 2, 3]).unwrap();
        assert_eq!(read_sound_file(&home, "done").unwrap(), vec![1, 2, 3]);
        // `blocked` is off: the caller cannot name a path, only a kind.
        assert!(read_sound_file(&home, "blocked").is_err());
        assert!(read_sound_file(&home, "/etc/passwd").is_err());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn sound_file_refuses_missing_large_and_directory() {
        let home = sound_home(
            "bad",
            r#"{"sounds":{"blocked":"~/none.wav","done":"~/dir"}}"#,
        );
        assert!(read_sound_file(&home, "blocked").is_err());
        std::fs::create_dir(home.join("dir")).unwrap();
        assert!(
            read_sound_file(&home, "done")
                .unwrap_err()
                .contains("regular")
        );
        std::fs::write(
            home.join("none.wav"),
            vec![0u8; MAX_SOUND_BYTES as usize + 1],
        )
        .unwrap();
        assert!(
            read_sound_file(&home, "blocked")
                .unwrap_err()
                .contains("1 MiB")
        );
        std::fs::write(home.join("none.wav"), vec![0u8; MAX_SOUND_BYTES as usize]).unwrap();
        assert!(read_sound_file(&home, "blocked").is_ok());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn explorer_settings_invalid_warn_and_keep_defaults() {
        for json in [
            r#"{"explorer":{"roots":[]}}"#,
            r#"{"explorer":{"roots":["relative"]}}"#,
            r#"{"explorer":{"roots":"/srv"}}"#,
            r#"{"explorer":"x"}"#,
        ] {
            let s = rows(json);
            assert_eq!(s.explorer_roots, ["~"], "{json}");
            assert_eq!(s.warnings.len(), 1, "{json}");
        }
        assert_eq!(rows(r#"{"explorer":{"showHidden":1}}"#).warnings.len(), 1);
    }

    #[test]
    fn sidebar_rows_accept_names_objects_and_herdr_tokens() {
        let s = rows(
            r#"{"sidebar":{"rows":{"agent":[["state_icon",{"token":"name","bold":true}],["$deliver_step","dir"]]}}}"#,
        );
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let tok = |t: &str, bold| poimen_core::settings::RowToken {
            token: t.into(),
            bold,
            dim: false,
        };
        assert_eq!(
            s.sidebar_rows.agent,
            Some(vec![
                vec![tok("state_icon", false), tok("name", true)],
                vec![tok("$deliver_step", false), tok("dir", false)],
            ])
        );
        assert_eq!(s.sidebar_rows.workspace, None);
    }

    #[test]
    fn sidebar_rows_accept_every_builtin_token_including_notification() {
        let s =
            rows(r#"{"sidebar":{"rows":{"agent":[["state_icon","name"],["notification","pr"]]}}}"#);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let tokens = s.sidebar_rows.agent.expect("row kept");
        assert_eq!(tokens[1][0].token, "notification");
    }

    #[test]
    fn sidebar_sort_reaches_the_settings() {
        assert_eq!(rows("{}").sidebar_sort, SidebarSort::Manual);
        assert_eq!(
            rows(r#"{"sidebar":{"sort":"name"}}"#).sidebar_sort,
            SidebarSort::Name
        );
        assert_eq!(
            rows(r#"{"sidebar":{"sort":"attention"}}"#).sidebar_sort,
            SidebarSort::Attention
        );
        let s = rows(r#"{"sidebar":{"sort":"newest"}}"#);
        assert_eq!(s.sidebar_sort, SidebarSort::Manual);
        assert!(s.warnings[0].contains("sidebar.sort"));
    }

    #[test]
    fn group_worktrees_reaches_the_settings() {
        assert!(rows("{}").sidebar_group_worktrees);
        assert!(!rows(r#"{"sidebar":{"groupWorktrees":false}}"#).sidebar_group_worktrees);
    }

    #[test]
    fn set_sidebar_sort_keeps_comments_and_other_keys() {
        let text = "{\n  // mine\n  \"sidebar\": { \"layout\": \"split\" }\n}";
        let out = set_sidebar_sort_text(text, "name").unwrap();
        assert!(out.contains("// mine") && out.contains("\"layout\": \"split\""));
        assert!(out.contains("\"sort\": \"name\"") || out.contains("\"sort\":\"name\""));
        let again = set_sidebar_sort_text(&out, "manual").unwrap();
        assert!(again.contains("\"manual\"") && !again.contains("\"name\""));
        assert!(set_sidebar_sort_text("{}", "newest").is_err());
        assert!(
            set_sidebar_sort_text("{}", "attention")
                .unwrap()
                .contains("\"attention\"")
        );
        assert!(set_sidebar_sort_text("{\"sidebar\":3}", "name").is_err());
    }

    #[test]
    fn sidebar_agents_reaches_the_settings_and_is_written() {
        assert_eq!(rows("{}").sidebar_agents, "list");
        assert_eq!(
            rows(r#"{"sidebar":{"agents":"priority"}}"#).sidebar_agents,
            "priority"
        );
        let s = rows(r#"{"sidebar":{"agents":"newest"}}"#);
        assert_eq!(s.sidebar_agents, "list");
        assert!(s.warnings[0].contains("sidebar.agents"));
        let text = "{\n  // mine\n  \"sidebar\": { \"layout\": \"split\" }\n}";
        let out = set_sidebar_agents_text(text, "priority").unwrap();
        assert!(out.contains("// mine") && out.contains("\"layout\": \"split\""));
        assert!(
            out.contains("\"agents\": \"priority\"") || out.contains("\"agents\":\"priority\"")
        );
        assert!(set_sidebar_agents_text("{}", "newest").is_err());
        assert!(set_sidebar_agents_text("{\"sidebar\":3}", "list").is_err());
        let home = std::env::temp_dir().join(format!("poimen-agents-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        save_sidebar_agents(&home, "priority").unwrap();
        assert_eq!(load(&home).sidebar_agents, "priority");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn save_sidebar_sort_round_trips() {
        let home = std::env::temp_dir().join(format!("poimen-sort-{}", std::process::id()));
        std::fs::create_dir_all(&home).unwrap();
        save_sidebar_sort(&home, "name").unwrap();
        assert_eq!(load(&home).sidebar_sort, SidebarSort::Name);
        save_sidebar_sort(&home, "manual").unwrap();
        assert_eq!(load(&home).sidebar_sort, SidebarSort::Manual);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn sidebar_layout_parses_and_invalid_warns() {
        assert_eq!(rows("{}").sidebar_layout, "tree");
        assert_eq!(
            rows(r#"{"sidebar":{"layout":"split"}}"#).sidebar_layout,
            "split"
        );
        let s = rows(r#"{"sidebar":{"layout":"grid"}}"#);
        assert_eq!(s.sidebar_layout, "tree");
        assert!(s.warnings[0].contains("sidebar.layout"));
    }

    #[test]
    fn show_base_branch_parses_and_invalid_warns() {
        assert!(!rows("{}").sidebar_show_base_branch);
        assert!(rows(r#"{"sidebar":{"showBaseBranch":true}}"#).sidebar_show_base_branch);
        let s = rows(r#"{"sidebar":{"showBaseBranch":"yes"}}"#);
        assert!(!s.sidebar_show_base_branch);
        assert!(s.warnings[0].contains("sidebar.showBaseBranch"));
    }

    #[test]
    fn set_sidebar_layout_replaces_inserts_and_keeps_the_rest() {
        let text = "{\n  // c\n  \"theme\": \"dark\",\n  \"sidebar\": { \"layout\": \"tree\", \"rows\": {} }\n}";
        let out = set_sidebar_layout_text(text, "split").unwrap();
        assert_eq!(out, text.replace("\"tree\"", "\"split\""));
        let into = set_sidebar_layout_text("{\"sidebar\":{\"rows\":{}}}", "split").unwrap();
        assert_eq!(
            into,
            "{\"sidebar\":{\n  \"layout\": \"split\",\"rows\":{}}}"
        );
        let empty = set_sidebar_layout_text("{\"sidebar\":{}}", "split").unwrap();
        assert_eq!(empty, "{\"sidebar\":{\"layout\": \"split\"}}");
        let none = set_sidebar_layout_text("{\"theme\":\"dark\"}", "split").unwrap();
        assert_eq!(
            none,
            "{\n  \"sidebar\": { \"layout\": \"split\" },\"theme\":\"dark\"}"
        );
        let nested = set_sidebar_layout_text("{\"x\":{\"layout\":\"tree\"}}", "split").unwrap();
        assert!(nested.contains("\"x\":{\"layout\":\"tree\"}"));
    }

    #[test]
    fn set_path_creates_and_descends_nested_objects() {
        let text = "{\n  // keep\n  \"explorer\": { \"showHidden\": true }\n}";
        let out = set_explorer_roots_text(text, &["~/Source".into()]).unwrap();
        assert_eq!(
            out,
            "{\n  // keep\n  \"explorer\": {\n  \"roots\": [\"~/Source\"], \"showHidden\": true }\n}"
        );
        let replaced = set_explorer_roots_text(&out, &["~".into(), "/srv".into()]).unwrap();
        assert!(replaced.contains("\"roots\": [\"~\",\"/srv\"], \"showHidden\": true"));
        assert!(replaced.contains("// keep"));
        let created = set_explorer_show_hidden_text("{\"theme\":\"dark\"}", true).unwrap();
        assert_eq!(
            created,
            "{\n  \"explorer\": { \"showHidden\": true },\"theme\":\"dark\"}"
        );
        let plain = "{\"explorer\": { \"roots\": [\"~\"], \"showHidden\": true }}";
        let plain = set_explorer_roots_text(plain, &["~".into(), "/srv".into()]).unwrap();
        let r = resolve(Some(&plain), || None);
        assert_eq!(r.explorer_roots, vec!["~", "/srv"]);
        assert!(r.explorer_show_hidden);
    }

    #[test]
    fn set_explorer_roots_rejects_bad_input() {
        assert!(set_explorer_roots_text("{}", &[]).is_err());
        assert!(set_explorer_roots_text("{}", &["relative".into()]).is_err());
        assert!(set_explorer_roots_text("{\"explorer\":[1]}", &["~".into()]).is_err());
        assert!(set_explorer_show_hidden_text("[]", true).is_err());
    }

    #[test]
    fn tilde_path_shortens_paths_under_home() {
        let home = Path::new("/Users/me");
        assert_eq!(tilde_path("/Users/me", home), "~");
        assert_eq!(tilde_path("/Users/me/", home), "~");
        assert_eq!(tilde_path("/Users/me/Source/", home), "~/Source");
        assert_eq!(tilde_path("/Users/meow", home), "/Users/meow");
        assert_eq!(tilde_path("/srv/x", home), "/srv/x");
        assert_eq!(tilde_path("~/x", home), "~/x");
    }

    #[test]
    fn tilde_path_shortens_windows_paths_under_home() {
        let home = Path::new(r"C:\Users\me");
        assert_eq!(tilde_path(r"C:\Users\me", home), "~");
        assert_eq!(tilde_path(r"c:\users\ME\", home), "~");
        assert_eq!(tilde_path(r"C:\Users\me\Source\x", home), "~/Source/x");
        assert_eq!(tilde_path("C:/Users/me/Source", home), "~/Source");
        assert_eq!(tilde_path(r"C:\Users\meow", home), r"C:\Users\meow");
        assert_eq!(tilde_path(r"D:\src", home), r"D:\src");
    }

    #[test]
    fn windows_absolute_roots_are_accepted() {
        let roots = [r"C:\src".to_owned(), "D:/x".into(), r"~\y".into()];
        let text = set_explorer_roots_text("{}", &roots).unwrap();
        assert_eq!(rows(&text).explorer_roots, roots);
        assert!(set_explorer_roots_text("{}", &["C:x".into()]).is_err());
        assert!(set_explorer_roots_text("{}", &["src".into()]).is_err());
    }

    #[test]
    fn save_explorer_roots_writes_home_relative_paths() {
        let home = std::env::temp_dir().join(format!("poimen-roots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let sub = home.join("Source").to_string_lossy().into_owned();
        save_explorer_roots(&home, &[sub, "/srv".into()]).unwrap();
        save_explorer_show_hidden(&home, true).unwrap();
        let s = load(&home);
        assert_eq!(s.explorer_roots, vec!["~/Source", "/srv"]);
        assert!(s.explorer_show_hidden);
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn save_explorer_roots_drops_duplicates_after_shortening() {
        let home = std::env::temp_dir().join(format!("poimen-dedupe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let abs = home.join("Source").to_string_lossy().into_owned();
        let h = home.to_string_lossy().into_owned();
        save_explorer_roots(
            &home,
            &["~/Source".into(), abs, "~".into(), h, "/srv".into()],
        )
        .unwrap();
        assert_eq!(load(&home).explorer_roots, vec!["~/Source", "~", "/srv"]);
        assert_eq!(load(&home).home, Some(home.to_string_lossy().into_owned()));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn set_spaces_layout_validates_and_keeps_the_rest() {
        let layout = serde_json::json!({"panes": [{"name": "a", "command": "nvim"}, {"name": "b", "split": {"from": "a", "direction": "down", "ratio": 0.7}}]});
        let text = "{\n  // keep\n  \"theme\": \"dark\"\n}";
        let out = set_spaces_layout_text(text, &layout).unwrap();
        assert!(out.contains("// keep"));
        assert!(out.contains("\"theme\": \"dark\""));
        let parsed = resolve(Some(&out.replace("  // keep\n", "")), || None);
        let panes = parsed.spaces_layout.unwrap().panes;
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[1].split.as_ref().unwrap().ratio, 0.7);
        let bad = serde_json::json!({"panes": [{"name": "a"}, {"name": "a", "split": {"direction": "right"}}]});
        assert!(
            set_spaces_layout_text("{}", &bad)
                .unwrap_err()
                .contains("duplicate")
        );
        assert!(set_spaces_layout_text("{\"spaces\": 1}", &layout).is_err());
    }

    #[test]
    fn remove_spaces_layout_drops_only_the_entry() {
        let middle = "{\n  \"spaces\": {\n    \"layout\": { \"panes\": [] },\n    \"x\": 1\n  }\n}";
        assert_eq!(
            remove_spaces_layout_text(middle).unwrap(),
            "{\n  \"spaces\": {\n    \"x\": 1\n  }\n}"
        );
        let last = "{\n  \"spaces\": {\n    \"x\": 1,\n    \"layout\": { \"panes\": [] }\n  },\n  \"theme\": \"dark\"\n}";
        assert_eq!(
            remove_spaces_layout_text(last).unwrap(),
            "{\n  \"spaces\": {\n    \"x\": 1\n  },\n  \"theme\": \"dark\"\n}"
        );
        let only = "{ \"spaces\": { \"layout\": {\"panes\":[]} } }";
        assert_eq!(
            remove_spaces_layout_text(only).unwrap(),
            "{ \"spaces\": { } }"
        );
        let missing = "{ // c\n \"theme\": \"dark\" }";
        assert_eq!(remove_spaces_layout_text(missing).unwrap(), missing);
        let compact = "{\"spaces\":{\"layout\":{\"panes\":[]},\"x\":1}}";
        let out = remove_spaces_layout_text(compact).unwrap();
        assert_eq!(out, "{\"spaces\":{\"x\":1}}");
        assert!(serde_json::from_str::<Value>(&out).is_ok());
    }

    #[test]
    fn trusted_projects_resolve_and_bad_entries_warn() {
        let s = resolve(
            Some(r#"{"trustedProjects":{"/a":"abc","/b":3,"/c":""}}"#),
            || None,
        );
        assert_eq!(s.trusted_projects.len(), 1);
        assert_eq!(s.trusted_projects["/a"], "abc");
        assert_eq!(s.warnings.len(), 2);
        let s = resolve(Some(r#"{"trustedProjects":[1]}"#), || None);
        assert!(s.trusted_projects.is_empty() && s.warnings.len() == 1);
        assert!(resolve(None, || None).trusted_projects.is_empty());
    }

    #[test]
    fn trust_and_revoke_round_trip_keeping_other_keys() {
        let home = std::env::temp_dir().join(format!("poimen-trust-set-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        revoke_project(&home, "/a").unwrap();
        assert!(!config_path(&home).exists());
        std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
        std::fs::write(config_path(&home), "{\n  \"theme\": \"dark\"\n}").unwrap();
        trust_project(&home, "/a", "h1").unwrap();
        trust_project(&home, "/b", "h2").unwrap();
        trust_project(&home, "/a", "h3").unwrap();
        let s = load(&home);
        assert_eq!(s.trusted_projects["/a"], "h3");
        assert_eq!(s.trusted_projects["/b"], "h2");
        revoke_project(&home, "/a").unwrap();
        revoke_project(&home, "/unknown").unwrap();
        let s = load(&home);
        assert_eq!(s.trusted_projects.keys().collect::<Vec<_>>(), ["/b"]);
        assert_eq!(s.theme, "dark");
        let _ = std::fs::remove_dir_all(&home);
    }

    fn seed_home(name: &str) -> PathBuf {
        let home = std::env::temp_dir().join(format!("poimen-seed-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        home
    }

    fn seed_with(home: &Path, installed: &[&str]) -> Result<bool, String> {
        let (layout, templates) = crate::templates::seed_values(|p| installed.contains(&p));
        seed_defaults(home, &layout, &templates)
    }

    #[test]
    fn first_start_seeds_a_missing_config_with_the_layout_and_the_default_template() {
        let home = seed_home("missing");
        assert_eq!(seed_with(&home, &["claude", "copilot"]), Ok(true));
        let s = load(&home);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let names: Vec<_> = s
            .spaces_layout
            .as_ref()
            .unwrap()
            .panes
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(names, ["claude", "copilot", "shell"]);
        assert_eq!(s.space_templates.len(), 1);
        let t = &s.space_templates[0];
        assert_eq!((t.name.as_str(), t.agent.as_str()), ("default", "claude"));
        assert!(t.dir.is_none() && t.remote.is_none() && t.layout.is_none());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn seeding_follows_the_installed_agents() {
        let home = seed_home("copilot-only");
        assert_eq!(seed_with(&home, &["copilot"]), Ok(true));
        let s = load(&home);
        let names: Vec<_> = s
            .spaces_layout
            .as_ref()
            .unwrap()
            .panes
            .iter()
            .map(|p| p.name.clone())
            .collect();
        assert_eq!(names, ["copilot", "shell"]);
        assert_eq!(s.space_templates[0].agent, "copilot");
        let _ = std::fs::remove_dir_all(&home);

        let home = seed_home("none");
        assert_eq!(seed_with(&home, &[]), Ok(true));
        let s = load(&home);
        let names: Vec<_> = s
            .spaces_layout
            .as_ref()
            .unwrap()
            .panes
            .iter()
            .map(|p| p.name.clone())
            .collect();
        assert_eq!(names, ["shell"]);
        assert_eq!(s.space_templates.len(), 1);
        assert_eq!(s.space_templates[0].agent, "claude");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn seeding_adds_the_keys_to_an_existing_config_and_keeps_the_rest() {
        let home = seed_home("existing");
        std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
        std::fs::write(
            config_path(&home),
            "{\n  // keep me\n  \"theme\": \"dark\",\n  \"spaces\": { \"other\": 1 }\n}\n",
        )
        .unwrap();
        assert_eq!(seed_with(&home, &["claude"]), Ok(true));
        let text = std::fs::read_to_string(config_path(&home)).unwrap();
        assert!(text.contains("// keep me"), "{text}");
        assert!(text.contains("\"theme\": \"dark\""), "{text}");
        assert!(text.contains("\"other\": 1"), "{text}");
        assert!(text.contains("\"layout\""), "{text}");
        assert!(text.contains("\"spaceTemplates\""), "{text}");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_config_with_either_key_is_never_touched() {
        for body in [
            "{\"spaces\": {\"layout\": {\"panes\": [{\"name\": \"solo\"}]}}}",
            "{\"spaceTemplates\": []}",
        ] {
            let home = seed_home("keys");
            std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
            std::fs::write(config_path(&home), body).unwrap();
            assert_eq!(seed_with(&home, &["claude"]), Ok(false));
            assert_eq!(std::fs::read_to_string(config_path(&home)).unwrap(), body);
            let _ = std::fs::remove_dir_all(&home);
        }
    }

    #[test]
    fn seeding_runs_once_and_a_deleted_default_stays_deleted() {
        let home = seed_home("once");
        assert_eq!(seed_with(&home, &["claude"]), Ok(true));
        save_space_templates(&home, &serde_json::json!([])).unwrap();
        assert_eq!(seed_with(&home, &["claude"]), Ok(false));
        assert!(load(&home).space_templates.is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn an_invalid_config_is_an_error_and_is_left_alone() {
        let home = seed_home("invalid");
        std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
        std::fs::write(config_path(&home), "[1, 2]").unwrap();
        assert!(seed_with(&home, &["claude"]).is_err());
        assert_eq!(
            std::fs::read_to_string(config_path(&home)).unwrap(),
            "[1, 2]"
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn save_and_remove_spaces_layout_round_trip() {
        let home = std::env::temp_dir().join(format!("poimen-spaces-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        remove_spaces_layout(&home).unwrap();
        assert!(!config_path(&home).exists());
        let layout = serde_json::json!({"panes": [{"name": "solo", "command": "claude"}]});
        save_spaces_layout(&home, &layout).unwrap();
        assert_eq!(load(&home).spaces_layout.unwrap().panes[0].name, "solo");
        remove_spaces_layout(&home).unwrap();
        assert!(load(&home).spaces_layout.is_none());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn save_space_templates_writes_only_that_key_and_keeps_the_rest() {
        let home = std::env::temp_dir().join(format!("poimen-tpls-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(config_path(&home).parent().unwrap()).unwrap();
        std::fs::write(
            config_path(&home),
            "{\n  \"theme\": \"dark\",\n  \"spaceTemplates\": []\n}\n",
        )
        .unwrap();
        let templates = serde_json::json!([
            {"name": "work", "match": {"dir": "~/src", "remote": {"provider": "github", "owner": "acme"}},
             "agent": "claude", "model": "opus",
             "layout": {"panes": [{"name": "main", "command": "ls"}]}},
            {"name": "az", "match": {"remote": {"provider": "azure-devops", "organization": "o", "project": "p"}},
             "agent": "copilot"}
        ]);
        save_space_templates(&home, &templates).unwrap();
        let text = std::fs::read_to_string(config_path(&home)).unwrap();
        assert!(text.contains("\"theme\": \"dark\""));
        let s = load(&home);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        assert_eq!(s.theme, "dark");
        assert_eq!(s.space_templates.len(), 2);
        assert_eq!(s.space_templates[0].model.as_deref(), Some("opus"));
        save_space_templates(&home, &serde_json::json!([])).unwrap();
        assert!(load(&home).space_templates.is_empty());
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn saving_templates_keeps_a_layout_name_reference_as_a_string() {
        let t = serde_json::json!([{"name": "a", "agent": "claude", "layout": "review"}]);
        let out = set_space_templates_text("{}", &t).unwrap();
        let back: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(back["spaceTemplates"][0]["layout"], "review");
        let bad = serde_json::json!([{"name": "a", "agent": "claude", "layout": 3}]);
        assert!(set_space_templates_text("{}", &bad).is_err());
    }

    #[test]
    fn save_space_templates_creates_the_file_and_rejects_invalid_input() {
        let ok = serde_json::json!([{"name": "a", "match": {"dir": "~/a"}, "agent": "claude"}]);
        assert!(
            set_space_templates_text("{}", &ok)
                .unwrap()
                .contains("\"spaceTemplates\"")
        );
        for bad in [
            serde_json::json!([
                {"name": "a", "agent": "claude"},
                {"name": "b", "match": {}, "agent": "claude"}
            ]),
            serde_json::json!([{"name": "a", "match": {"dir": "rel"}, "agent": "claude"}]),
            serde_json::json!([{"name": "a", "match": {"dir": "~/a"}}]),
            serde_json::json!([
                {"name": "a", "match": {"dir": "~/a"}, "agent": "claude"},
                {"name": "a", "match": {"dir": "~/b"}, "agent": "claude"}
            ]),
            serde_json::json!({"name": "a"}),
        ] {
            let err = set_space_templates_text("{}", &bad).unwrap_err();
            assert!(err.starts_with("spaceTemplates: "), "{err}");
        }
    }

    #[test]
    fn set_sidebar_layout_rejects_bad_input() {
        assert!(set_sidebar_layout_text("{}", "grid").is_err());
        assert!(set_sidebar_layout_text("[1]", "tree").is_err());
        assert!(set_sidebar_layout_text("{\"sidebar\":3}", "tree").is_err());
    }

    #[test]
    fn save_sidebar_layout_writes_a_temp_file() {
        let home = std::env::temp_dir().join(format!("poimen-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        save_sidebar_layout(&home, "split").unwrap();
        assert_eq!(load(&home).sidebar_layout, "split");
        save_sidebar_layout(&home, "tree").unwrap();
        assert_eq!(load(&home).sidebar_layout, "tree");
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn sidebar_rows_unknown_token_warns_and_keeps_the_default() {
        let s =
            rows(r#"{"sidebar":{"rows":{"workspace":[["name","nope"]],"terminal":[["name"]]}}}"#);
        assert_eq!(s.sidebar_rows.workspace, None);
        assert!(s.sidebar_rows.terminal.is_some());
        assert_eq!(s.warnings.len(), 1);
        assert!(s.warnings[0].contains("sidebar.rows.workspace") && s.warnings[0].contains("nope"));
    }

    #[test]
    fn sidebar_rows_wrong_types_warn() {
        for json in [
            r#"{"sidebar":{"rows":{"agent":"name"}}}"#,
            r#"{"sidebar":{"rows":{"agent":[]}}}"#,
            r#"{"sidebar":{"rows":{"agent":[[]]}}}"#,
            r#"{"sidebar":{"rows":{"agent":[["name",5]]}}}"#,
            r#"{"sidebar":{"rows":{"agent":[[{"token":"name","bold":"yes"}]]}}}"#,
            r#"{"sidebar":{"rows":{"agent":[["$"]]}}}"#,
            r#"{"sidebar":{"rows":{"pane":[["name"]]}}}"#,
            r#"{"sidebar":{"rows":[]}}"#,
            r#"{"sidebar":3}"#,
        ] {
            let s = rows(json);
            assert_eq!(s.sidebar_rows, SidebarRows::default(), "{json}");
            assert_eq!(s.warnings.len(), 1, "{json}");
        }
    }

    #[test]
    fn missing_file_gives_defaults() {
        assert_eq!(resolve(None, none), Settings::default());
    }

    #[test]
    fn reads_valid_entries() {
        let s = resolve(
            Some(r#"{"theme":"dark","terminal":{"fontFamily":"Fira Code","fontSize":15}}"#),
            none,
        );
        assert_eq!(s.theme, "dark");
        assert_eq!(s.font_family.as_deref(), Some("Fira Code"));
        assert_eq!(s.font_size, 15.0);
        assert!(s.warnings.is_empty());
    }

    #[test]
    fn color_schema_passes_a_name_through() {
        assert_eq!(resolve(None, none).color_schema, None);
        let s = resolve(Some(r#"{"colorSchema":" dracula "}"#), none);
        assert_eq!(s.color_schema, Some(ColorSchema::Name("dracula".into())));
        assert!(s.warnings.is_empty());
    }

    #[test]
    fn color_schema_accepts_a_light_dark_pair() {
        let s = resolve(
            Some(r#"{"colorSchema":{"light":" one-light ","dark":"dracula"}}"#),
            none,
        );
        assert_eq!(
            s.color_schema,
            Some(ColorSchema::Pair {
                light: "one-light".into(),
                dark: "dracula".into()
            })
        );
        assert!(s.warnings.is_empty());
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(
            json["colorSchema"],
            serde_json::json!({"light": "one-light", "dark": "dracula"})
        );
    }

    #[test]
    fn color_schema_pair_needs_two_names() {
        for bad in [
            r#"{"light":"a"}"#,
            r#"{"light":"a","dark":3}"#,
            r#"{"light":"","dark":"b"}"#,
            "{}",
        ] {
            let s = resolve(Some(&format!(r#"{{"colorSchema":{bad}}}"#)), none);
            assert_eq!(s.color_schema, None, "{bad}");
            assert_eq!(s.warnings.len(), 1, "{bad}");
        }
    }

    #[test]
    fn color_schema_that_is_not_a_name_warns() {
        for bad in ["3", "\"\"", "null", "[\"a\"]"] {
            let s = resolve(Some(&format!(r#"{{"colorSchema":{bad}}}"#)), none);
            assert_eq!(s.color_schema, None, "{bad}");
            assert_eq!(s.warnings.len(), 1, "{bad}");
        }
    }

    #[test]
    fn auto_control_defaults_to_true_and_can_be_turned_off() {
        assert!(resolve(None, none).auto_control);
        assert!(resolve(Some(r#"{"terminal":{}}"#), none).auto_control);
        let off = resolve(Some(r#"{"terminal":{"autoControl":false}}"#), none);
        assert!(!off.auto_control);
        assert!(off.warnings.is_empty());
        let bad = resolve(Some(r#"{"terminal":{"autoControl":"no"}}"#), none);
        assert!(bad.auto_control);
        assert_eq!(bad.warnings.len(), 1);
    }

    #[test]
    fn invalid_entries_warn_and_fall_back() {
        let s = resolve(
            Some(r#"{"theme":"blue","terminal":{"fontFamily":3,"fontSize":500,"ghostty":"yes"}}"#),
            none,
        );
        assert_eq!(s.theme, "system");
        assert_eq!(s.font_family, None);
        assert_eq!(s.font_size, DEFAULT_FONT_SIZE);
        assert_eq!(s.warnings.len(), 4);
    }

    #[test]
    fn broken_json_warns() {
        for bad in ["{nope", "[1]"] {
            let s = resolve(Some(bad), none);
            assert_eq!(s.theme, "system");
            assert_eq!(s.warnings.len(), 1);
        }
    }

    #[test]
    fn parses_ghostty_config() {
        let g = parse_ghostty(
            "# comment\nfont-family = \"JetBrains Mono\"\nfont-family = Menlo\nfont-size = 14\nbackground = #1e1e2e\nforeground = cdd6f4\ncursor-color = #f5e0dc\nselection-background = #585b70\nselection-foreground = #cdd6f4\npalette = 1=#f38ba8\npalette = 15 = \"a6adc8\"\npalette = 16=#000000\npalette = 2=red\nbad line\n",
        );
        assert_eq!(g.font_family.as_deref(), Some("JetBrains Mono"));
        assert_eq!(g.font_size, Some(14.0));
        assert_eq!(g.background.as_deref(), Some("#1e1e2e"));
        assert_eq!(g.foreground.as_deref(), Some("#cdd6f4"));
        assert_eq!(g.cursor.as_deref(), Some("#f5e0dc"));
        assert_eq!(g.selection_background.as_deref(), Some("#585b70"));
        assert_eq!(g.selection_foreground.as_deref(), Some("#cdd6f4"));
        assert_eq!(g.palette.len(), 16);
        assert_eq!(g.palette[1].as_deref(), Some("#f38ba8"));
        assert_eq!(g.palette[15].as_deref(), Some("#a6adc8"));
        assert_eq!(g.palette.iter().flatten().count(), 2);
        assert_eq!(
            parse_ghostty("background = red\nfont-size = x"),
            Ghostty::default()
        );
    }

    #[test]
    fn ghostty_fills_gaps_but_explicit_wins() {
        let ghostty = || {
            Some(
                "font-family = Hack\nfont-size = 16\nbackground = #000000\ncursor-color = #ffffff\npalette = 0=#111111"
                    .to_owned(),
            )
        };
        let s = resolve(
            Some(r#"{"terminal":{"ghostty":true,"fontSize":12}}"#),
            ghostty,
        );
        assert_eq!(s.font_family.as_deref(), Some("Hack"));
        assert_eq!(s.font_size, 12.0);
        assert_eq!(
            s.colors,
            Some(Colors {
                background: Some("#000000".into()),
                foreground: None,
                cursor: Some("#ffffff".into()),
                selection_background: None,
                selection_foreground: None,
                palette: {
                    let mut p = vec![None; 16];
                    p[0] = Some("#111111".into());
                    p
                },
            })
        );
    }

    #[test]
    fn ghostty_ignored_unless_requested_and_warns_when_missing() {
        let never = || -> Option<String> { panic!("must not read") };
        assert_eq!(resolve(Some(r#"{"theme":"light"}"#), never).colors, None);
        let s = resolve(Some(r#"{"terminal":{"ghostty":true}}"#), none);
        assert_eq!(s.warnings.len(), 1);
    }

    #[test]
    fn load_reads_from_a_temp_home() {
        let home = std::env::temp_dir().join(format!("poimen-settings-{}", std::process::id()));
        std::fs::create_dir_all(home.join(".config/poimen")).unwrap();
        std::fs::write(config_path(&home), r#"{"theme":"light"}"#).unwrap();
        assert_eq!(load(&home).theme, "light");
        std::fs::remove_dir_all(&home).unwrap();
        assert_eq!(
            load(&home),
            Settings {
                home: Some(home.to_string_lossy().into_owned()),
                ..Settings::default()
            }
        );
    }

    #[test]
    fn the_herdr_color_schema_is_ignored() {
        let s = rows(r#"{"colorSchema":"herdr"}"#);
        assert_eq!(s.color_schema, None);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        let other = rows(r#"{"colorSchema":"vesper"}"#);
        assert_eq!(other.color_schema, Some(ColorSchema::Name("vesper".into())));
    }

    #[test]
    fn the_old_herdr_keys_are_named_and_never_an_error() {
        let text = "{\n \"backend\": \"herdr\", \"herdr\": {\"autoStart\": false}, \"colorSchema\": \"herdr\"}";
        assert_eq!(
            ignored_herdr_keys(text),
            ["backend", "herdr", "colorSchema \"herdr\""]
        );
        let s = rows(text);
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        assert!(ignored_herdr_keys(r#"{"colorSchema":"vesper"}"#).is_empty());
        assert!(ignored_herdr_keys("not json").is_empty());
    }

    #[test]
    fn set_theme_replaces_the_value_and_keeps_the_rest() {
        let text = "{\n  // my theme\n  \"theme\": \"system\", /* c */\n  \"terminal\": {\"theme\": \"x\"}\n}";
        let out = set_theme_text(text, "dark").unwrap();
        assert_eq!(
            out,
            "{\n  // my theme\n  \"theme\": \"dark\", /* c */\n  \"terminal\": {\"theme\": \"x\"}\n}"
        );
    }

    #[test]
    fn set_theme_inserts_when_missing_and_ignores_nested_keys() {
        let out = set_theme_text("{\"terminal\":{\"theme\":\"x\"}}", "light").unwrap();
        assert_eq!(
            out,
            "{\n  \"theme\": \"light\",\"terminal\":{\"theme\":\"x\"}}"
        );
        assert_eq!(
            set_theme_text("{ }", "dark").unwrap(),
            "{\"theme\": \"dark\"}"
        );
        assert_eq!(resolve(Some(&out), none).theme, "light");
    }

    #[test]
    fn set_color_schema_replaces_a_name_with_a_pair_and_keeps_comments() {
        let text =
            "{\n  // scheme\n  \"colorSchema\": \"dracula\", /* c */\n  \"theme\": \"dark\"\n}";
        let pair = serde_json::json!({"light": "one-light", "dark": "one-dark"});
        let out = set_color_schema_text(text, &pair).unwrap();
        assert_eq!(
            out,
            "{\n  // scheme\n  \"colorSchema\": {\"dark\":\"one-dark\",\"light\":\"one-light\"}, /* c */\n  \"theme\": \"dark\"\n}"
        );
        let back = set_color_schema_text(&out, &serde_json::json!("vesper")).unwrap();
        assert_eq!(
            back,
            "{\n  // scheme\n  \"colorSchema\": \"vesper\", /* c */\n  \"theme\": \"dark\"\n}"
        );
        let plain = set_color_schema_text("{}", &pair).unwrap();
        assert_eq!(
            resolve(Some(&plain), none).color_schema,
            Some(ColorSchema::Pair {
                light: "one-light".into(),
                dark: "one-dark".into()
            })
        );
        assert!(back.contains("// scheme"));
    }

    #[test]
    fn set_color_schema_inserts_when_missing() {
        let out =
            set_color_schema_text("{\"theme\":\"dark\"}", &serde_json::json!("vesper")).unwrap();
        assert_eq!(out, "{\n  \"colorSchema\": \"vesper\",\"theme\":\"dark\"}");
    }

    #[test]
    fn set_color_schema_rejects_bad_input() {
        for bad in [
            serde_json::json!(""),
            serde_json::json!(3),
            serde_json::json!({"light": "a"}),
            serde_json::json!({"light": "a", "dark": "b", "x": "c"}),
        ] {
            assert!(set_color_schema_text("{}", &bad).is_err(), "{bad}");
        }
        assert!(set_color_schema_text("[1]", &serde_json::json!("a")).is_err());
    }

    #[test]
    fn save_color_schema_writes_a_temp_file() {
        let home = std::env::temp_dir().join(format!("poimen-save-cs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        save_color_schema(&home, &serde_json::json!("dracula")).unwrap();
        let text = std::fs::read_to_string(config_path(&home)).unwrap();
        assert_eq!(text, "{\"colorSchema\": \"dracula\"}\n");
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn set_theme_rejects_bad_input() {
        assert!(set_theme_text("{}", "blue").is_err());
        assert!(set_theme_text("[1]", "dark").is_err());
    }

    #[test]
    fn save_theme_writes_and_creates_the_file() {
        let home = std::env::temp_dir().join(format!("poimen-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        save_theme(&home, "dark").unwrap();
        assert_eq!(load(&home).theme, "dark");
        std::fs::write(
            config_path(&home),
            r#"{"terminal":{"fontSize":15},"theme":"dark"}"#,
        )
        .unwrap();
        save_theme(&home, "light").unwrap();
        let s = load(&home);
        assert_eq!((s.theme.as_str(), s.font_size), ("light", 15.0));
        std::fs::remove_dir_all(&home).unwrap();
    }

    #[test]
    fn serializes_camel_case() {
        let v = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(v["fontSize"], DEFAULT_FONT_SIZE);
        assert!(v["fontFamily"].is_null());
        assert_eq!(v["autoControl"], true);
        assert_eq!(v["shortcuts"]["toggleSidebar"], "CmdOrCtrl+B");
    }

    #[test]
    fn normalizes_accelerators() {
        assert_eq!(
            normalize_accelerator("cmd+shift+w").as_deref(),
            Some("Cmd+Shift+W")
        );
        assert_eq!(
            normalize_accelerator("shift+ctrl+arrowleft").as_deref(),
            Some("Ctrl+Shift+Left")
        );
        assert_eq!(
            normalize_accelerator("CmdOrCtrl+\\").as_deref(),
            Some("CmdOrCtrl+\\")
        );
        assert_eq!(normalize_accelerator("f5").as_deref(), Some("F5"));
    }

    #[test]
    fn rejects_invalid_accelerators() {
        for bad in ["", "W", "Shift+W", "Hyper+W", "Cmd+", "Cmd+Foo", "Cmd+F13"] {
            assert_eq!(normalize_accelerator(bad), None, "{bad}");
        }
    }

    #[test]
    fn shortcuts_default_without_overrides() {
        let s = resolve(Some("{}"), none);
        assert_eq!(s.shortcuts, default_shortcuts());
        assert_eq!(s.shortcuts["closeTab"], "CmdOrCtrl+W");
        assert_eq!(s.shortcuts["find"], "CmdOrCtrl+Shift+F");
        assert_eq!(s.shortcuts["jump"], "CmdOrCtrl+K");
        assert_eq!(s.shortcuts["toggleOutput"], "CmdOrCtrl+J");
    }

    #[test]
    fn shortcut_overrides_apply_and_can_unbind() {
        let s = resolve(
            Some(r#"{"shortcuts":{"toggleSidebar":"ctrl+alt+b","refresh":""}}"#),
            none,
        );
        assert_eq!(s.shortcuts["toggleSidebar"], "Ctrl+Alt+B");
        assert_eq!(s.shortcuts["refresh"], "");
        assert_eq!(s.shortcuts["closeTab"], "CmdOrCtrl+W");
        assert!(s.warnings.is_empty());
    }

    #[test]
    fn invalid_shortcuts_warn_and_fall_back() {
        let s = resolve(
            Some(
                r#"{"shortcuts":{"closeTab":"nope","bogus":"Cmd+X","zoomIn":3,"refresh":"CmdOrCtrl+B"}}"#,
            ),
            none,
        );
        assert_eq!(s.shortcuts["closeTab"], "CmdOrCtrl+W");
        assert_eq!(s.shortcuts["zoomIn"], "CmdOrCtrl+=");
        assert_eq!(s.shortcuts["refresh"], "CmdOrCtrl+R");
        assert!(!s.shortcuts.contains_key("bogus"));
        assert_eq!(s.warnings.len(), 4, "{:?}", s.warnings);
        let bad = resolve(Some(r#"{"shortcuts":[]}"#), none);
        assert_eq!(bad.shortcuts, default_shortcuts());
        assert_eq!(bad.warnings.len(), 1);
    }
}
