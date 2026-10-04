use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::*;
use crate::terminal::AttachRequest;

struct StatusBackend {
    active: Mutex<HashSet<String>>,
}

impl TerminalBackend for StatusBackend {
    fn create(&self, _: &Host, _: &str, _: &str, _: &str, _: u16, _: u16) -> Result<()> {
        panic!("Restoring must not create a session")
    }
    fn reserve_attach(&self, _: &str) -> Result<u64> {
        panic!("Restoring must not attach")
    }
    fn attach(&self, _: AttachRequest<'_>) -> Result<String> {
        panic!("Restoring must not attach")
    }
    fn write(&self, _: &str, _: &str, _: &[u8]) -> Result<()> {
        panic!("Restoring must not write")
    }
    fn resize(&self, _: &str, _: &str, _: u16, _: u16) -> Result<()> {
        panic!("Restoring must not resize")
    }
    fn sync_size(
        &self,
        _: &Host,
        _: &str,
        _: &str,
        _: &str,
        _: u16,
        _: u16,
    ) -> Result<crate::terminal::TerminalDiagnostics> {
        panic!("Restoring must not resize")
    }
    fn scrollback(&self, _: &Host, _: &str, _: &str, _: &str) -> Result<()> {
        panic!("Restoring must not enter scrollback")
    }
    fn disconnect(&self, _: &str, _: &str) -> Result<()> {
        panic!("Restoring must not disconnect")
    }
    fn kill(&self, _: &Host, _: &str) -> Result<()> {
        panic!("Restoring must not kill")
    }
    fn list(&self, host: &Host) -> Result<HashSet<String>> {
        if host.ssh_host.is_some() {
            Err(AppError::Operation(anyhow::anyhow!("SSH unavailable")))
        } else {
            Ok(HashSet::from(["crui_live".to_owned()]))
        }
    }
    fn connected_session_ids(&self) -> Result<HashSet<String>> {
        Ok(self
            .active
            .lock()
            .map_err(|_| AppError::InvalidInput("Test lock".into()))?
            .clone())
    }
    fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[test]
fn restore_reconciles_by_host_without_mutating_runtime() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = Database::open(&dir.path().join("sessions.sqlite3"))?;
    let local = repository::list_hosts(&*db.connection()?)?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Local host missing".into()))?;
    let remote = repository::add_host(&*db.connection()?, "Remote", "remote-01")?;
    let local_workspace =
        repository::add_workspace(&*db.connection()?, &local.id, "/tmp/repo", "Repo")?;
    let remote_workspace = repository::add_workspace(
        &*db.connection()?,
        &remote.id,
        "/home/user/repo",
        "Remote repo",
    )?;
    let live = repository::add_session(
        &*db.connection()?,
        &local_workspace.id,
        "Live",
        "crui_live",
        "copilot",
    )?;
    let missing = repository::add_session(
        &*db.connection()?,
        &local_workspace.id,
        "Missing",
        "crui_missing",
        "copilot",
    )?;
    let offline = repository::add_session(
        &*db.connection()?,
        &remote_workspace.id,
        "Offline",
        "crui_offline",
        "copilot",
    )?;
    let backend = Arc::new(StatusBackend {
        active: Mutex::new(HashSet::from([live.id.clone()])),
    });
    let manager = SessionManager {
        db,
        backend: backend.clone(),
    };
    let snapshot = manager.list()?;
    let statuses: HashMap<_, _> = snapshot
        .iter()
        .map(|item| (item.session.id.clone(), &item.status))
        .collect();
    assert!(matches!(
        statuses.get(&live.id),
        Some(SessionStatus::Running)
    ));
    assert!(matches!(
        statuses.get(&missing.id),
        Some(SessionStatus::Dead)
    ));
    assert!(matches!(
        statuses.get(&offline.id),
        Some(SessionStatus::HostUnavailable)
    ));
    let live_record = snapshot
        .iter()
        .find(|item| item.session.id == live.id)
        .ok_or_else(|| AppError::NotFound("Live session missing".into()))?;
    let wire = serde_json::to_value(live_record).map_err(anyhow::Error::from)?;
    assert_eq!(wire["id"], live.id);
    assert_eq!(wire["status"], "running");
    assert!(wire["copilotSessionId"].is_null());
    assert_eq!(wire["copilotHasMessages"], false);
    assert_eq!(wire["workspace"]["repoPath"], "/tmp/repo");

    backend
        .active
        .lock()
        .map_err(|_| AppError::InvalidInput("Test lock".into()))?
        .clear();
    let statuses: HashMap<_, _> = manager
        .list()?
        .into_iter()
        .map(|item| (item.session.id, item.status))
        .collect();
    assert!(matches!(
        statuses.get(&live.id),
        Some(SessionStatus::Disconnected)
    ));
    Ok(())
}

#[test]
fn forgetting_a_session_does_not_contact_or_kill_its_host() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let db = Database::open(&dir.path().join("sessions.sqlite3"))?;
    let local = repository::list_hosts(&*db.connection()?)?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Local host missing".into()))?;
    let workspace = repository::add_workspace(&*db.connection()?, &local.id, "/tmp/repo", "Repo")?;
    let session = repository::add_session(
        &*db.connection()?,
        &workspace.id,
        "Forget me",
        "crui_forget",
        "copilot",
    )?;
    let manager = SessionManager {
        db,
        backend: Arc::new(StatusBackend {
            active: Mutex::new(HashSet::new()),
        }),
    };
    manager.forget(&session.id)?;
    assert!(manager.get(&session.id).is_err());
    Ok(())
}
