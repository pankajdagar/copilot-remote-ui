use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, Result};

pub fn migrate(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS hosts (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            ssh_host TEXT UNIQUE,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
        );
        CREATE UNIQUE INDEX IF NOT EXISTS one_local_host ON hosts ((ssh_host IS NULL))
            WHERE ssh_host IS NULL;
        CREATE TABLE IF NOT EXISTS workspaces (
            id TEXT PRIMARY KEY,
            host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE RESTRICT,
            repo_path TEXT NOT NULL,
            display_name TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            UNIQUE(host_id, repo_path)
        );
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE RESTRICT,
            name TEXT NOT NULL,
            tmux_session_name TEXT NOT NULL UNIQUE,
            command TEXT NOT NULL,
            pinned INTEGER NOT NULL DEFAULT 0 CHECK(pinned IN (0, 1)),
            created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
            last_opened_at TEXT
        );
        CREATE INDEX IF NOT EXISTS sessions_recent ON sessions(pinned DESC, last_opened_at DESC);",
    )?;
    let version: i64 = transaction.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > 7 {
        return Err(AppError::Operation(anyhow::anyhow!(
            "Session database version {version} is newer than this app supports"
        )));
    }
    if version == 0 {
        let mut columns = transaction.prepare("PRAGMA table_info(sessions)")?;
        let names = columns
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(columns);
        if !names.iter().any(|name| name == "copilot_session_id") {
            transaction.execute_batch("ALTER TABLE sessions ADD COLUMN copilot_session_id TEXT")?;
        }
        transaction.execute_batch(
            "CREATE UNIQUE INDEX IF NOT EXISTS sessions_copilot_id
                ON sessions(copilot_session_id) WHERE copilot_session_id IS NOT NULL;
             CREATE TABLE IF NOT EXISTS copilot_runtimes (
                host_id TEXT PRIMARY KEY REFERENCES hosts(id) ON DELETE RESTRICT,
                remote_port INTEGER NOT NULL CHECK(remote_port BETWEEN 1024 AND 65535),
                tmux_session_name TEXT NOT NULL UNIQUE,
                created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             );",
        )?;
        transaction.pragma_update(None, "user_version", 1)?;
    }
    if version < 2 {
        let mut columns = transaction.prepare("PRAGMA table_info(sessions)")?;
        let names = columns
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(columns);
        if !names.iter().any(|name| name == "copilot_has_messages") {
            transaction.execute_batch(
                "ALTER TABLE sessions ADD COLUMN copilot_has_messages INTEGER NOT NULL DEFAULT 0
                 CHECK(copilot_has_messages IN (0, 1))",
            )?;
        }
        transaction.pragma_update(None, "user_version", 2)?;
    }
    if version < 3 {
        let mut columns = transaction.prepare("PRAGMA table_info(sessions)")?;
        let names = columns
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(columns);
        if !names.iter().any(|name| name == "copilot_model") {
            transaction.execute_batch("ALTER TABLE sessions ADD COLUMN copilot_model TEXT")?;
        }
        transaction.pragma_update(None, "user_version", 3)?;
    }
    if version < 4 {
        let mut columns = transaction.prepare("PRAGMA table_info(sessions)")?;
        let names = columns
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(columns);
        if !names.iter().any(|name| name == "copilot_reasoning_effort") {
            transaction
                .execute_batch("ALTER TABLE sessions ADD COLUMN copilot_reasoning_effort TEXT")?;
        }
        if !names.iter().any(|name| name == "copilot_context_tier") {
            transaction
                .execute_batch("ALTER TABLE sessions ADD COLUMN copilot_context_tier TEXT")?;
        }
        transaction.pragma_update(None, "user_version", 4)?;
    }
    if version < 5 {
        let mut columns = transaction.prepare("PRAGMA table_info(sessions)")?;
        let names = columns
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(columns);
        if !names.iter().any(|name| name == "copilot_autopilot") {
            transaction.execute_batch(
                "ALTER TABLE sessions ADD COLUMN copilot_autopilot INTEGER NOT NULL DEFAULT 1
                 CHECK(copilot_autopilot IN (0, 1))",
            )?;
        }
        transaction.pragma_update(None, "user_version", 5)?;
    }
    if version < 6 {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS reviewed_mcp_servers (
                host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE RESTRICT,
                name TEXT NOT NULL,
                url TEXT NOT NULL,
                reviewed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                PRIMARY KEY(host_id, name)
            )",
        )?;
        transaction.pragma_update(None, "user_version", 6)?;
    }
    if version < 7 {
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS reviewed_cli_mcp (
                host_id TEXT NOT NULL REFERENCES hosts(id) ON DELETE RESTRICT,
                name TEXT NOT NULL,
                config_sha256 TEXT NOT NULL,
                reviewed_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                PRIMARY KEY(host_id, name)
            )",
        )?;
        transaction.pragma_update(None, "user_version", 7)?;
    }
    let local: Option<String> = transaction
        .query_row("SELECT id FROM hosts WHERE ssh_host IS NULL", [], |row| {
            row.get(0)
        })
        .optional()?;
    if local.is_none() {
        transaction.execute(
            "INSERT INTO hosts (id, name, ssh_host) VALUES (?1, 'Local', NULL)",
            params![ulid::Ulid::new().to_string()],
        )?;
    }
    transaction.commit()?;
    Ok(())
}
