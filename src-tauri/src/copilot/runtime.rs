use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::Output;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use github_copilot_sdk::{Client, ClientOptions, Transport};
use tokio::sync::Mutex;

use crate::db::runtime_repository::{self, RuntimeRecord};
use crate::error::{AppError, Result};
use crate::sessions::manager::SessionManager;
use crate::sessions::model::Host;
use crate::ssh::manager;
use crate::tmux::{commands, session as tmux_session};

use super::ports::{self, ChatForward, PortInventory};
use super::tunnel::LocalTunnel;

struct HostRuntime {
    client: Option<Client>,
    tunnel: Option<LocalTunnel>,
}

pub struct RemoteCopilotRuntime {
    sessions: Arc<SessionManager>,
    hosts: Mutex<HashMap<String, Arc<Mutex<HostRuntime>>>>,
    paused: Mutex<HashSet<String>>,
}

impl RemoteCopilotRuntime {
    fn connection_error(host: &Host, tunnel: &LocalTunnel, reason: &str) -> Result<AppError> {
        let details = tunnel
            .diagnostics()?
            .map(|stderr| format!("; SSH reported: {stderr}"))
            .unwrap_or_default();
        Ok(AppError::CopilotRuntime {
            host: host.name.clone(),
            reason: format!("{reason}{details}"),
        })
    }

