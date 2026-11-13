//! The worktree rules live in `poimen_core::worktree`; the app adds the cleanup over its `Backend`.

use crate::backend::Backend;
pub use poimen_core::worktree::*;
use poimen_core::worktree::{SpaceView, Spaces};

/// The running `Backend` as the spaces the cleanup may close (`cleanup_merged`).
pub struct BackendSpaces<'a>(pub &'a dyn Backend);

impl Spaces for BackendSpaces<'_> {
    fn view(&self) -> Result<SpaceView, String> {
        let snapshot = self.0.snapshot()?;
        Ok(SpaceView {
            panes: snapshot.panes,
            workspaces: snapshot.workspaces,
        })
    }

    fn close_space(&self, id: &str) -> Result<(), String> {
        self.0.close_space(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend_fake::FakeBackend;
    use serde_json::json;

    #[test]
    fn the_backend_snapshot_and_close_reach_the_cleanup() {
        let backend = FakeBackend::default();
        backend.snapshot.lock().unwrap().panes =
            json!([{"workspace_id": "w2", "cwd": "/elsewhere"}]);
        let spaces = BackendSpaces(&backend);
        assert_eq!(spaces.view().unwrap().panes[0]["workspace_id"], "w2");
        spaces.close_space("w2").unwrap();
        assert!(
            backend
                .calls()
                .iter()
                .any(|c| c.starts_with("close_space w2"))
        );
        backend.fail_on("close_space");
        assert_eq!(spaces.close_space("w2"), Err("boom".into()));
    }
}
