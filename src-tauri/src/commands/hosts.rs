use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::error::Result;
use crate::sessions::manager::SessionManager;
use crate::sessions::model::{Host, Workspace};
use crate::sessions::{discovery, repository};

#[tauri::command]
pub fn list_hosts(app: AppHandle, state: State<'_, Arc<SessionManager>>) -> Result<Vec<Host>> {
    let home = app.path().home_dir().map_err(anyhow::Error::from)?;
    discovery::sync(&mut *state.db.connection()?, &home)?;
    repository::list_hosts(&*state.db.connection()?)
}

#[tauri::command]
pub fn list_all_workspaces(state: State<'_, Arc<SessionManager>>) -> Result<Vec<Workspace>> {
    repository::list_all_workspaces(&*state.db.connection()?)
}
#[tauri::command]
pub fn add_host(
    state: State<'_, Arc<SessionManager>>,
    name: String,
    ssh_host: String,
) -> Result<Host> {
    state.add_host(&name, &ssh_host)
}

#[tauri::command]
pub fn list_workspaces(
    state: State<'_, Arc<SessionManager>>,
    host_id: String,
) -> Result<Vec<Workspace>> {
    repository::list_workspaces(&*state.db.connection()?, &host_id)
}

#[tauri::command]
pub fn add_workspace(
    state: State<'_, Arc<SessionManager>>,
    host_id: String,
    repo_path: String,
    display_name: String,
) -> Result<Workspace> {
    state.add_workspace(&host_id, &repo_path, &display_name)
}
