//! The small tab requests: `tab.rename`, `tab.move` and `tab.focus`.

use serde_json::json;

use poimen_protocol::{TabFocus, TabMove, TabRename};

use crate::{ApiResult, Shared, lock, require_name, tab_json};

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
