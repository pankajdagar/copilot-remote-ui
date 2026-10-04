use serde::ser::{Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Operation(#[from] anyhow::Error),
    #[error("Background task failed: {0}")]
    Task(#[from] tokio::task::JoinError),
    #[error("SSH tunnel to {host} failed: {reason}")]
    SshTunnel { host: String, reason: String },
    #[error("Chat tunnel to {host} is paused; resume it from Ports to reconnect")]
    ChatPaused { host: String },
    #[error("Copilot headless on {host} is unavailable: {reason}")]
    CopilotRuntime { host: String, reason: String },
    #[error("Copilot session could not be resumed: {0}")]
    CopilotResume(String),
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;
