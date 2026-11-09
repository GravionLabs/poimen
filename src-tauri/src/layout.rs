//! The pane layout of a new space. Parsing and resolution live in `poimen_core::layout`; `apply`
//! goes through the `Backend` trait, so tests use a fake and never touch a live backend.

pub use poimen_core::layout::*;

/// Builds the layout in the new space: the root pane is named and gets its command, every other
/// pane splits an earlier one (`pane.split` in `cwd`, without focus), is named and gets its
/// command typed. `backend` makes the calls.
pub fn apply(
    backend: &dyn crate::backend::Backend,
    layout: &Layout,
    cwd: &str,
    root_pane: &str,
) -> Result<(), String> {
    let mut ids: Vec<(&str, String)> = Vec::new();
    for spec in &layout.panes {
        let id = match &spec.split {
            None => root_pane.to_owned(),
            Some(s) => {
                let target = ids
                    .iter()
                    .find(|(n, _)| *n == s.from)
                    .map(|(_, id)| id.as_str())
                    .ok_or(format!("pane {}: unknown split.from {}", spec.name, s.from))?;
                backend
                    .split(target, s.direction, Some(cwd), Some(s.ratio))?
                    .ok_or("herdr did not report the new pane")?
            }
        };
        backend.rename_terminal(&id, Some(&spec.name))?;
        if let Some(command) = &spec.command {
            backend.send_input(&id, command)?;
        }
        ids.push((&spec.name, id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend_fake::FakeBackend;

    #[test]
    fn apply_of_a_layout_without_commands_types_nothing() {
        let fake = FakeBackend::default();
        apply(
            &fake,
            &default_layout().without_commands(),
            "/work/app",
            "w1:root",
        )
        .unwrap();
        assert!(fake.calls().iter().all(|c| !c.starts_with("input ")));
    }

    #[test]
    fn apply_splits_renames_and_types_in_order() {
        let fake = FakeBackend::default();
        apply(&fake, &default_layout(), "/work/app", "w1:root").unwrap();
        assert_eq!(
            fake.calls(),
            [
                "rename w1:root agent",
                "input w1:root claude",
                "split w1:root right /work/app 0.6",
                "rename w1:p12 tests",
                // `shell` splits `tests` (the pane created by the first split), downward.
                "split w1:p12 down /work/app 0.5",
                "rename w1:p14 shell",
            ]
        );
    }

    #[test]
    fn apply_stops_on_a_failed_call() {
        let fake = FakeBackend::default();
        fake.fail_on("split");
        assert_eq!(
            apply(&fake, &default_layout(), "/work/app", "w1:root"),
            Err("boom".into())
        );
        assert_eq!(
            fake.calls(),
            ["rename w1:root agent", "input w1:root claude"]
        );
    }
}
