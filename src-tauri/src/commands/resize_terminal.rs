use std::sync::Arc;

use tauri::State;

use crate::error::{AppError, Result};
use crate::sessions::manager::SessionManager;
use crate::terminal::TerminalDiagnostics;

#[tauri::command]
pub fn write_terminal(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    connection_id: String,
    bytes: Vec<u8>,
) -> Result<()> {
    if bytes.len() > 1024 * 1024 {
        return Err(AppError::InvalidInput(
            "Terminal input exceeds 1 MiB".into(),
        ));
    }
    state.backend.write(&session_id, &connection_id, &bytes)
}

#[tauri::command]
pub fn resize_terminal(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    connection_id: String,
    cols: u16,
    rows: u16,
) -> Result<()> {
    state
        .backend
        .resize(&session_id, &connection_id, cols, rows)
}

#[tauri::command]
pub async fn sync_terminal_size(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    connection_id: String,
    cols: u16,
    rows: u16,
) -> Result<TerminalDiagnostics> {
    let manager = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || {
        let session = manager.get(&session_id)?;
        manager.backend.sync_size(
            &session.host,
            &session.tmux_session_name,
            &session_id,
            &connection_id,
            cols,
            rows,
        )
    })
    .await?
}

#[tauri::command]
pub async fn enter_scrollback(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    connection_id: String,
) -> Result<()> {
    let manager = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || {
        let session = manager.get(&session_id)?;
        manager.backend.scrollback(
            &session.host,
            &session.tmux_session_name,
            &session_id,
            &connection_id,
        )
    })
    .await?
}

#[tauri::command]
pub fn disconnect_terminal(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    connection_id: String,
) -> Result<()> {
    state.backend.disconnect(&session_id, &connection_id)
}
