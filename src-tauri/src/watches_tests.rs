//! Tests of `Watches` (a child of the crate root, which owns it).

use super::Watches;

#[test]
fn a_newer_command_makes_an_older_result_stale() {
    let mut watches = Watches::<&str>::default();
    let older = watches.begin("panel");
    let newer = watches.begin("panel");
    assert!(watches.store("panel", newer, Some("new")));
    assert!(!watches.store("panel", older, Some("old")));
    assert_eq!(watches.slots["panel"].1, Some("new"));
}

#[test]
fn a_remove_before_a_watch_finishes_is_not_undone() {
    let mut watches = Watches::<&str>::default();
    let generation = watches.begin("panel");
    watches.remove("panel");
    assert!(!watches.store("panel", generation, Some("late")));
    assert!(!watches.slots.contains_key("panel"));
}

#[test]
fn a_key_begun_again_after_a_remove_does_not_take_an_older_result() {
    let mut watches = Watches::<&str>::default();
    let older = watches.begin("flow");
    watches.remove("flow");
    let newer = watches.begin("flow");
    assert!(!watches.store("flow", older, Some("old")));
    assert!(watches.store("flow", newer, Some("new")));
}

#[test]
fn keys_do_not_make_each_other_stale() {
    let mut watches = Watches::<&str>::default();
    let panel = watches.begin("panel");
    let flow = watches.begin("flow");
    assert!(watches.store("panel", panel, Some("p")));
    assert!(watches.store("flow", flow, Some("f")));
    watches.remove("flow");
    assert_eq!(watches.slots["panel"].1, Some("p"));
}

#[test]
fn a_current_result_replaces_the_value_and_none_clears_it() {
    let mut watches = Watches::<&str>::default();
    let g = watches.begin("panel");
    assert!(watches.store("panel", g, Some("a")));
    let g = watches.begin("panel");
    assert!(watches.store("panel", g, None));
    assert_eq!(watches.slots["panel"].1, None);
}
