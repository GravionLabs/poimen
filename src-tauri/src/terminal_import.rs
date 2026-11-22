//! Fonts and colors taken from other terminals: Ghostty `theme = <name>` files and `kitty.conf`
//! (with `include`d files). Pure parsing; the file reads are passed in so tests need no disk.

use std::path::{Path, PathBuf};

use crate::settings::{Colors, Settings, hex_color, valid_size};

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

/// Where the other terminals' files come from; each is only read when its flag is set.
pub trait TerminalSources {
    fn ghostty(&self) -> Option<String>;
    /// A Ghostty theme file by name.
    fn ghostty_theme(&self, _name: &str) -> Option<String> {
        None
    }
    fn kitty(&self) -> Option<String> {
        None
    }
    /// A file `include`d by `kitty.conf`, relative to its directory.
    fn kitty_include(&self, _name: &str) -> Option<String> {
        None
    }
}

pub struct GhosttyOnly<F>(std::cell::RefCell<Option<F>>);

impl<F: FnOnce() -> Option<String>> TerminalSources for GhosttyOnly<F> {
    fn ghostty(&self) -> Option<String> {
        self.0.borrow_mut().take().and_then(|f| f())
    }
}

/// The files below a home directory.
pub struct HomeSources<'a>(pub &'a Path);

impl TerminalSources for HomeSources<'_> {
    fn ghostty(&self) -> Option<String> {
        std::fs::read_to_string(crate::settings::ghostty_path(self.0)).ok()
    }
    fn ghostty_theme(&self, name: &str) -> Option<String> {
        read_ghostty_theme(name, &ghostty_theme_dirs(self.0))
    }
    fn kitty(&self) -> Option<String> {
        std::fs::read_to_string(crate::settings::kitty_path(self.0)).ok()
    }
    fn kitty_include(&self, name: &str) -> Option<String> {
        read_kitty_include(&kitty_dir(self.0), name)
    }
}

pub fn ghostty_only<F: FnOnce() -> Option<String>>(f: F) -> GhosttyOnly<F> {
    GhosttyOnly(std::cell::RefCell::new(Some(f)))
}

