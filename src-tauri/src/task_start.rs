//! The app side of "start a task": the plan itself is `poimen_core::task_start`; opening the tab
//! of a mode without a worktree needs the `Backend`.

use crate::backend::Backend;
pub use poimen_core::task_start::*;

/// Opens the tab `name` in the space at `dir` and runs `command` in it.
pub fn start_tab(
    backend: &dyn Backend,
    workspace_id: &str,
    dir: &str,
    name: &str,
    command: &str,
) -> Result<(), String> {
    crate::project::start_tab(backend, workspace_id, dir, name, command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend_fake::FakeBackend;

    #[test]
    fn the_tab_is_created_named_and_typed_into() {
        let fake = FakeBackend::default();
        start_tab(&fake, "w1", "/work/app", "#12 plain", "sh -c 'x'").unwrap();
        let calls = fake.calls();
        assert!(calls[0].starts_with("create_terminal w1"), "{calls:?}");
        assert!(calls[1].contains("#12 plain"), "{calls:?}");
        assert!(calls[2].contains("sh -c 'x'"), "{calls:?}");
    }

    #[test]
    fn a_failing_tab_step_is_reported() {
        let fake = FakeBackend::default();
        fake.fail_on("create_terminal");
        assert_eq!(
            start_tab(&fake, "w1", "/work/app", "t", "c"),
            Err("boom".into())
        );
    }
}
