pub mod pty;
pub mod resize;
pub mod stream;

use std::collections::HashSet;

use serde::Serialize;
use tauri::ipc::Channel;

use crate::error::Result;
use crate::sessions::model::Host;
use crate::terminal::stream::TerminalEvent;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalDiagnostics {
    pub application_mouse: bool,
    pub alternate_screen: bool,
    pub pane_rows: u16,
    pub client_rows: Option<u16>,
    pub pane_count: u16,
}

pub trait TerminalBackend: Send + Sync {
    fn create(
        &self,
        host: &Host,
        name: &str,
        path: &str,
        command: &str,
        cols: u16,
        rows: u16,
    ) -> Result<()>;
    fn attach(&self, request: AttachRequest<'_>) -> Result<String>;
    fn reserve_attach(&self, session_id: &str) -> Result<u64>;
    fn write(&self, session_id: &str, connection_id: &str, bytes: &[u8]) -> Result<()>;
    fn resize(&self, session_id: &str, connection_id: &str, cols: u16, rows: u16) -> Result<()>;
    fn sync_size(
        &self,
        host: &Host,
        name: &str,
        session_id: &str,
        connection_id: &str,
        cols: u16,
        rows: u16,
    ) -> Result<TerminalDiagnostics>;
    fn scrollback(
        &self,
        host: &Host,
        name: &str,
        session_id: &str,
        connection_id: &str,
    ) -> Result<()>;
    fn disconnect(&self, session_id: &str, connection_id: &str) -> Result<()>;
    fn kill(&self, host: &Host, name: &str) -> Result<()>;
    fn list(&self, host: &Host) -> Result<HashSet<String>>;
    fn connected_session_ids(&self) -> Result<HashSet<String>>;
    fn shutdown(&self) -> Result<()>;
}

pub struct AttachRequest<'a> {
    pub host: &'a Host,
    pub session_id: &'a str,
    pub name: &'a str,
    pub generation: u64,
    pub cols: u16,
    pub rows: u16,
    pub channel: Channel<TerminalEvent>,
}
