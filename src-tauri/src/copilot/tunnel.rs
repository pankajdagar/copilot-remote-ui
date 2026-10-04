use std::io::Read;
use std::net::TcpListener;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::error::{AppError, Result};
use crate::ssh::openssh;

const MAX_SSH_DIAGNOSTIC_BYTES: usize = 4096;

pub struct LocalTunnel {
    pub local_port: u16,
    child: Child,
    stderr: Arc<Mutex<Vec<u8>>>,
    stderr_done: mpsc::Receiver<()>,
}

impl LocalTunnel {
    fn ssh_command(alias: &str, local_port: u16, remote_port: u16) -> Command {
        let forward = format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}");
        let mut command = Command::new("ssh");
        command
            .args([
                "-N",
                "-T",
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=8",
                // Other forwards inherited from the SSH alias may already be bound.
                "-o",
                "ExitOnForwardFailure=no",
                "-L",
            ])
            .arg(forward)
            .arg("--")
            .arg(alias)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        command
    }

    fn from_child(local_port: u16, mut child: Child) -> Result<Self> {
        let mut pipe = match child.stderr.take() {
            Some(pipe) => pipe,
            None => {
                if child.try_wait()?.is_none() {
                    child.kill()?;
                }
                child.wait()?;
                return Err(AppError::InvalidInput(
                    "SSH tunnel was started without an error stream".into(),
                ));
            }
        };
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&stderr);
        let (sender, stderr_done) = mpsc::channel();
        std::thread::spawn(move || {
            let mut chunk = [0u8; 1024];
            loop {
                match pipe.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(size) => {
                        let Ok(mut output) = captured.lock() else {
                            tracing::error!("SSH tunnel diagnostic buffer lock poisoned");
                            break;
                        };
                        output.extend_from_slice(&chunk[..size]);
                        if output.len() > MAX_SSH_DIAGNOSTIC_BYTES {
                            let excess = output.len() - MAX_SSH_DIAGNOSTIC_BYTES;
                            output.drain(..excess);
                        }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Could not read SSH tunnel diagnostics");
                        break;
                    }
                }
            }
            let _ = sender.send(());
        });
        Ok(Self {
            local_port,
            child,
            stderr,
            stderr_done,
        })
    }

    pub fn diagnostics(&self) -> Result<Option<String>> {
        let bytes = self.stderr.lock().map_err(|_| {
            AppError::InvalidInput("SSH tunnel diagnostic buffer lock poisoned".into())
        })?;
        let text: String = String::from_utf8_lossy(&bytes)
            .chars()
            .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
            .collect();
        let text = text.trim();
        Ok((!text.is_empty()).then(|| text.to_owned()))
    }

    fn exit_reason(&self, status: ExitStatus) -> Result<String> {
        let _ = self.stderr_done.recv_timeout(Duration::from_millis(250));
        Ok(match self.diagnostics()? {
            Some(details) => format!("SSH exited with {status}: {details}"),
            None => format!(
                "SSH exited with {status} without diagnostics. Terminal SSH access does not guarantee non-interactive port forwarding"
            ),
        })
    }

    pub async fn open(alias: &str, remote_port: u16) -> Result<Self> {
        openssh::validate_alias(alias)?;
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let local_port = listener.local_addr()?.port();
        drop(listener);
        Self::open_on_port(alias, local_port, remote_port).await
    }

    async fn open_on_port(alias: &str, local_port: u16, remote_port: u16) -> Result<Self> {
        let child = Self::ssh_command(alias, local_port, remote_port).spawn()?;
        let mut tunnel = Self::from_child(local_port, child)?;
        tunnel.wait_ready(alias, remote_port).await?;
        Ok(tunnel)
    }

    async fn wait_ready(&mut self, alias: &str, remote_port: u16) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            if let Some(status) = self.child.try_wait()? {
                return Err(AppError::SshTunnel {
                    host: alias.to_owned(),
                    reason: self.exit_reason(status)?,
                });
            }
            if tokio::net::TcpStream::connect(("127.0.0.1", self.local_port))
                .await
                .is_ok()
            {
                tracing::info!(%alias, local_port = self.local_port, %remote_port, "SSH loopback tunnel ready");
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(AppError::SshTunnel {
                    host: alias.to_owned(),
                    reason: match self.diagnostics()? {
                        Some(details) => {
                            format!("Timed out opening the local forwarded port: {details}")
                        }
                        None => "Timed out opening the local forwarded port".into(),
                    },
                });
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    pub fn is_alive(&mut self) -> Result<bool> {
        Ok(self.child.try_wait()?.is_none())
    }

    pub fn close(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            self.child.kill()?;
        }
        self.child.wait()?;
        Ok(())
    }
}

impl Drop for LocalTunnel {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::warn!(%error, "Could not stop local SSH tunnel");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tunnel_ignores_unrelated_forward_collisions_but_requests_its_own_loopback_port() {
        let command = LocalTunnel::ssh_command("configured-host", 49200, 27000);
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-o", "ExitOnForwardFailure=no"]));
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-L", "127.0.0.1:49200:127.0.0.1:27000"]));
        assert_eq!(&args[args.len() - 2..], ["--", "configured-host"]);
    }

    #[tokio::test]
    async fn shows_the_real_ssh_failure_instead_of_guessing_about_credentials() -> Result<()> {
        let child = Command::new("sh")
            .args([
                "-c",
                "printf 'channel open failed: administratively prohibited\\n' >&2; exit 255",
            ])
            .stderr(Stdio::piped())
            .spawn()?;
        let mut tunnel = LocalTunnel::from_child(0, child)?;
        let error = tunnel
            .wait_ready("configured-host", 49200)
            .await
            .expect_err("the local test process exits before a tunnel opens");
        match error {
            AppError::SshTunnel { host, reason } => {
                assert_eq!(host, "configured-host");
                assert!(reason.contains("exit status: 255"));
                assert!(reason.contains("administratively prohibited"));
                assert!(!reason.contains("check your configured host and credentials"));
            }
            other => panic!("expected SSH tunnel failure, got {other}"),
        }
        Ok(())
    }

    #[test]
    fn reports_authentication_failures_without_terminal_escape_sequences() -> Result<()> {
        let child = Command::new("sh")
            .args([
                "-c",
                "printf '\\033[31mPermission denied (publickey)\\033[0m\\n' >&2; exit 255",
            ])
            .stderr(Stdio::piped())
            .spawn()?;
        let mut tunnel = LocalTunnel::from_child(0, child)?;
        let status = tunnel.child.wait()?;
        let reason = tunnel.exit_reason(status)?;
        assert!(reason.contains("Permission denied (publickey)"));
        assert!(!reason.contains('\u{1b}'));
        Ok(())
    }

    #[test]
    fn bounds_the_ssh_error_to_recent_diagnostics() -> Result<()> {
        let child = Command::new("sh")
            .args([
                "-c",
                "printf '%5000s' x >&2; printf ' final SSH error\\n' >&2; exit 255",
            ])
            .stderr(Stdio::piped())
            .spawn()?;
        let mut tunnel = LocalTunnel::from_child(0, child)?;
        let status = tunnel.child.wait()?;
        let reason = tunnel.exit_reason(status)?;
        assert!(reason.contains("final SSH error"));
        assert!(tunnel
            .diagnostics()?
            .is_some_and(|text| text.len() <= MAX_SSH_DIAGNOSTIC_BYTES));
        Ok(())
    }
}
