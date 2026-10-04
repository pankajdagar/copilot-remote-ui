use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, Result};
use crate::sessions::model::{Host, Session, Workspace};

fn host(row: &rusqlite::Row<'_>) -> rusqlite::Result<Host> {
    Ok(Host {
        id: row.get(0)?,
        name: row.get(1)?,
        ssh_host: row.get(2)?,
        created_at: row.get(3)?,
    })
}

fn workspace(row: &rusqlite::Row<'_>) -> rusqlite::Result<Workspace> {
    Ok(Workspace {
        id: row.get(0)?,
        host_id: row.get(1)?,
        repo_path: row.get(2)?,
        display_name: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn session(row: &rusqlite::Row<'_>) -> rusqlite::Result<Session> {
    Ok(Session {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        name: row.get(2)?,
        tmux_session_name: row.get(3)?,
        copilot_session_id: row.get(17)?,
        copilot_has_messages: row.get(18)?,
        command: row.get(4)?,
        pinned: row.get(5)?,
        created_at: row.get(6)?,
        last_opened_at: row.get(7)?,
        host: Host {
            id: row.get(8)?,
            name: row.get(9)?,
            ssh_host: row.get(10)?,
            created_at: row.get(11)?,
        },
        workspace: Workspace {
            id: row.get(12)?,
            host_id: row.get(13)?,
            repo_path: row.get(14)?,
            display_name: row.get(15)?,
            created_at: row.get(16)?,
        },
    })
}

const SESSION_QUERY: &str = "SELECT s.id, s.workspace_id, s.name, s.tmux_session_name,
    s.command, s.pinned, s.created_at, s.last_opened_at,
    h.id, h.name, h.ssh_host, h.created_at,
    w.id, w.host_id, w.repo_path, w.display_name, w.created_at,
    s.copilot_session_id, s.copilot_has_messages
    FROM sessions s JOIN workspaces w ON w.id = s.workspace_id
    JOIN hosts h ON h.id = w.host_id";

pub fn list_hosts(db: &Connection) -> Result<Vec<Host>> {
    let mut statement = db.prepare(
        "SELECT id, name, ssh_host, created_at FROM hosts ORDER BY ssh_host IS NOT NULL, name",
    )?;
    let hosts = statement
        .query_map([], host)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(hosts)
}

pub fn get_host(db: &Connection, id: &str) -> Result<Host> {
    db.query_row(
        "SELECT id, name, ssh_host, created_at FROM hosts WHERE id = ?1",
        [id],
        host,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound("Host not found".into()))
}

pub fn add_host(db: &Connection, name: &str, ssh_host: &str) -> Result<Host> {
    let id = ulid::Ulid::new().to_string();
    db.execute(
        "INSERT INTO hosts(id, name, ssh_host) VALUES (?1, ?2, ?3)",
        params![id, name, ssh_host],
    )?;
    get_host(db, &id)
}

pub fn list_workspaces(db: &Connection, host_id: &str) -> Result<Vec<Workspace>> {
    let mut statement = db.prepare(
        "SELECT id, host_id, repo_path, display_name, created_at
         FROM workspaces WHERE host_id = ?1 ORDER BY display_name",
    )?;
    let workspaces = statement
        .query_map([host_id], workspace)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(workspaces)
}

pub fn list_all_workspaces(db: &Connection) -> Result<Vec<Workspace>> {
    let mut statement = db.prepare(
        "SELECT id, host_id, repo_path, display_name, created_at
         FROM workspaces ORDER BY display_name, repo_path",
    )?;
    let workspaces = statement
        .query_map([], workspace)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(workspaces)
}

pub fn get_workspace(db: &Connection, id: &str) -> Result<Workspace> {
    db.query_row(
        "SELECT id, host_id, repo_path, display_name, created_at FROM workspaces WHERE id = ?1",
        [id],
        workspace,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound("Workspace not found".into()))
}

pub fn add_workspace(db: &Connection, host_id: &str, path: &str, name: &str) -> Result<Workspace> {
    let id = ulid::Ulid::new().to_string();
    db.execute(
        "INSERT INTO workspaces(id, host_id, repo_path, display_name) VALUES (?1, ?2, ?3, ?4)",
        params![id, host_id, path, name],
    )?;
    get_workspace(db, &id)
}

pub fn list_sessions(db: &Connection) -> Result<Vec<Session>> {
    let mut statement = db.prepare(&format!(
        "{SESSION_QUERY} ORDER BY s.pinned DESC, COALESCE(s.last_opened_at, s.created_at) DESC"
    ))?;
    let sessions = statement
        .query_map([], session)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(sessions)
}

pub fn get_session(db: &Connection, id: &str) -> Result<Session> {
    db.query_row(&format!("{SESSION_QUERY} WHERE s.id = ?1"), [id], session)
        .optional()?
        .ok_or_else(|| AppError::NotFound("Session not found".into()))
}

pub fn add_session(
    db: &Connection,
    workspace_id: &str,
    name: &str,
    tmux_name: &str,
    command: &str,
) -> Result<Session> {
    let id = ulid::Ulid::new().to_string();
    db.execute(
        "INSERT INTO sessions(id, workspace_id, name, tmux_session_name, command)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, workspace_id, name, tmux_name, command],
    )?;
    get_session(db, &id)
}

pub fn set_copilot_session_id(
    db: &Connection,
    id: &str,
    copilot_session_id: &str,
) -> Result<Session> {
    if db.execute(
        "UPDATE sessions SET copilot_session_id = ?1, copilot_has_messages = 0 WHERE id = ?2",
        params![copilot_session_id, id],
    )? == 0
    {
        return Err(AppError::NotFound("Session not found".into()));
    }
    get_session(db, id)
}

pub fn link_existing_copilot_session(
    db: &Connection,
    app_id: &str,
    copilot_id: &str,
) -> Result<Session> {
    let linked: Option<String> = db
        .query_row(
            "SELECT name FROM sessions WHERE copilot_session_id = ?1 AND id <> ?2",
            params![copilot_id, app_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(name) = linked {
        return Err(AppError::InvalidInput(format!(
            "This Copilot conversation is already linked to {name}"
        )));
    }
    if db.execute(
        "UPDATE sessions SET copilot_session_id = ?1, copilot_has_messages = 1,
         copilot_model = NULL, copilot_reasoning_effort = NULL, copilot_context_tier = NULL
         WHERE id = ?2",
        params![copilot_id, app_id],
    )? == 0
    {
        return Err(AppError::NotFound("Session not found".into()));
    }
    get_session(db, app_id)
}

pub fn mark_copilot_has_messages(db: &Connection, id: &str) -> Result<()> {
    if db.execute(
        "UPDATE sessions SET copilot_has_messages = 1
         WHERE id = ?1 AND copilot_session_id IS NOT NULL",
        [id],
    )? == 0
    {
        return Err(AppError::NotFound(
            "Copilot session record not found".into(),
        ));
    }
    Ok(())
}

pub struct CopilotPreferences {
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub context_tier: Option<String>,
    pub autopilot: bool,
}

pub fn get_copilot_preferences(db: &Connection, id: &str) -> Result<CopilotPreferences> {
    db.query_row(
        "SELECT copilot_model, copilot_reasoning_effort, copilot_context_tier, copilot_autopilot
         FROM sessions WHERE id = ?1",
        [id],
        |row| {
            Ok(CopilotPreferences {
                model: row.get(0)?,
                reasoning_effort: row.get(1)?,
                context_tier: row.get(2)?,
                autopilot: row.get(3)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound("Session not found".into()))
}

pub fn set_copilot_autopilot(db: &Connection, id: &str, enabled: bool) -> Result<()> {
    if db.execute(
        "UPDATE sessions SET copilot_autopilot = ?1
         WHERE id = ?2 AND copilot_session_id IS NOT NULL",
        params![enabled, id],
    )? == 0
    {
        return Err(AppError::NotFound(
            "Copilot session record not found".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
pub fn get_copilot_model(db: &Connection, id: &str) -> Result<Option<String>> {
    Ok(get_copilot_preferences(db, id)?.model)
}

#[cfg(test)]
pub fn set_copilot_model(db: &Connection, id: &str, model_id: &str) -> Result<()> {
    set_copilot_preferences(db, id, model_id, None, None)
}

pub fn set_copilot_preferences(
    db: &Connection,
    id: &str,
    model_id: &str,
    reasoning_effort: Option<&str>,
    context_tier: Option<&str>,
) -> Result<()> {
    if db.execute(
        "UPDATE sessions SET copilot_model = ?1, copilot_reasoning_effort = ?2,
         copilot_context_tier = ?3 WHERE id = ?4 AND copilot_session_id IS NOT NULL",
        params![model_id, reasoning_effort, context_tier, id],
    )? == 0
    {
        return Err(AppError::NotFound(
            "Copilot session record not found".into(),
        ));
    }
    Ok(())
}

pub fn rename(db: &Connection, id: &str, name: &str) -> Result<Session> {
    if db.execute(
        "UPDATE sessions SET name = ?1 WHERE id = ?2",
        params![name, id],
    )? == 0
    {
        return Err(AppError::NotFound("Session not found".into()));
    }
    get_session(db, id)
}

pub fn pin(db: &Connection, id: &str, pinned: bool) -> Result<Session> {
    if db.execute(
        "UPDATE sessions SET pinned = ?1 WHERE id = ?2",
        params![pinned, id],
    )? == 0
    {
        return Err(AppError::NotFound("Session not found".into()));
    }
    get_session(db, id)
}

pub fn opened(db: &Connection, id: &str) -> Result<()> {
    db.execute(
        "UPDATE sessions SET last_opened_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
        [id],
    )?;
    Ok(())
}

pub fn delete(db: &Connection, id: &str) -> Result<()> {
    if db.execute("DELETE FROM sessions WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound("Session not found".into()));
    }
    Ok(())
}