    fn system_cli_path() -> Result<PathBuf> {
        if let Some(path) = std::env::var_os("COPILOT_CLI_PATH") {
            let path = PathBuf::from(path);
            if path.is_file() {
                return Ok(path);
            }
            return Err(AppError::CopilotRuntime {
                host: "local desktop".into(),
                reason: "COPILOT_CLI_PATH does not point to an installed Copilot CLI".into(),
            });
        }
        let mut candidates = Vec::new();
        #[cfg(target_os = "macos")]
        {
            candidates.push(PathBuf::from("/opt/homebrew/bin/copilot"));
            candidates.push(PathBuf::from("/usr/local/bin/copilot"));
        }
        if let Some(path) = std::env::var_os("PATH") {
            candidates.extend(
                std::env::split_paths(&path)
                    .filter(|directory| directory.is_absolute())
                    .map(|directory| directory.join("copilot")),
            );
        }
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            candidates.push(home.join(".volta/bin/copilot"));
            candidates.push(home.join(".local/bin/copilot"));
        }
        candidates.into_iter().find(|path| path.is_file()).ok_or_else(|| {
            AppError::CopilotRuntime {
                host: "local desktop".into(),
                reason: "Rust SDK 1.0.14 requires a local Copilot CLI path even for an external server; set COPILOT_CLI_PATH to an approved installation".into(),
            }
        })
    }

    pub fn new(sessions: Arc<SessionManager>) -> Self {
        Self {
            sessions,
            hosts: Mutex::new(HashMap::new()),
            paused: Mutex::new(HashSet::new()),
        }
    }

    pub async fn ensure(&self, host: &Host) -> Result<Client> {
        if self.paused.lock().await.contains(&host.id) {
            return Err(AppError::ChatPaused {
                host: host.name.clone(),
            });
        }
        let alias = host.ssh_host.as_deref().ok_or_else(|| {
            AppError::InvalidInput("Copilot chat requires a remote host in this version".into())
        })?;
        let local_program = Self::system_cli_path()?;
        let entry = {
            let mut hosts = self.hosts.lock().await;
            hosts
                .entry(host.id.clone())
                .or_insert_with(|| {
                    Arc::new(Mutex::new(HostRuntime {
                        client: None,
                        tunnel: None,
                    }))
                })
                .clone()
        };
        let mut runtime = entry.lock().await;
        if self.paused.lock().await.contains(&host.id) {
            return Err(AppError::ChatPaused {
                host: host.name.clone(),
            });
        }
        if let Some(client) = runtime.client.clone() {
            let tunnel_alive = match runtime.tunnel.as_mut() {
                Some(tunnel) => tunnel.is_alive()?,
                None => false,
            };
            if tunnel_alive {
                match tokio::time::timeout(Duration::from_secs(5), client.ping(None)).await {
                    Ok(Ok(_)) => return Ok(client),
                    Ok(Err(_)) | Err(_) => {
                        tracing::warn!(host = %host.name, "Copilot headless health check failed");
                    }
                }
            }
        }
        if let Some(client) = runtime.client.take() {
            if !matches!(
                tokio::time::timeout(Duration::from_secs(3), client.stop()).await,
                Ok(Ok(()))
            ) {
                tracing::warn!(host = %host.name, "Could not detach old Copilot SDK client");
            }
        }
        runtime.tunnel.take();

        let record = self.runtime_record(host)?;
        let host_for_list = host.clone();
        let names =
            tokio::task::spawn_blocking(move || tmux_session::list(&host_for_list)).await??;
        if !names.contains(&record.tmux_session_name) {
            let host_for_start = host.clone();
            let record_for_start = record.clone();
            tokio::task::spawn_blocking(move || {
                Self::start_server(&host_for_start, &record_for_start)
            })
            .await??;
        }

        let tunnel = LocalTunnel::open(alias, record.remote_port).await?;
        let transport = Transport::External {
            host: "127.0.0.1".into(),
            port: tunnel.local_port,
            connection_token: None,
        };
        let client = match tokio::time::timeout(
            Duration::from_secs(15),
            Client::start(
                ClientOptions::new()
                    .with_program(local_program)
                    .with_transport(transport),
            ),
        )
        .await
        {
            Ok(Ok(client)) => client,
            Ok(Err(_error)) => {
                return Err(Self::connection_error(
                    host,
                    &tunnel,
                    "SDK handshake failed; check the remote Copilot CLI version and authentication",
                )?);
            }
            Err(_) => {
                return Err(Self::connection_error(
                    host,
                    &tunnel,
                    "Timed out connecting the SDK to the headless server",
                )?);
            }
        };
        match tokio::time::timeout(Duration::from_secs(5), client.ping(None)).await {
            Ok(Ok(_)) => {}
            Ok(Err(_error)) => {
                return Err(Self::connection_error(
                    host,
                    &tunnel,
                    "Headless server health check failed",
                )?);
            }
            Err(_) => {
                return Err(Self::connection_error(
                    host,
                    &tunnel,
                    "Headless server health check timed out",
                )?);
            }
        }
        tracing::info!(host = %host.name, "Copilot SDK connected through loopback tunnel");
        runtime.tunnel = Some(tunnel);
        runtime.client = Some(client.clone());
        Ok(client)
    }

    pub async fn connect_session_host(&self, app_session_id: &str) -> Result<()> {
        let session = self.sessions.get(app_session_id)?;
        self.ensure(&session.host).await?;
        Ok(())
    }

    pub async fn is_healthy(&self, host_id: &str) -> Result<bool> {
        let entry = self.hosts.lock().await.get(host_id).cloned();
        let Some(entry) = entry else {
            return Ok(false);
        };
        let mut runtime = entry.lock().await;
        let Some(tunnel) = runtime.tunnel.as_mut() else {
            return Ok(false);
        };
        if !tunnel.is_alive()? {
            return Ok(false);
        }
        let Some(client) = runtime.client.as_ref() else {
            return Ok(false);
        };
        Ok(matches!(
            tokio::time::timeout(Duration::from_secs(5), client.ping(None)).await,
            Ok(Ok(_))
        ))
    }

    pub async fn disconnect_host(&self, host_id: &str) -> Result<()> {
        let entry = self.hosts.lock().await.get(host_id).cloned();
        if let Some(entry) = entry {
            let mut runtime = entry.lock().await;
            let stopped = match runtime.client.take() {
                Some(client) => {
                    match tokio::time::timeout(Duration::from_secs(3), client.stop()).await {
                        Ok(Ok(())) => Ok(()),
                        _ => Err(AppError::CopilotRuntime {
                            host: host_id.into(),
                            reason: "Could not detach SDK sessions promptly".into(),
                        }),
                    }
                }
                None => Ok(()),
            };
            if let Some(mut tunnel) = runtime.tunnel.take() {
                tunnel.close()?;
            }
            stopped?;
        }
        Ok(())
    }

    pub async fn pause_host(&self, host_id: &str) -> Result<()> {
        self.paused.lock().await.insert(host_id.to_owned());
        self.disconnect_host(host_id).await
    }

    pub async fn resume_host(&self, app_session_id: &str) -> Result<()> {
        let host = self.sessions.get(app_session_id)?.host;
        self.paused.lock().await.remove(&host.id);
        Ok(())
    }

    pub async fn ports(&self, app_session_id: &str) -> Result<PortInventory> {
        let host = self.sessions.get(app_session_id)?.host;
        let configured = ports::configured(host.ssh_host.as_deref()).await?;
        let remote_port = runtime_repository::get(&*self.sessions.db.connection()?, &host.id)?
            .map(|record| record.remote_port);
        let entry = self.hosts.lock().await.get(&host.id).cloned();
        let local_port = if let Some(entry) = entry {
            let mut runtime = entry.lock().await;
            match runtime.tunnel.as_mut() {
                Some(tunnel) => {
                    if tunnel.is_alive()? {
                        Some(tunnel.local_port)
                    } else {
                        None
                    }
                }
                _ => None,
            }
        } else {
            None
        };
        let paused = self.paused.lock().await.contains(&host.id);
        Ok(PortInventory {
            chat: ChatForward {
                local_port,
                remote_port,
                paused,
            },
            configured,
        })
    }

    fn runtime_record(&self, host: &Host) -> Result<RuntimeRecord> {
        let db = self.sessions.db.connection()?;
        if let Some(record) = runtime_repository::get(&db, &host.id)? {
            return Ok(record);
        }
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(anyhow::Error::from)?
            .as_nanos();
        let port = 20_000 + (nanos % 35_000) as u16;
        runtime_repository::reserve(&db, &host.id, port, &format!("crui_copilot_{}", host.id))
    }

    fn start_server(host: &Host, record: &RuntimeRecord) -> Result<()> {
        let version = manager::command(host, &["copilot".into(), "--version".into()]).output()?;
        if !version.status.success() {
            if version.status.code() == Some(255) {
                return Err(AppError::SshTunnel {
                    host: host.name.clone(),
                    reason: "Could not check Copilot through the configured SSH host".into(),
                });
            }
            return Err(AppError::CopilotRuntime {
                host: host.name.clone(),
                reason: "Copilot CLI is not available to noninteractive SSH on this host".into(),
            });
        }
        let argv = vec![
            "copilot".into(),
            "--headless".into(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            record.remote_port.to_string(),
        ];
        let output: Output = manager::command(
            host,
            &commands::create(&record.tmux_session_name, "/", &argv, 80, 24),
        )
        .output()?;
        if !output.status.success() {
            return Err(AppError::CopilotRuntime {
                host: host.name.clone(),
                reason: format!(
                    "Could not start headless Copilot in tmux ({})",
                    output.status
                ),
            });
        }
        tracing::info!(host_id = %record.host_id, remote_port = record.remote_port, "Started Copilot headless runtime");
        Ok(())
    }

    pub async fn close_local_tunnels(&self) -> Result<()> {
        let entries: Vec<_> = self.hosts.lock().await.values().cloned().collect();
        for entry in entries {
            let mut runtime = entry.lock().await;
            if let Some(mut tunnel) = runtime.tunnel.take() {
                tunnel.close()?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
