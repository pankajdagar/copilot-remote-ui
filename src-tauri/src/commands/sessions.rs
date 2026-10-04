use std::sync::Arc;

use tauri::State;

use crate::error::Result;
use crate::sessions::manager::SessionManager;
use crate::sessions::model::{Session, SessionWithStatus};

#[tauri::command]
pub async fn list_sessions(
    state: State<'_, Arc<SessionManager>>,
) -> Result<Vec<SessionWithStatus>> {
    let manager = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || manager.list()).await?
}

#[tauri::command]
pub fn touch_session(state: State<'_, Arc<SessionManager>>, session_id: String) -> Result<Session> {
    state.opened(&session_id)?;
    state.get(&session_id)
}
