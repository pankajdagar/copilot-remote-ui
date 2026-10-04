use std::sync::Arc;

use tauri::State;

use crate::error::Result;
use crate::git::status::{self, ChangedFile};
use crate::sessions::manager::SessionManager;

#[tauri::command]
pub async fn list_changes(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
) -> Result<Vec<ChangedFile>> {
    let manager = Arc::clone(state.inner());
    tokio::task::spawn_blocking(move || {
        let session = manager.get(&session_id)?;
        status::list(&session.host, &session.workspace.repo_path)
    })
    .await?
}
