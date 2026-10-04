use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::Mutex;

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use tauri::ipc::Channel;

use crate::error::{AppError, Result};
use crate::ssh::openssh;
use crate::terminal::resize::dimensions;
use crate::terminal::stream::TerminalEvent;

pub struct PtyConnection {
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child: Box<dyn Child + Send>,
}

impl PtyConnection {
    pub fn spawn(
        host_alias: Option<&str>,
        args: &[String],
        cols: u16,
        rows: u16,
        channel: Channel<TerminalEvent>,
    ) -> Result<Self> {
        dimensions(cols, rows)?;
        let pair = native_pty_system().openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;

        let mut command = if let Some(alias) = host_alias {
            let mut process = CommandBuilder::new("ssh");
            process.args(["-tt", "--", alias, &openssh::remote_command(args)]);
            process
        } else {
            let mut process = CommandBuilder::new(&args[0]);
            process.args(&args[1..]);
            process
        };
        command.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(command)?;
        drop(pair.slave);

        std::thread::spawn(move || {
            let mut buffer = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => {
                        let _ = channel.send(TerminalEvent::Closed {
                            message: "Terminal connection closed".into(),
                        });
                        break;
                    }
                    Ok(size) => {
                        if channel
                            .send(TerminalEvent::Output {
                                bytes: buffer[..size].to_vec(),
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "PTY read failed");
                        let _ = channel.send(TerminalEvent::Closed {
                            message: format!("Terminal connection lost: {error}"),
                        });
                        break;
                    }
                }
            }
        });
        Ok(Self {
            master: pair.master,
            writer,
            child,
        })
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        dimensions(cols, rows)?;
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    pub fn force_resize(&self, cols: u16, rows: u16) -> Result<()> {
        dimensions(cols, rows)?;
        let current = self.master.get_size()?;
        if current.cols == cols && current.rows == rows {
            let intermediate_cols = if cols > 2 { cols - 1 } else { cols + 1 };
            self.resize(intermediate_cols, rows)?;
        }
        self.resize(cols, rows)
    }

    pub fn close(mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
        }
        self.child.wait()?;
        Ok(())
    }
}

pub struct ActiveTerminal {
    pub connection_id: String,
    pub pty: PtyConnection,
}

pub struct TerminalSlot(pub Mutex<HashMap<String, ActiveTerminal>>);

impl TerminalSlot {
    pub fn with_connection<T>(
        &self,
        session_id: &str,
        connection_id: &str,
        operation: impl FnOnce(&mut PtyConnection) -> Result<T>,
    ) -> Result<T> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| AppError::InvalidInput("Terminal lock poisoned".into()))?;
        let active = guard
            .get_mut(session_id)
            .filter(|active| active.connection_id == connection_id);
        operation(
            &mut active
                .ok_or_else(|| AppError::NotFound("Terminal is not connected".into()))?
                .pty,
        )
    }

    pub fn disconnect(&self, session_id: &str, connection_id: &str) -> Result<()> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| AppError::InvalidInput("Terminal lock poisoned".into()))?;
        let active = if guard
            .get(session_id)
            .is_some_and(|active| active.connection_id == connection_id)
        {
            guard.remove(session_id)
        } else {
            None
        };
        drop(guard);
        if let Some(active) = active {
            active.pty.close()?;
        }
        Ok(())
    }

    pub fn connected_session_ids(&self) -> Result<HashSet<String>> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| AppError::InvalidInput("Terminal lock poisoned".into()))?;
        let mut connected = HashSet::new();
        for (session_id, active) in guard.iter_mut() {
            if active.pty.child.try_wait()?.is_none() {
                connected.insert(session_id.clone());
            }
        }
        Ok(connected)
    }

    pub fn shutdown(&self) -> Result<()> {
        let mut guard = self
            .0
            .lock()
            .map_err(|_| AppError::InvalidInput("Terminal lock poisoned".into()))?;
        let active: Vec<_> = guard.drain().map(|(_, active)| active).collect();
        drop(guard);
        let mut first_error = None;
        for connection in active {
            if let Err(error) = connection.pty.close() {
                if first_error.is_none() {
                    first_error = Some(error);
                } else {
                    tracing::warn!(%error, "Could not close additional PTY");
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

#[cfg(test)]
#[path = "pty_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "pty_cache_tests.rs"]
mod cache_tests;
