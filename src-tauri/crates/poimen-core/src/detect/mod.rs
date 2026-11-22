//! Palette entries detected in a repository without a project file: one `Detector` per source
//! (`package.json` first; `Cargo.toml`, `justfile` and others implement the same trait later).
//! Nothing here runs a command: an entry carries the argv a user pick would run.

pub mod npm;

use std::path::Path;

use serde::Serialize;

/// One detected command: what the palette lists and the argv that runs on a pick.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DetectedEntry {
    /// The detector that found it (`npm`), the tag of its palette source.
    pub source: String,
    /// The name of the entry, also the name of the tab it opens.
    pub name: String,
    /// What the entry runs, for the user (for npm the command line of the script).
    pub description: String,
    /// The program and its arguments; never sent to the frontend, a pick names the entry and the
    /// backend detects again.
    #[serde(skip)]
    pub argv: Vec<String>,
}

/// A source of detected entries.
pub trait Detector {
    /// The entries found at the repository `root`; a missing or invalid source yields none.
    fn detect(&self, root: &Path) -> Vec<DetectedEntry>;
}

/// The detectors, in the order their entries are listed.
pub fn detectors() -> Vec<Box<dyn Detector>> {
    vec![Box::new(npm::Npm)]
}

/// The entries of every detector at `root`.
pub fn detect_all(root: &Path) -> Vec<DetectedEntry> {
    detectors().iter().flat_map(|d| d.detect(root)).collect()
}

/// The command line to type into a shell for `argv`: elements with characters a shell treats
/// specially are single-quoted.
pub fn command_line(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            let plain = !a.is_empty()
                && a.chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._:/@%+=,-".contains(c));
            if plain {
                a.clone()
            } else {
                crate::tasks::shell_quote(a)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_quotes_only_what_a_shell_would_expand() {
        let argv = ["pnpm", "run", "build:prod"].map(String::from);
        assert_eq!(command_line(&argv), "pnpm run build:prod");
        let argv = ["npm", "run", "a b;rm"].map(String::from);
        assert_eq!(command_line(&argv), "npm run 'a b;rm'");
        let argv = ["npm", "run", "it's"].map(String::from);
        assert_eq!(command_line(&argv), "npm run 'it'\\''s'");
    }
}
