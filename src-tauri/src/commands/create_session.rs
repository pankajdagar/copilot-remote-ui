use std::sync::Arc;

use tauri::State;

use crate::error::Result;
use crate::sessions::manager::SessionManager;
use crate::sessions::model::Session;

#[tauri::command]
pub async fn create_session(
    state: State<'_, Arc<SessionManager>>,
    workspace_id: String,
    name: String,
    command: String,
    cols: u16,
    rows: u16,
) -> Result<Session> {
    let manager = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || manager.create(&workspace_id, &name, &command, cols, rows))
        .await?
}
