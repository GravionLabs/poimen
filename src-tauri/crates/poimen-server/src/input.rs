//! Input and resize of a terminal: who may send them and when a terminal cannot take them.

use super::*;

pub(super) fn running_host<'a>(
    entry: &'a mut TerminalEntry,
    id: &str,
) -> Result<&'a mut host::Handle, (ErrorCode, String)> {
    let from_history = entry.info.from_history;
    entry.host.as_mut().ok_or_else(|| {
        let why = if from_history {
            format!("terminal {id} has ended; its history is read-only")
        } else {
            format!("terminal {id} has exited")
        };
        (ErrorCode::Exited, why)
    })
}

/// Input belongs to the controller. A connection attached to this terminal must be its
/// controller to send it; a connection that is not attached may send input only while nobody
/// controls. Resize also belongs to observers and to unattached connections while nobody
/// controls, so the unfocused leaves of a split tab follow their containers.
fn authorize(
    stream: &TermStream,
    ctx: Option<&Attachment>,
    terminal_id: &str,
    resize: bool,
) -> Result<(), (ErrorCode, String)> {
    let denied = |why: &str| Err((ErrorCode::NotController, why.to_string()));
    match ctx.filter(|a| a.terminal_id == terminal_id) {
        Some(_) if resize && !stream.has_controller() => Ok(()),
        Some(a) if a.mode == AttachMode::Observe => {
            denied("an observer can neither send input nor resize a terminal with a controller")
        }
        Some(a) if !stream.is_controller(a.client_id) => denied("the control was taken over"),
        Some(_) => Ok(()),
        None if resize && stream.has_controller() => {
            denied("only the controller resizes a terminal that has one")
        }
        None => Ok(()),
    }
}

pub(super) fn terminal_input(
    shared: &Shared,
    p: TerminalInput,
    ctx: Option<&Attachment>,
) -> ApiResult {
    // The write may block on a full PTY; do it outside the global lock.
    let writer = {
        let mut inner = lock(&shared.inner);
        let entry = terminal_mut(&mut inner, &p.terminal_id)?;
        if entry.info.from_history {
            // Before the controller check: an attachment to it is never the controller.
            running_host(entry, &p.terminal_id)?;
        }
        authorize(&entry.stream, ctx, &p.terminal_id, false)?;
        let writer = running_host(entry, &p.terminal_id)?.writer();
        entry.last_input_ms = Some(now_millis());
        reset_error_on_input(shared, &mut inner, &p.terminal_id, &p.data);
        writer
    };
    writer
        .write(p.data.as_bytes())
        .map_err(|e| (ErrorCode::Internal, e))?;
    Ok(json!({}))
}

pub(super) fn terminal_resize(
    shared: &Shared,
    p: TerminalResize,
    ctx: Option<&Attachment>,
) -> ApiResult {
    if p.cols == 0 || p.rows == 0 {
        return Err((
            ErrorCode::InvalidParams,
            "cols and rows must be at least 1".into(),
        ));
    }
    let mut inner = lock(&shared.inner);
    let entry = terminal_mut(&mut inner, &p.terminal_id)?;
    authorize(&entry.stream, ctx, &p.terminal_id, true)?;
    running_host(entry, &p.terminal_id)?
        .resize(p.cols, p.rows)
        .map_err(|e| (ErrorCode::Internal, e))?;
    entry.info.cols = p.cols;
    entry.info.rows = p.rows;
    entry.stream.resized(p.cols, p.rows);
    shared.persist(&inner);
    Ok(json!({}))
}
