//! The commands of an open terminal stream: input, resize and the prompt draft (#1163).

use tauri::State;

use crate::{Shared, SharedMachines, machines, on_routed, stream_of};

#[tauri::command]
pub fn terminal_input(
    backend: State<'_, Shared>,
    remote: State<'_, std::sync::Arc<machines::RemoteStreams>>,
    id: u32,
    text: String,
) -> Result<(), String> {
    let (backend, id) = stream_of(&backend, &remote, id);
    backend.stream_input(id, &text)
}

#[tauri::command]
pub fn terminal_resize(
    backend: State<'_, Shared>,
    remote: State<'_, std::sync::Arc<machines::RemoteStreams>>,
    id: u32,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let (backend, id) = stream_of(&backend, &remote, id);
    backend.stream_resize(id, cols, rows)
}

/// Sets or clears the draft of `target` (a pane id, of a machine too). An error is the caller's to
/// ignore: a server of an older release does not know the request, and the draft only guards.
#[tauri::command]
pub async fn terminal_draft(
    backend: State<'_, Shared>,
    machines: State<'_, SharedMachines>,
    target: String,
    draft: bool,
) -> Result<(), String> {
    on_routed(backend.inner(), machines.inner(), &target, move |b, id| {
        b.terminal_draft(id, draft)
    })
    .await
}
