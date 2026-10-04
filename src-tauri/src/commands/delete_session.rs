use std::sync::Arc;

use tauri::State;

use crate::error::Result;
use crate::sessions::manager::SessionManager;
use crate::sessions::model::Session;

#[tauri::command]
pub async fn delete_session(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
) -> Result<()> {
    let manager = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || manager.delete(&session_id)).await?
}

#[tauri::command]
pub fn forget_session(state: State<'_, Arc<SessionManager>>, session_id: String) -> Result<()> {
    state.forget(&session_id)
}

#[tauri::command]
pub async fn restart_session(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> Result<Session> {
    let manager = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || manager.restart(&session_id, cols, rows)).await?
}
