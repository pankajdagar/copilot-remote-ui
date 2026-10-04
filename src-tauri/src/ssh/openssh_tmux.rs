use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::error::{AppError, Result};
use crate::sessions::model::Host;
use crate::terminal::pty::{ActiveTerminal, PtyConnection, TerminalSlot};
use crate::terminal::resize::dimensions;
use crate::terminal::{AttachRequest, TerminalBackend, TerminalDiagnostics};
use crate::tmux::{commands, session};

pub struct OpenSshTmuxBackend {
    active: TerminalSlot,
    next_request: AtomicU64,
    latest_requests: Mutex<HashMap<String, u64>>,
}

impl OpenSshTmuxBackend {
    pub fn new() -> Self {
        Self {
            active: TerminalSlot(Mutex::new(HashMap::new())),
            next_request: AtomicU64::new(0),
            latest_requests: Mutex::new(HashMap::new()),
        }
    }
}

impl TerminalBackend for OpenSshTmuxBackend {
    fn create(
        &self,
        host: &Host,
        name: &str,
        path: &str,
        command: &str,
        cols: u16,
        rows: u16,
    ) -> Result<()> {
        dimensions(cols, rows)?;
        let argv = shell_words::split(command)
            .map_err(|error| AppError::InvalidInput(format!("Invalid command: {error}")))?;
        if argv.is_empty() || argv[0].is_empty() {
            return Err(AppError::InvalidInput(
                "Command must contain a program".into(),
            ));
        }
        session::create(host, name, path, &argv, cols, rows)
    }

    fn attach(&self, request: AttachRequest<'_>) -> Result<String> {
        let AttachRequest {
            host,
            session_id,
            name,
            generation,
            cols,
            rows,
            channel,
        } = request;
        dimensions(cols, rows)?;
        session::set_interaction_policy(host, name)?;
        let args = commands::attach(name);
        let mut guard = self
            .active
            .0
            .lock()
            .map_err(|_| AppError::InvalidInput("Terminal lock poisoned".into()))?;
        let latest = self
            .latest_requests
            .lock()
            .map_err(|_| AppError::InvalidInput("Attach request lock poisoned".into()))?
            .get(session_id)
            .copied();
        if latest != Some(generation) {
            return Err(AppError::InvalidInput(
                "Attach request was superseded".into(),
            ));
        }
        if let Some(previous) = guard.remove(session_id) {
            previous.pty.close()?;
        }
        let pty = PtyConnection::spawn(host.ssh_host.as_deref(), &args, cols, rows, channel)?;
        let connection_id = ulid::Ulid::new().to_string();
        guard.insert(
            session_id.into(),
            ActiveTerminal {
                connection_id: connection_id.clone(),
                pty,
            },
        );
        tracing::info!(%session_id, %connection_id, "Attached terminal client");
        Ok(connection_id)
    }

    fn reserve_attach(&self, session_id: &str) -> Result<u64> {
        let request = self.next_request.fetch_add(1, Ordering::SeqCst) + 1;
        self.latest_requests
            .lock()
            .map_err(|_| AppError::InvalidInput("Attach request lock poisoned".into()))?
            .insert(session_id.into(), request);
        Ok(request)
    }

    fn write(&self, session_id: &str, connection_id: &str, bytes: &[u8]) -> Result<()> {
        self.active
            .with_connection(session_id, connection_id, |pty| {
                pty.writer.write_all(bytes)?;
                pty.writer.flush()?;
                Ok(())
            })
    }

    fn resize(&self, session_id: &str, connection_id: &str, cols: u16, rows: u16) -> Result<()> {
        self.active
            .with_connection(session_id, connection_id, |pty| pty.resize(cols, rows))
    }

    fn sync_size(
        &self,
        host: &Host,
        name: &str,
        session_id: &str,
        connection_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<TerminalDiagnostics> {
        self.active
            .with_connection(session_id, connection_id, |pty| {
                pty.force_resize(cols, rows)
            })?;
        session::set_interaction_policy(host, name)?;
        session::inspect(host, name)
    }

    fn scrollback(
        &self,
        host: &Host,
        name: &str,
        session_id: &str,
        connection_id: &str,
    ) -> Result<()> {
        self.active
            .with_connection(session_id, connection_id, |_| Ok(()))?;
        session::enter_scrollback(host, name)
    }

    fn disconnect(&self, session_id: &str, connection_id: &str) -> Result<()> {
        self.active.disconnect(session_id, connection_id)?;
        tracing::info!(%session_id, %connection_id, "Detached terminal client");
        Ok(())
    }

    fn kill(&self, host: &Host, name: &str) -> Result<()> {
        session::kill(host, name)
    }

    fn list(&self, host: &Host) -> Result<HashSet<String>> {
        session::list(host)
    }

    fn connected_session_ids(&self) -> Result<HashSet<String>> {
        self.active.connected_session_ids()
    }

    fn shutdown(&self) -> Result<()> {
        self.active.shutdown()
    }
}
