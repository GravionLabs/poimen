//! `started_by` (#982): the sidebar row of a started agent names its starter.

use poimen_console::tui::sidebar::{Model, Row};
use poimen_core::settings::{SidebarRows, SidebarSort};
use poimen_protocol::{Space, Terminal};
use serde_json::json;

fn terminal(id: &str, name: &str, started_by: Option<&str>) -> Terminal {
    serde_json::from_value(json!({
        "id": id, "space_id": "s1", "name": name, "cwd": "/w", "cols": 80, "rows": 24,
        "status": "running", "started_by": started_by,
    }))
    .unwrap()
}

fn text(rows: &[Row], id: &str) -> String {
    let row = rows
        .iter()
        .find(|r| r.terminal_id.as_deref() == Some(id))
        .unwrap();
    row.lines
        .iter()
        .flatten()
        .map(|s| s.text.clone())
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn a_started_terminal_names_its_starter_and_a_gone_starter_names_nobody() {
    let model = Model {
        spaces: vec![Space {
            id: "s1".into(),
            name: "work".into(),
            cwd: "/w".into(),
            tabs: vec![],
            active_tab: None,
        }],
        tabs: vec![],
        terminals: vec![
            terminal("lead", "lead", None),
            terminal("helper", "helper", Some("lead")),
            terminal("orphan", "orphan", Some("gone")),
        ],
        ..Model::default()
    };
    let rows = model.rows(&SidebarRows::default(), SidebarSort::Manual, None);
    assert!(
        text(&rows, "helper").contains("↳ lead"),
        "{}",
        text(&rows, "helper")
    );
    assert!(!text(&rows, "lead").contains('↳'));
    assert!(!text(&rows, "orphan").contains('↳'));
}