/// Applies the Ghostty and Kitty files to `s`: explicit poimen entries win, Ghostty wins over
/// Kitty where both set a value.
pub fn apply(
    s: &mut Settings,
    sources: &dyn TerminalSources,
    use_ghostty: bool,
    use_kitty: bool,
    explicit_family: bool,
    explicit_size: bool,
) {
    let mut imported: Option<Ghostty> = None;
    if use_ghostty {
        match sources.ghostty() {
            None => s
                .warnings
                .push("Ghostty config not found; ignoring terminal.ghostty".into()),
            Some(text) => {
                let g = ghostty_with_theme(
                    &text,
                    &s.theme,
                    &|n| sources.ghostty_theme(n),
                    &mut s.warnings,
                );
                imported = Some(g);
            }
        }
    }
    if use_kitty {
        match sources.kitty() {
            None => s
                .warnings
                .push("Kitty config not found; ignoring terminal.kitty".into()),
            Some(text) => {
                let k = parse_kitty(&text, &|n| sources.kitty_include(n));
                imported = Some(match imported {
                    Some(g) => over(g, k),
                    None => k,
                });
            }
        }
    }
    if let Some(g) = imported {
        // Explicit poimen entries win over the imported ones.
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

const MAX_INCLUDE_DEPTH: usize = 8;

/// The theme name(s) of a Ghostty config: `theme = name` or `theme = light:A,dark:B`.
#[derive(Debug, PartialEq)]
pub enum ThemeName {
    One(String),
    Pair { light: String, dark: String },
}

impl ThemeName {
    /// The name for the poimen `theme` setting; `system` cannot be known here and takes dark.
    pub fn pick(&self, theme: &str) -> &str {
        match self {
            Self::One(n) => n,
            Self::Pair { light, dark } => {
                if theme == "light" {
                    light
                } else {
                    dark
                }
            }
        }
    }
}

/// The last `theme` line of a Ghostty config, if any.
pub fn ghostty_theme_name(text: &str) -> Option<ThemeName> {
    let value = text.lines().rev().find_map(|line| {
        let (key, value) = line.trim().split_once('=')?;
        (key.trim() == "theme").then(|| value.trim().trim_matches('"').trim().to_owned())
    })?;
    if value.is_empty() {
        return None;
    }
    let (mut light, mut dark) = (None, None);
    for part in value.split(',') {
        match part.trim().split_once(':') {
            Some(("light", n)) => light = Some(n.trim().to_owned()),
            Some(("dark", n)) => dark = Some(n.trim().to_owned()),
            _ => {}
        }
    }
    match (light, dark) {
        (Some(light), Some(dark)) => Some(ThemeName::Pair { light, dark }),
        (Some(n), None) | (None, Some(n)) => Some(ThemeName::One(n)),
        (None, None) => Some(ThemeName::One(value)),
    }
}

/// The directories Ghostty looks themes up in, in order.
pub fn ghostty_theme_dirs(home: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(x) = std::env::var("XDG_CONFIG_HOME")
        && !x.is_empty()
    {
        dirs.push(PathBuf::from(x).join("ghostty").join("themes"));
    }
    dirs.push(home.join(".config/ghostty/themes"));
    if let Ok(r) = std::env::var("GHOSTTY_RESOURCES_DIR")
        && !r.is_empty()
    {
        dirs.push(PathBuf::from(r).join("themes"));
    }
    dirs.push("/usr/share/ghostty/themes".into());
    dirs.push("/usr/local/share/ghostty/themes".into());
    dirs.push("/Applications/Ghostty.app/Contents/Resources/ghostty/themes".into());
    dirs
}

/// Reads a theme file by name from the theme directories; an absolute path is read as it is.
/// A name with a path separator that is not absolute is refused.
pub fn read_ghostty_theme(name: &str, dirs: &[PathBuf]) -> Option<String> {
    if Path::new(name).is_absolute() {
        return std::fs::read_to_string(name).ok();
    }
    if name.is_empty() || name.contains(['/', '\\']) || name.contains("..") {
        return None;
    }
    dirs.iter()
        .find_map(|d| std::fs::read_to_string(d.join(name)).ok())
}

/// `config` over `theme`: every value the config sets wins, the theme fills the gaps.
pub fn over(config: Ghostty, theme: Ghostty) -> Ghostty {
    let mut palette = theme.palette;
    for (i, c) in config.palette.into_iter().enumerate() {
        if c.is_some() {
            palette.resize(16, None);
            palette[i] = c;
        }
    }
    Ghostty {
        font_family: config.font_family.or(theme.font_family),
        font_size: config.font_size.or(theme.font_size),
        background: config.background.or(theme.background),
        foreground: config.foreground.or(theme.foreground),
        cursor: config.cursor.or(theme.cursor),
        selection_background: config.selection_background.or(theme.selection_background),
        selection_foreground: config.selection_foreground.or(theme.selection_foreground),
        palette,
    }
}

/// Reads `kitty.conf` text (`key value`, `#` comments). `include <file>` is read through
/// `read` (the file name is relative to the config directory) up to a depth of 8; later lines
/// override earlier ones.
pub fn parse_kitty(text: &str, read: &dyn Fn(&str) -> Option<String>) -> Ghostty {
    let mut out = Ghostty::default();
    apply_kitty(text, read, &mut out, 0);
    out
}

fn apply_kitty(text: &str, read: &dyn Fn(&str) -> Option<String>, out: &mut Ghostty, depth: usize) {
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let value = value.trim();
        match key {
            "include" => {
                if depth < MAX_INCLUDE_DEPTH
                    && !value.is_empty()
                    && let Some(inner) = read(value)
                {
                    apply_kitty(&inner, read, out, depth + 1);
                }
            }
            "font_family" if !value.is_empty() && value != "auto" => {
                out.font_family = Some(value.to_owned());
            }
            "font_size" => {
                if let Some(s) = value.parse().ok().filter(|s| valid_size(*s)) {
                    out.font_size = Some(s);
                }
            }
            "background" => out.background = hex_color(value).or(out.background.take()),
            "foreground" => out.foreground = hex_color(value).or(out.foreground.take()),
            "cursor" => out.cursor = hex_color(value).or(out.cursor.take()),
            "selection_background" => {
                out.selection_background = hex_color(value).or(out.selection_background.take());
            }
            "selection_foreground" => {
                out.selection_foreground = hex_color(value).or(out.selection_foreground.take());
            }
            k => {
                if let Some(n) = k.strip_prefix("color")
                    && let Ok(i) = n.parse::<usize>()
                    && i < 16
                    && let Some(c) = hex_color(value)
                {
                    out.palette.resize(16, None);
                    out.palette[i] = Some(c);
                }
            }
        }
    }
}

/// Kitty's config directory.
pub fn kitty_dir(home: &Path) -> PathBuf {
    if let Ok(x) = std::env::var("KITTY_CONFIG_DIRECTORY")
        && !x.is_empty()
    {
        return PathBuf::from(x);
    }
    home.join(".config").join("kitty")
}

/// Reads an included kitty file relative to `dir`; absolute paths and `..` are refused.
pub fn read_kitty_include(dir: &Path, name: &str) -> Option<String> {
    let p = Path::new(name);
    if p.is_absolute() || name.contains("..") {
        return None;
    }
    std::fs::read_to_string(dir.join(p)).ok()
}

/// Parses a Ghostty config and resolves its theme through `read_theme`. A theme that is not
/// found is named in `warnings` and changes nothing.
pub fn ghostty_with_theme(
    text: &str,
    theme: &str,
    read_theme: &dyn Fn(&str) -> Option<String>,
    warnings: &mut Vec<String>,
) -> Ghostty {
    let config = parse_ghostty(text);
    let Some(name) = ghostty_theme_name(text) else {
        return config;
    };
    let name = name.pick(theme);
    match read_theme(name) {
        Some(t) => over(config, parse_ghostty(&t)),
        None => {
            warnings.push(format!("Ghostty theme {name} not found; ignoring it"));
            config
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn theme_names() {
        assert_eq!(
            ghostty_theme_name("a = b\ntheme = Catppuccin Mocha\n"),
            Some(ThemeName::One("Catppuccin Mocha".into()))
        );
        let p = ghostty_theme_name("theme = \"light:Day,dark:Night\"").unwrap();
        assert_eq!(p.pick("light"), "Day");
        assert_eq!(p.pick("dark"), "Night");
        assert_eq!(p.pick("system"), "Night");
        assert_eq!(ghostty_theme_name("theme =\n"), None);
        assert_eq!(ghostty_theme_name("font-size = 3"), None);
    }

    #[test]
    fn theme_fills_gaps_and_explicit_wins() {
        let mut w = Vec::new();
        let g = ghostty_with_theme(
            "theme = x\nbackground = #000000\npalette = 1=#111111",
            "dark",
            &|n| {
                assert_eq!(n, "x");
                Some("background = #ffffff\nforeground = #eeeeee\npalette = 1=#222222\npalette = 2=#333333".into())
            },
            &mut w,
        );
        assert!(w.is_empty());
        assert_eq!(g.background.as_deref(), Some("#000000"));
        assert_eq!(g.foreground.as_deref(), Some("#eeeeee"));
        assert_eq!(g.palette[1].as_deref(), Some("#111111"));
        assert_eq!(g.palette[2].as_deref(), Some("#333333"));
    }

    #[test]
    fn missing_theme_warns_and_changes_nothing() {
        let mut w = Vec::new();
        let g = ghostty_with_theme(
            "theme = nope\nbackground = #000000",
            "dark",
            &|_| None,
            &mut w,
        );
        assert_eq!(w.len(), 1);
        assert_eq!(g, parse_ghostty("background = #000000"));
    }

    #[test]
    fn theme_lookup_refuses_traversal_and_searches_dirs() {
        let d = std::env::temp_dir().join(format!("poimen-gt-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("Mine"), "background = #123456").unwrap();
        let dirs = vec![PathBuf::from("/nonexistent"), d.clone()];
        assert_eq!(
            read_ghostty_theme("Mine", &dirs).as_deref(),
            Some("background = #123456")
        );
        assert_eq!(read_ghostty_theme("../Mine", &dirs), None);
        assert_eq!(read_ghostty_theme("Other", &dirs), None);
        let abs = d.join("Mine");
        assert!(read_ghostty_theme(abs.to_str().unwrap(), &[]).is_some());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn parses_kitty_with_includes() {
        let read = |n: &str| match n {
            "theme.conf" => {
                Some("background #1e1e2e\ncolor1 #f38ba8\ninclude deep.conf".to_owned())
            }
            "deep.conf" => Some("foreground #cdd6f4\ncolor15 a6adc8".to_owned()),
            _ => None,
        };
        let k = parse_kitty(
            "# c\nfont_family  Fira Code\nfont_size 12.5\ninclude theme.conf\ninclude missing.conf\ncursor #f5e0dc\ncolor16 #000000\ncolor2 red\nselection_background #585b70\nbackground #000000\nbad\n",
            &read,
        );
        assert_eq!(k.font_family.as_deref(), Some("Fira Code"));
        assert_eq!(k.font_size, Some(12.5));
        assert_eq!(k.background.as_deref(), Some("#000000"));
        assert_eq!(k.foreground.as_deref(), Some("#cdd6f4"));
        assert_eq!(k.cursor.as_deref(), Some("#f5e0dc"));
        assert_eq!(k.selection_background.as_deref(), Some("#585b70"));
        assert_eq!(k.palette.len(), 16);
        assert_eq!(k.palette[1].as_deref(), Some("#f38ba8"));
        assert_eq!(k.palette[15].as_deref(), Some("#a6adc8"));
        assert_eq!(k.palette.iter().flatten().count(), 2);
    }

    #[test]
    fn kitty_include_loops_stop() {
        let k = parse_kitty("include a.conf", &|_| {
            Some("include a.conf\ncolor0 #010101".into())
        });
        assert_eq!(k.palette[0].as_deref(), Some("#010101"));
    }

    #[test]
    fn kitty_include_refuses_escapes() {
        assert_eq!(read_kitty_include(Path::new("/tmp"), "../etc/passwd"), None);
        assert_eq!(read_kitty_include(Path::new("/tmp"), "/etc/passwd"), None);
    }

    struct Fake;
    impl crate::settings::TerminalSources for Fake {
        fn ghostty(&self) -> Option<String> {
            Some("theme = light:Day,dark:Night\nbackground = #000000".into())
        }
        fn ghostty_theme(&self, n: &str) -> Option<String> {
            (n == "Day").then(|| "foreground = #111111".to_owned())
        }
        fn kitty(&self) -> Option<String> {
            Some("font_family Kit\nfont_size 11\nforeground #222222\ninclude t.conf".into())
        }
        fn kitty_include(&self, n: &str) -> Option<String> {
            (n == "t.conf").then(|| "cursor #333333".to_owned())
        }
    }

    #[test]
    fn settings_combine_ghostty_theme_and_kitty() {
        let s = crate::settings::resolve_with(
            Some(r#"{"theme":"light","terminal":{"ghostty":true,"kitty":true,"fontSize":15}}"#),
            &Fake,
        );
        assert!(s.warnings.is_empty(), "{:?}", s.warnings);
        assert_eq!(s.font_family.as_deref(), Some("Kit"));
        assert_eq!(s.font_size, 15.0);
        let c = s.colors.unwrap();
        assert_eq!(c.background.as_deref(), Some("#000000"));
        assert_eq!(c.foreground.as_deref(), Some("#111111"));
        assert_eq!(c.cursor.as_deref(), Some("#333333"));
    }

    #[test]
    fn dark_theme_not_found_warns_and_keeps_config() {
        let s = crate::settings::resolve_with(
            Some(r#"{"theme":"dark","terminal":{"ghostty":true}}"#),
            &Fake,
        );
        assert_eq!(s.warnings.len(), 1);
        assert!(s.warnings[0].contains("Night"));
        assert_eq!(s.colors.unwrap().foreground, None);
    }

    #[test]
    fn kitty_missing_and_bad_flag_warn() {
        struct None_;
        impl crate::settings::TerminalSources for None_ {
            fn ghostty(&self) -> Option<String> {
                None
            }
        }
        let s = crate::settings::resolve_with(Some(r#"{"terminal":{"kitty":true}}"#), &None_);
        assert_eq!(s.warnings.len(), 1);
        let s = crate::settings::resolve_with(Some(r#"{"terminal":{"kitty":"x"}}"#), &None_);
        assert_eq!(s.warnings.len(), 1);
    }
}
