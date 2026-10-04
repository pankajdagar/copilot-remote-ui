use std::sync::Arc;

use tauri::State;

use crate::error::Result;
use crate::sessions::manager::SessionManager;
use crate::sessions::model::Session;

#[tauri::command]
pub fn rename_session(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    name: String,
) -> Result<Session> {
    state.rename(&session_id, &name)
}

#[tauri::command]
pub fn pin_session(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    pinned: bool,
) -> Result<Session> {
    state.pin(&session_id, pinned)
}
