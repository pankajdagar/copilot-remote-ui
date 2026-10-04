use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, Result};

#[derive(Clone, Debug)]
pub struct RuntimeRecord {
    pub host_id: String,
    pub remote_port: u16,
    pub tmux_session_name: String,
}

pub fn get(db: &Connection, host_id: &str) -> Result<Option<RuntimeRecord>> {
    db.query_row(
        "SELECT host_id, remote_port, tmux_session_name FROM copilot_runtimes WHERE host_id = ?1",
        [host_id],
        |row| {
            Ok(RuntimeRecord {
                host_id: row.get(0)?,
                remote_port: row.get(1)?,
                tmux_session_name: row.get(2)?,
            })
        },
    )
    .optional()
    .map_err(AppError::from)
}

pub fn reserve(db: &Connection, host_id: &str, port: u16, name: &str) -> Result<RuntimeRecord> {
    db.execute(
        "INSERT INTO copilot_runtimes (host_id, remote_port, tmux_session_name)
         VALUES (?1, ?2, ?3) ON CONFLICT(host_id) DO NOTHING",
        params![host_id, port, name],
    )?;
    get(db, host_id)?.ok_or_else(|| AppError::NotFound("Copilot runtime record missing".into()))
}
