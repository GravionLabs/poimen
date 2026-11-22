//! `space.create` with a layout through the backend, and a task's `env` in it (#992).

use super::*;
use crate::backend::LayoutRequest;

#[test]
fn a_tasks_env_goes_into_space_create() {
    let (backend, log) = backend_for("space-env", vec![], false);
    let env = std::collections::BTreeMap::from([
        ("POIMEN_ISSUE".to_owned(), "12".to_owned()),
        ("POIMEN_BRANCH".to_owned(), "feat/12-x".to_owned()),
    ]);
    let request = LayoutRequest {
        env: Some(&env),
        ..layout_request("/work/api", None, None)
    };
    backend.create_space_with_layout(request).unwrap().unwrap();
    assert_eq!(
        params_of(&log, "space.create")["env"],
        json!({"POIMEN_ISSUE": "12", "POIMEN_BRANCH": "feat/12-x"})
    );
}

pub(super) fn layout_request<'a>(
    cwd: &'a str,
    decision: Option<&'a trust::Decision>,
    command: Option<&'a str>,
) -> LayoutRequest<'a> {
    LayoutRequest {
        cwd,
        label: "api",
        decision,
        command,
        env: None,
    }
}

#[test]
fn a_new_space_with_layout_is_built_by_the_server() {
    let (backend, log) = backend_for("space-layout", vec![], false);
    let request = layout_request("/work/api", None, Some("claude 'x'"));
    let outcome = backend.create_space_with_layout(request).unwrap().unwrap();
    assert_eq!(
        outcome,
        LaidOut::Created {
            space_id: "s9".into(),
            warnings: vec!["template app: harness codex is not installed".into()],
            setup_started: true,
        }
    );
    assert_eq!(
        params_of(&log, "space.create"),
        json!({"name": "api", "cwd": "/work/api", "layout": true, "command": "claude 'x'"})
    );
    // The server built the space: no terminal, split or input from the app.
    assert_eq!(lock(&log).len(), 1);
}
