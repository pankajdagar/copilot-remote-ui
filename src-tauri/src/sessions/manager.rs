use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::sessions::model::{Host, Session, SessionStatus, SessionWithStatus, Workspace};
use crate::sessions::repository;
use crate::ssh::openssh;
use crate::terminal::TerminalBackend;

pub struct SessionManager {
    pub db: Database,
    pub backend: Arc<dyn TerminalBackend>,
}

fn nonempty(value: &str, label: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > 200 {
        return Err(AppError::InvalidInput(format!(
            "{label} must be 1 to 200 characters"
        )));
    }
    Ok(value.to_owned())
}

impl SessionManager {
    pub fn add_host(&self, name: &str, alias: &str) -> Result<Host> {
        let name = nonempty(name, "Host name")?;
        let alias = nonempty(alias, "SSH alias")?;
        openssh::validate_alias(&alias)?;
        repository::add_host(&*self.db.connection()?, &name, &alias)
    }

    pub fn add_workspace(&self, host_id: &str, path: &str, name: &str) -> Result<Workspace> {
        let path = nonempty(path, "Repository path")?;
        let db = self.db.connection()?;
        let host = repository::get_host(&db, host_id)?;
        let absolute = if host.ssh_host.is_some() {
            path.starts_with('/')
        } else {
            Path::new(&path).is_absolute()
        };
        if !absolute {
            return Err(AppError::InvalidInput(
                "Use an absolute repository path".into(),
            ));
        }
        let name = nonempty(name, "Repository name")?;
        repository::add_workspace(&db, host_id, &path, &name)
    }

    pub fn create(
        &self,
        workspace_id: &str,
        name: &str,
        command: &str,
        cols: u16,
        rows: u16,
    ) -> Result<Session> {
        let name = nonempty(name, "Session name")?;
        let command = nonempty(command, "Command")?;
        let (workspace, host) = {
            let db = self.db.connection()?;
            let workspace = repository::get_workspace(&db, workspace_id)?;
            let host = repository::get_host(&db, &workspace.host_id)?;
            (workspace, host)
        };
        let tmux_name = format!("crui_{}", ulid::Ulid::new());
        self.backend.create(
            &host,
            &tmux_name,
            &workspace.repo_path,
            &command,
            cols,
            rows,
        )?;
        tracing::info!(%tmux_name, host = %host.name, "Created persistent tmux session");
        match repository::add_session(
            &*self.db.connection()?,
            workspace_id,
            &name,
            &tmux_name,
            &command,
        ) {
            Ok(session) => Ok(session),
            Err(error) => {
                if let Err(cleanup) = self.backend.kill(&host, &tmux_name) {
                    tracing::error!(%cleanup, %tmux_name, "Failed to clean up unrecorded tmux session");
                }
                Err(error)
            }
        }
    }

    pub fn list(&self) -> Result<Vec<SessionWithStatus>> {
        let sessions = repository::list_sessions(&*self.db.connection()?)?;
        let connected = self.backend.connected_session_ids()?;
        let mut hosts = HashMap::new();
        for session in &sessions {
            hosts
                .entry(session.host.id.clone())
                .and_modify(|(_, all_connected): &mut (Host, bool)| {
                    *all_connected &= connected.contains(&session.id)
                })
                .or_insert_with(|| (session.host.clone(), connected.contains(&session.id)));
        }
        let available = std::thread::scope(|scope| -> Result<HashMap<_, _>> {
            let checks: Vec<_> = hosts
                .into_iter()
                .filter(|(_, (_, all_connected))| !all_connected)
                .map(|(id, (host, _))| {
                    scope.spawn(move || (id, host.name.clone(), self.backend.list(&host)))
                })
                .collect();
            let mut available = HashMap::new();
            for check in checks {
                let (id, name, result) = check.join().map_err(|_| {
                    AppError::Operation(anyhow::anyhow!("Host status worker panicked"))
                })?;
                match result {
                    Ok(names) => {
                        available.insert(id, Some(names));
                    }
                    Err(error) => {
                        tracing::warn!(host = %name, %error, "Cannot reconcile tmux sessions");
                        available.insert(id, None);
                    }
                }
            }
            Ok(available)
        })?;
        Ok(sessions
            .into_iter()
            .map(|session| {
                let status = if connected.contains(&session.id) {
                    SessionStatus::Running
                } else {
                    match available.get(&session.host.id) {
                        Some(Some(names)) if names.contains(&session.tmux_session_name) => {
                            SessionStatus::Disconnected
                        }
                        Some(Some(_)) => SessionStatus::Dead,
                        _ => SessionStatus::HostUnavailable,
                    }
                };
                SessionWithStatus { session, status }
            })
            .collect())
    }

    pub fn get(&self, id: &str) -> Result<Session> {
        repository::get_session(&*self.db.connection()?, id)
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<Session> {
        let name = nonempty(name, "Session name")?;
        repository::rename(&*self.db.connection()?, id, &name)
    }

    pub fn pin(&self, id: &str, pinned: bool) -> Result<Session> {
        repository::pin(&*self.db.connection()?, id, pinned)
    }

    pub fn opened(&self, id: &str) -> Result<()> {
        repository::opened(&*self.db.connection()?, id)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let session = self.get(id)?;
        let names = self.backend.list(&session.host)?;
        if names.contains(&session.tmux_session_name) {
            self.backend
                .kill(&session.host, &session.tmux_session_name)?;
        }
        repository::delete(&*self.db.connection()?, id)?;
        tracing::info!(session_id = %id, "Deleted logical session");
        Ok(())
    }

    pub fn forget(&self, id: &str) -> Result<()> {
        repository::delete(&*self.db.connection()?, id)?;
        tracing::info!(session_id = %id, "Forgot logical session without stopping tmux");
        Ok(())
    }

    pub fn restart(&self, id: &str, cols: u16, rows: u16) -> Result<Session> {
        let session = self.get(id)?;
        let names = self.backend.list(&session.host)?;
        if names.contains(&session.tmux_session_name) {
            self.backend
                .kill(&session.host, &session.tmux_session_name)?;
        }
        self.backend.create(
            &session.host,
            &session.tmux_session_name,
            &session.workspace.repo_path,
            &session.command,
            cols,
            rows,
        )?;
        Ok(session)
    }
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
