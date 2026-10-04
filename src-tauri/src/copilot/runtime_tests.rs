use std::net::TcpListener;
use std::process::Command;
use std::time::Duration;

use github_copilot_sdk::rpc::ModeSetRequest;
use github_copilot_sdk::session_events::SessionMode;
use github_copilot_sdk::{
    permission, Client, ClientOptions, ResumeSessionConfig, SessionConfig, SessionId, Transport,
};

use super::RemoteCopilotRuntime;
use crate::error::{AppError, Result};

struct TestRuntime(String);

impl TestRuntime {
    fn alive(&self) -> Result<bool> {
        Ok(Command::new("tmux")
            .args(["-L", &self.0, "has-session", "-t", "=crui_headless_test"])
            .status()?
            .success())
    }
}

impl Drop for TestRuntime {
    fn drop(&mut self) {
        if let Err(error) = Command::new("tmux")
            .args(["-L", &self.0, "kill-server"])
            .output()
        {
            eprintln!("Could not stop test headless tmux server: {error}");
        }
    }
}

#[tokio::test]
#[ignore = "requires the official Copilot CLI and tmux"]
async fn sdk_connects_to_external_loopback_headless_runtime() -> Result<()> {
    let home = tempfile::tempdir()?;
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let socket = format!("crui_sdk_test_{}", ulid::Ulid::new());
    let runtime = TestRuntime(socket.clone());
    let created = Command::new("tmux")
        .args([
            "-L",
            &socket,
            "-f",
            "/dev/null",
            "new-session",
            "-d",
            "-s",
            "crui_headless_test",
            "-c",
        ])
        .arg(home.path())
        .args([
            "-x",
            "80",
            "-y",
            "24",
            "--",
            "copilot",
            "--headless",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .env("HOME", home.path())
        .env_remove("COPILOT_CUSTOM_INSTRUCTIONS_DIRS")
        .output()?;
    if !created.status.success() {
        return Err(AppError::CopilotRuntime {
            host: "local test".into(),
            reason: "Could not start headless CLI inside isolated tmux".into(),
        });
    }
    let mut available = false;
    for _ in 0..50 {
        if !runtime.alive()? {
            return Err(AppError::CopilotRuntime {
                host: "local test".into(),
                reason: "Headless CLI exited before listening".into(),
            });
        }
        if tokio::net::TcpStream::connect(("127.0.0.1", port))
            .await
            .is_ok()
        {
            available = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    if !available {
        return Err(AppError::CopilotRuntime {
            host: "local test".into(),
            reason: "Headless CLI did not open its loopback port".into(),
        });
    }
    let client = tokio::time::timeout(
        Duration::from_secs(10),
        Client::start(
            ClientOptions::new()
                .with_program(RemoteCopilotRuntime::system_cli_path()?)
                .with_transport(Transport::External {
                    host: "127.0.0.1".into(),
                    port,
                    connection_token: None,
                }),
        ),
    )
    .await
    .map_err(anyhow::Error::from)?
    .map_err(|error| AppError::Operation(anyhow::anyhow!("SDK handshake failed: {error}")))?;
    client
        .ping(Some("copilot-remote-ui"))
        .await
        .map_err(|error| AppError::Operation(anyhow::anyhow!("SDK ping failed: {error}")))?;
    let session_id = SessionId::new(format!("remote-chat-{}", ulid::Ulid::new()));
    let created = client
        .create_session(
            SessionConfig::default()
                .with_session_id(session_id.clone())
                .with_enable_session_store(true)
                .with_working_directory(home.path())
                .with_permission_handler(permission::deny_all()),
        )
        .await
        .map_err(|error| AppError::Operation(anyhow::anyhow!("SDK create failed: {error}")))?;
    let sdk_id = created.id().to_string();
    assert_eq!(created.id(), session_id);
    let second_repo = tempfile::tempdir()?;
    let empty_id = SessionId::new(format!("remote-empty-{}", ulid::Ulid::new()));
    let prepared = client
        .prepare_session(
            SessionConfig::default()
                .with_session_id(empty_id.clone())
                .with_enable_session_store(true)
                .with_working_directory(second_repo.path())
                .with_permission_handler(permission::deny_all()),
        )
        .map_err(|error| AppError::Operation(anyhow::anyhow!("SDK prepare failed: {error}")))?;
    let mut startup = prepared.subscribe();
    let empty = prepared.start().await.map_err(|error| {
        AppError::Operation(anyhow::anyhow!("SDK empty create failed: {error}"))
    })?;
    assert_eq!(empty.id(), empty_id);
    let _empty_history = empty.get_events().await.map_err(|error| {
        AppError::Operation(anyhow::anyhow!(
            "New Copilot session history could not be loaded: {error}"
        ))
    })?;
    let mut reported_cwd = None;
    for _ in 0..30 {
        match tokio::time::timeout(Duration::from_millis(300), startup.recv()).await {
            Ok(Ok(event)) if event.event_type == "session.start" => {
                reported_cwd = event
                    .data
                    .pointer("/context/cwd")
                    .and_then(serde_json::Value::as_str)
                    .map(std::path::PathBuf::from);
                if reported_cwd.is_some() {
                    break;
                }
            }
            Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {}
        }
    }
    let reported_cwd = reported_cwd.ok_or_else(|| AppError::CopilotRuntime {
        host: "local test".into(),
        reason: "SDK did not expose its resolved session working directory".into(),
    })?;
    assert_eq!(
        std::fs::canonicalize(reported_cwd)?,
        std::fs::canonicalize(second_repo.path())?,
        "Copilot resolved tools against a different working directory"
    );
    let original_mode = empty
        .rpc()
        .mode()
        .get()
        .await
        .map_err(|error| AppError::Operation(anyhow::anyhow!("Mode query failed: {error}")))?;
    empty
        .rpc()
        .mode()
        .set(ModeSetRequest {
            mode: SessionMode::Autopilot,
            expected_mode: Some(original_mode),
            ..Default::default()
        })
        .await
        .map_err(|error| {
            AppError::Operation(anyhow::anyhow!(
                "Autopilot mode could not be set on the headless CLI: {error}"
            ))
        })?;
    assert!(matches!(
        empty
            .rpc()
            .mode()
            .get()
            .await
            .map_err(|error| AppError::Operation(anyhow::anyhow!(
                "Autopilot mode could not be queried: {error}"
            )))?,
        SessionMode::Autopilot
    ));
    let metadata = client
        .get_session_metadata(&session_id)
        .await
        .map_err(|error| AppError::Operation(anyhow::anyhow!("SDK metadata failed: {error}")))?;
    if metadata.is_none() {
        let mut events = created.subscribe();
        tokio::time::timeout(Duration::from_secs(30), created.send("Reply with OK."))
            .await
            .map_err(anyhow::Error::from)?
            .map_err(|_error| AppError::CopilotRuntime {
                host: "local test".into(),
                reason: "Cannot send a harmless test prompt to initialize session storage".into(),
            })?;
        for _ in 0..60 {
            match tokio::time::timeout(Duration::from_millis(500), events.recv()).await {
                Ok(Ok(event)) if event.event_type == "session.idle" => break,
                Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {}
            }
        }
        let stored = client
            .get_session_metadata(&session_id)
            .await
            .map_err(|error| {
                AppError::Operation(anyhow::anyhow!("SDK metadata failed: {error}"))
            })?;
        assert!(
            stored.is_some(),
            "SDK session was not persisted after its first turn"
        );
    }
    client
        .stop()
        .await
        .map_err(|error| AppError::Operation(anyhow::anyhow!("SDK detach failed: {error}")))?;
    assert!(
        runtime.alive()?,
        "Detaching the SDK killed the headless server"
    );
    let second = Client::start(
        ClientOptions::new()
            .with_program(RemoteCopilotRuntime::system_cli_path()?)
            .with_transport(Transport::External {
                host: "127.0.0.1".into(),
                port,
                connection_token: None,
            }),
    )
    .await
    .map_err(|error| AppError::Operation(anyhow::anyhow!("SDK reconnect failed: {error}")))?;
    let prepared_resume = second
        .prepare_resume_session(
            ResumeSessionConfig::new(SessionId::new(sdk_id.as_str()))
                .with_enable_session_store(true)
                .with_working_directory(home.path())
                .with_permission_handler(permission::deny_all()),
        )
        .map_err(|error| {
            AppError::Operation(anyhow::anyhow!("SDK prepare resume failed: {error}"))
        })?;
    let mut resume_events = prepared_resume.subscribe();
    let resumed = prepared_resume
        .start()
        .await
        .map_err(|error| AppError::Operation(anyhow::anyhow!("SDK resume failed: {error}")))?;
    assert_eq!(resumed.id().as_ref(), sdk_id);
    let mut resume_cwd = None;
    for _ in 0..30 {
        match tokio::time::timeout(Duration::from_millis(300), resume_events.recv()).await {
            Ok(Ok(event)) if event.event_type == "session.resume" => {
                resume_cwd = event
                    .data
                    .pointer("/context/cwd")
                    .and_then(serde_json::Value::as_str)
                    .map(std::path::PathBuf::from);
                if resume_cwd.is_some() {
                    break;
                }
            }
            Ok(Ok(_)) | Ok(Err(_)) | Err(_) => {}
        }
    }
    let resume_cwd = resume_cwd.ok_or_else(|| {
        AppError::CopilotResume("SDK did not report the resumed working directory".into())
    })?;
    assert_eq!(
        std::fs::canonicalize(resume_cwd)?,
        std::fs::canonicalize(home.path())?
    );
    let history = resumed
        .get_events()
        .await
        .map_err(|error| AppError::Operation(anyhow::anyhow!("SDK history failed: {error}")))?;
    assert!(
        history
            .iter()
            .any(|event| event.event_type == "session.start"),
        "Resumed SDK session did not restore its stored lifecycle events"
    );
    let empty_after_restart = match second
        .resume_session(
            ResumeSessionConfig::new(empty_id.clone())
                .with_enable_session_store(true)
                .with_working_directory(second_repo.path())
                .with_permission_handler(permission::deny_all()),
        )
        .await
    {
        Ok(session) => session,
        Err(_) => second
            .create_session(
                SessionConfig::default()
                    .with_session_id(empty_id.clone())
                    .with_enable_session_store(true)
                    .with_working_directory(home.path())
                    .with_permission_handler(permission::deny_all()),
            )
            .await
            .map_err(|error| {
                AppError::Operation(anyhow::anyhow!(
                    "Could not safely reopen empty SDK session: {error}"
                ))
            })?,
    };
    assert_eq!(empty_after_restart.id(), empty_id);
    second.stop().await.map_err(|error| {
        AppError::Operation(anyhow::anyhow!("SDK second detach failed: {error}"))
    })?;
    Ok(())
}
