use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::error::Result;
use crate::sessions::manager::SessionManager;
use crate::terminal::stream::TerminalEvent;
use crate::terminal::AttachRequest;

#[tauri::command]
pub async fn attach_session(
    state: State<'_, Arc<SessionManager>>,
    session_id: String,
    cols: u16,
    rows: u16,
    on_data: Channel<TerminalEvent>,
) -> Result<String> {
    let manager = Arc::clone(state.inner());
    let request = manager.backend.reserve_attach(&session_id)?;
    tokio::task::spawn_blocking(move || {
        let session = manager.get(&session_id)?;
        let connection_id = manager.backend.attach(AttachRequest {
            host: &session.host,
            session_id: &session.id,
            name: &session.tmux_session_name,
            generation: request,
            cols,
            rows,
            channel: on_data,
        })?;
        if let Err(error) = manager.opened(&session_id) {
            if let Err(cleanup) = manager.backend.disconnect(&session_id, &connection_id) {
                tracing::warn!(%cleanup, "Could not close terminal after metadata error");
            }
            return Err(error);
        }
        Ok(connection_id)
    })
    .await?
}
