//! The small tab requests: `tab.rename`, `tab.move` and `tab.focus`, and two helpers that
//! publish events and drop a terminal that has no leaf.

use serde_json::json;

use poimen_protocol::{TabFocus, TabMove, TabRename};

use crate::{ApiResult, Inner, Shared, close_entry, lock, require_name, tab_json};
use poimen_protocol::Event;

pub(super) fn tab_rename(shared: &Shared, p: TabRename) -> ApiResult {
    let name = require_name(&p.name)?;
    let mut inner = lock(&shared.inner);
    let events = inner.layout.rename(&p.tab_id, name)?;
    shared.apply(&inner, events);
    Ok(json!({ "tab": tab_json(&inner, &p.tab_id) }))
}

pub(super) fn tab_move(shared: &Shared, p: TabMove) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let events = inner.layout.move_tab(&p.tab_id, p.index)?;
    shared.apply(&inner, events);
    let tab = inner.layout.tab(&p.tab_id)?;
    Ok(json!({ "tab": tab, "space": inner.layout.space(&tab.space_id)? }))
}

/// `tab.focus`: shared by every client, the last one wins.
pub(super) fn tab_focus(shared: &Shared, p: TabFocus) -> ApiResult {
    let mut inner = lock(&shared.inner);
    let events = inner.layout.focus(&p.tab_id, p.terminal_id.as_deref())?;
    shared.apply(&inner, events);
    let tab = inner.layout.tab(&p.tab_id)?;
    Ok(json!({ "tab": tab, "space": inner.layout.space(&tab.space_id)? }))
}

pub(crate) fn publish_all(shared: &Shared, events: Vec<Event>) {
    for event in events {
        shared.publish(event);
    }
}

/// Closes a terminal that is not in any leaf yet (`terminal.closed`).
pub(crate) fn drop_terminal(shared: &Shared, inner: &mut Inner, terminal_id: &str) {
    if let Some(i) = inner
        .terminals
        .iter()
        .position(|t| t.info.id == terminal_id)
    {
        let entry = inner.terminals.remove(i);
        close_entry(shared, entry);
    }
}
