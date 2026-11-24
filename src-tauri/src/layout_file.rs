//! Export and import of one layout as a JSON file (#611, on top of `settings_transfer`): the file
//! is the layout object itself, `tabs[]` (ADR 0011) or `panes[]`, as the project file and
//! `layouts[]` hold it. Import only reads and validates; where the layout goes is the caller's
//! choice and a save of the project file still goes through `trust::gate`.

use std::path::Path;

use serde_json::Value;

use crate::layout::parse_tabs_value;
use crate::settings_transfer::{read_import, write_export};

/// A layout the loader would read without dropping anything.
fn check(layout: &Value) -> Result<(), String> {
    let (_, warnings) = parse_tabs_value(layout)?;
    match warnings.first() {
        Some(w) => Err(w.clone()),
        None => Ok(()),
    }
}

/// Writes `layout` (pretty, one trailing newline) to `path`, which must end in `.json`.
pub fn export(path: &Path, layout: &Value) -> Result<(), String> {
    check(layout)?;
    let mut text = serde_json::to_string_pretty(layout).map_err(|e| e.to_string())?;
    text.push('\n');
    write_export(path, &text)
}

/// Reads the layout in `path`: a regular file up to 1 MiB holding a valid layout object.
pub fn import(path: &Path) -> Result<Value, String> {
    let layout = Value::Object(read_import(path)?);
    check(&layout).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(layout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir::TestDir;
    use serde_json::json;

    fn temp(name: &str) -> (TestDir, std::path::PathBuf) {
        let dir = TestDir::new("poimen-layout-file-");
        let path = dir.join(name);
        (dir, path)
    }

    #[test]
    fn a_tabs_layout_survives_export_and_import_unchanged() {
        let layout = json!({"tabs": [{"name": "agent", "command": "claude", "children": [
            {"name": "t", "place": {"direction": "down", "ratio": 0.3},
             "root": {"kind": "split", "direction": "right", "ratio": 0.4, "children": [
                {"kind": "terminal", "name": "a"}, {"kind": "terminal", "name": "b"}]}}]}]});
        let (_dir, path) = temp("round.json");
        export(&path, &layout).unwrap();
        assert_eq!(import(&path).unwrap(), layout);
    }

    #[test]
    fn invalid_layouts_are_refused_both_ways() {
        let (_dir, path) = temp("bad.json");
        assert!(export(&path, &json!({"panes": []})).is_err());
        std::fs::write(
            &path,
            r#"{"tabs": [{"name": "a", "place": {"direction": "up"}}]}"#,
        )
        .unwrap();
        assert!(import(&path).unwrap_err().contains("place"));
        std::fs::write(&path, "[1]").unwrap();
        assert!(import(&path).is_err());
        let (_dir2, other) = temp("x.txt");
        assert!(export(&other, &json!({"panes": [{"name": "a"}]})).is_err());
    }
}
