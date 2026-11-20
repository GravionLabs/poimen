//! Where the poimen-server of this build keeps its socket and its log (the Server settings page).

/// The socket of the poimen-server this build talks to.
#[tauri::command]
pub fn server_socket_path() -> String {
    poimen_client::protocol::socket_path()
        .to_string_lossy()
        .into_owned()
}

/// The `server.log` of the poimen-server this build talks to, next to its `server.json`.
#[tauri::command]
pub fn server_log_path() -> String {
    poimen_client::protocol::files::state_file_path()
        .with_file_name("server.log")
        .to_string_lossy()
        .into_owned()
}
