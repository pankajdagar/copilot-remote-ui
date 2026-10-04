pub mod migrations;
pub mod runtime_repository;

use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use rusqlite::Connection;

use crate::error::{AppError, Result};

pub struct Database(Mutex<Connection>);

impl Database {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let mut connection = Connection::open(path)?;
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        migrations::migrate(&mut connection)?;
        Ok(Self(Mutex::new(connection)))
    }

    pub fn connection(&self) -> Result<MutexGuard<'_, Connection>> {
        self.0
            .lock()
            .map_err(|_| AppError::InvalidInput("Database lock poisoned".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copilot::mcp;
    use crate::db::runtime_repository;
    use crate::sessions::repository;

    #[test]
    fn metadata_persists_and_rename_preserves_tmux_identity() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("nested/sessions.sqlite3");
        let db = Database::open(&path)?;
        let local = repository::list_hosts(&*db.connection()?)?
            .into_iter()
            .next()
            .ok_or_else(|| AppError::NotFound("Local host missing".into()))?;
        let workspace =
            repository::add_workspace(&*db.connection()?, &local.id, "/tmp/a repo", "A repo")?;
        let runtime = runtime_repository::reserve(
            &*db.connection()?,
            &local.id,
            41_000,
            "crui_copilot_host",
        )?;
        assert_eq!(runtime.remote_port, 41_000);
        let still_reserved = runtime_repository::reserve(
            &*db.connection()?,
            &local.id,
            42_000,
            "crui_copilot_duplicate",
        )?;
        assert_eq!(still_reserved.remote_port, runtime.remote_port);
        assert_eq!(still_reserved.tmux_session_name, runtime.tmux_session_name);
        let session = repository::add_session(
            &*db.connection()?,
            &workspace.id,
            "Original",
            "crui_01JTEST",
            "copilot",
        )?;
        repository::set_copilot_session_id(&*db.connection()?, &session.id, "copilot-session-1")?;
        repository::set_copilot_preferences(
            &*db.connection()?,
            &session.id,
            "claude-sonnet-5",
            Some("high"),
            Some("long_context"),
        )?;
        repository::rename(&*db.connection()?, &session.id, "New name")?;
        repository::pin(&*db.connection()?, &session.id, true)?;
        drop(db);

        let reopened = Database::open(&path)?;
        let sessions = repository::list_sessions(&*reopened.connection()?)?;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].name, "New name");
        assert_eq!(sessions[0].tmux_session_name, "crui_01JTEST");
        assert_eq!(
            sessions[0].copilot_session_id.as_deref(),
            Some("copilot-session-1")
        );
        assert!(!sessions[0].copilot_has_messages);
        assert_eq!(
            repository::get_copilot_model(&*reopened.connection()?, &session.id)?.as_deref(),
            Some("claude-sonnet-5")
        );
        let preferences =
            repository::get_copilot_preferences(&*reopened.connection()?, &session.id)?;
        assert_eq!(preferences.reasoning_effort.as_deref(), Some("high"));
        assert_eq!(preferences.context_tier.as_deref(), Some("long_context"));
        repository::mark_copilot_has_messages(&*reopened.connection()?, &session.id)?;
        assert!(
            repository::get_session(&*reopened.connection()?, &session.id)?.copilot_has_messages
        );
        assert!(sessions[0].pinned);
        assert_eq!(sessions[0].workspace.repo_path, "/tmp/a repo");
        assert_eq!(repository::list_hosts(&*reopened.connection()?)?.len(), 1);
        Ok(())
    }

    #[test]
    fn upgrades_existing_tmux_only_database_without_changing_sessions() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("legacy.sqlite3");
        let legacy = Connection::open(&path)?;
        legacy.execute_batch(
            "CREATE TABLE hosts (id TEXT PRIMARY KEY, name TEXT NOT NULL, ssh_host TEXT,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')));
             CREATE TABLE workspaces (id TEXT PRIMARY KEY, host_id TEXT NOT NULL, repo_path TEXT NOT NULL,
                display_name TEXT NOT NULL, created_at TEXT NOT NULL);
             CREATE TABLE sessions (id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL, name TEXT NOT NULL,
                tmux_session_name TEXT NOT NULL UNIQUE, command TEXT NOT NULL, pinned INTEGER NOT NULL,
                created_at TEXT NOT NULL, last_opened_at TEXT);
             INSERT INTO hosts VALUES ('host', 'Old host', 'alias', '2026-09-25T00:00:00Z');
             INSERT INTO workspaces VALUES ('repo', 'host', '/remote/repo', 'repo', '2026-09-25T00:00:00Z');
             INSERT INTO sessions VALUES ('session', 'repo', 'My work', 'crui_original', 'copilot',
                1, '2026-09-25T00:00:00Z', NULL);",
        )?;
        drop(legacy);

        let db = Database::open(&path)?;
        let session = repository::get_session(&*db.connection()?, "session")?;
        assert_eq!(session.name, "My work");
        assert_eq!(session.tmux_session_name, "crui_original");
        assert!(session.pinned);
        assert!(session.copilot_session_id.is_none());
        assert!(!session.copilot_has_messages);
        assert_eq!(
            repository::get_copilot_model(&*db.connection()?, "session")?,
            None
        );
        let version: i64 = db
            .connection()?
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        assert_eq!(version, 7);
        Ok(())
    }

    #[test]
    fn upgrades_sdk_enabled_database_without_losing_the_saved_id() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sdk-v1.sqlite3");
        let db = Database::open(&path)?;
        let host = repository::list_hosts(&*db.connection()?)?.remove(0);
        let workspace =
            repository::add_workspace(&*db.connection()?, &host.id, "/tmp/repo", "Repo")?;
        let session = repository::add_session(
            &*db.connection()?,
            &workspace.id,
            "Work",
            "crui_keep",
            "bash",
        )?;
        repository::set_copilot_session_id(&*db.connection()?, &session.id, "sdk-keep")?;
        db.connection()?.execute_batch(
            "ALTER TABLE sessions DROP COLUMN copilot_has_messages; PRAGMA user_version = 1;",
        )?;
        drop(db);
        let upgraded = Database::open(&path)?;
        let saved = repository::get_session(&*upgraded.connection()?, &session.id)?;
        assert_eq!(saved.copilot_session_id.as_deref(), Some("sdk-keep"));
        assert!(!saved.copilot_has_messages);
        assert_eq!(
            repository::get_copilot_model(&*upgraded.connection()?, &session.id)?,
            None
        );
        assert_eq!(saved.tmux_session_name, "crui_keep");
        Ok(())
    }

    #[test]
    fn upgrades_v2_sessions_and_persists_preferred_model_for_empty_chats() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sdk-v2.sqlite3");
        let db = Database::open(&path)?;
        let host = repository::list_hosts(&*db.connection()?)?.remove(0);
        let workspace =
            repository::add_workspace(&*db.connection()?, &host.id, "/tmp/repo", "Repo")?;
        let session = repository::add_session(
            &*db.connection()?,
            &workspace.id,
            "Work",
            "crui_model",
            "bash",
        )?;
        repository::set_copilot_session_id(&*db.connection()?, &session.id, "sdk-empty")?;
        db.connection()?.execute_batch(
            "ALTER TABLE sessions DROP COLUMN copilot_model; PRAGMA user_version = 2;",
        )?;
        drop(db);

        let upgraded = Database::open(&path)?;
        assert_eq!(
            repository::get_copilot_model(&*upgraded.connection()?, &session.id)?,
            None
        );
        repository::set_copilot_model(&*upgraded.connection()?, &session.id, "claude-sonnet-5")?;
        drop(upgraded);
        let reopened = Database::open(&path)?;
        let saved = repository::get_session(&*reopened.connection()?, &session.id)?;
        assert_eq!(saved.copilot_session_id.as_deref(), Some("sdk-empty"));
        assert!(!saved.copilot_has_messages);
        assert_eq!(
            repository::get_copilot_model(&*reopened.connection()?, &session.id)?.as_deref(),
            Some("claude-sonnet-5")
        );
        Ok(())
    }

    #[test]
    fn upgrades_v3_model_choice_with_optional_context_and_effort() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sdk-v3.sqlite3");
        let db = Database::open(&path)?;
        let host = repository::list_hosts(&*db.connection()?)?.remove(0);
        let workspace =
            repository::add_workspace(&*db.connection()?, &host.id, "/tmp/repo", "Repo")?;
        let session =
            repository::add_session(&*db.connection()?, &workspace.id, "Work", "crui_v3", "bash")?;
        repository::set_copilot_session_id(&*db.connection()?, &session.id, "sdk-empty")?;
        repository::set_copilot_model(&*db.connection()?, &session.id, "gpt-6-sol")?;
        db.connection()?.execute_batch(
            "ALTER TABLE sessions DROP COLUMN copilot_reasoning_effort;
             ALTER TABLE sessions DROP COLUMN copilot_context_tier;
             PRAGMA user_version = 3;",
        )?;
        drop(db);
        let upgraded = Database::open(&path)?;
        let saved = repository::get_copilot_preferences(&*upgraded.connection()?, &session.id)?;
        assert_eq!(saved.model.as_deref(), Some("gpt-6-sol"));
        assert_eq!(saved.reasoning_effort, None);
        assert_eq!(saved.context_tier, None);
        repository::set_copilot_preferences(
            &*upgraded.connection()?,
            &session.id,
            "gpt-6-sol",
            Some("xhigh"),
            Some("long_context"),
        )?;
        drop(upgraded);
        let reopened = Database::open(&path)?;
        let saved = repository::get_copilot_preferences(&*reopened.connection()?, &session.id)?;
        assert_eq!(saved.model.as_deref(), Some("gpt-6-sol"));
        assert_eq!(saved.reasoning_effort.as_deref(), Some("xhigh"));
        assert_eq!(saved.context_tier.as_deref(), Some("long_context"));
        assert!(saved.autopilot);
        repository::set_copilot_autopilot(&*reopened.connection()?, &session.id, false)?;
        assert!(
            !repository::get_copilot_preferences(&*reopened.connection()?, &session.id)?.autopilot
        );
        drop(reopened);
        let restarted = Database::open(&path)?;
        assert!(
            !repository::get_copilot_preferences(&*restarted.connection()?, &session.id)?.autopilot
        );
        Ok(())
    }

    #[test]
    fn upgrades_v4_sessions_to_autopilot_default_without_losing_saved_chat() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sdk-v4.sqlite3");
        let db = Database::open(&path)?;
        let host = repository::list_hosts(&*db.connection()?)?.remove(0);
        let workspace =
            repository::add_workspace(&*db.connection()?, &host.id, "/tmp/repo", "Repo")?;
        let session =
            repository::add_session(&*db.connection()?, &workspace.id, "Work", "crui_v4", "bash")?;
        repository::set_copilot_session_id(&*db.connection()?, &session.id, "sdk-saved")?;
        db.connection()?.execute_batch(
            "ALTER TABLE sessions DROP COLUMN copilot_autopilot; PRAGMA user_version = 4;",
        )?;
        drop(db);
        let upgraded = Database::open(&path)?;
        assert!(
            repository::get_copilot_preferences(&*upgraded.connection()?, &session.id)?.autopilot
        );
        assert_eq!(
            repository::get_session(&*upgraded.connection()?, &session.id)?
                .copilot_session_id
                .as_deref(),
            Some("sdk-saved")
        );
        Ok(())
    }

    #[test]
    fn upgrades_v6_with_reviewed_https_without_replacing_it_on_cli_import() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("sdk-v6.sqlite3");
        let db = Database::open(&path)?;
        let host = repository::list_hosts(&*db.connection()?)?.remove(0);
        mcp::add(
            &*db.connection()?,
            &host.id,
            "reviewed-http",
            "https://example.com/mcp",
        )?;
        db.connection()?
            .execute_batch("DROP TABLE reviewed_cli_mcp; PRAGMA user_version = 6;")?;
        drop(db);
        let upgraded = Database::open(&path)?;
        assert_eq!(mcp::list(&*upgraded.connection()?, &host.id)?.len(), 1);
        assert!(mcp::list_imports(&*upgraded.connection()?, &host.id)?.is_empty());
        mcp::save_import(&*upgraded.connection()?, &host.id, "example-tools", "ab01")?;
        drop(upgraded);
        let reopened = Database::open(&path)?;
        assert_eq!(mcp::list(&*reopened.connection()?, &host.id)?.len(), 1);
        assert_eq!(
            mcp::list_imports(&*reopened.connection()?, &host.id)?[0].name,
            "example-tools"
        );
        Ok(())
    }

    #[test]
    fn linking_cli_chat_preserves_tmux_and_never_steals_another_logical_session() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let db = Database::open(&dir.path().join("sessions.sqlite3"))?;
        let host = repository::list_hosts(&*db.connection()?)?.remove(0);
        let workspace =
            repository::add_workspace(&*db.connection()?, &host.id, "/tmp/repo", "Repo")?;
        let first = repository::add_session(
            &*db.connection()?,
            &workspace.id,
            "First",
            "crui_first_cli",
            "bash",
        )?;
        let second = repository::add_session(
            &*db.connection()?,
            &workspace.id,
            "Second",
            "crui_second_cli",
            "bash",
        )?;
        repository::set_copilot_session_id(&*db.connection()?, &first.id, "original")?;
        repository::set_copilot_preferences(
            &*db.connection()?,
            &first.id,
            "old-model",
            Some("high"),
            Some("long_context"),
        )?;
        repository::set_copilot_session_id(&*db.connection()?, &second.id, "already-linked")?;
        assert!(repository::link_existing_copilot_session(
            &*db.connection()?,
            &first.id,
            "already-linked",
        )
        .is_err());
        assert_eq!(
            repository::get_session(&*db.connection()?, &first.id)?
                .copilot_session_id
                .as_deref(),
            Some("original")
        );
        let linked = repository::link_existing_copilot_session(
            &*db.connection()?,
            &first.id,
            "cli-conversation",
        )?;
        assert_eq!(
            linked.copilot_session_id.as_deref(),
            Some("cli-conversation")
        );
        assert!(linked.copilot_has_messages);
        assert_eq!(linked.tmux_session_name, "crui_first_cli");
        assert_eq!(linked.name, "First");
        let preferences = repository::get_copilot_preferences(&*db.connection()?, &first.id)?;
        assert!(preferences.model.is_none());
        assert!(preferences.reasoning_effort.is_none());
        assert!(preferences.context_tier.is_none());
        Ok(())
    }
}
