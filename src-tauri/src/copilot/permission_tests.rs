use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use github_copilot_sdk::handler::{PermissionHandler, PermissionResult};
use github_copilot_sdk::{PermissionRequestData, RequestId, SessionId};
use tauri::ipc::{Channel, InvokeResponseBody};

use crate::db::Database;
use crate::sessions::manager::SessionManager;
use crate::sessions::repository;
use crate::ssh::openssh_tmux::OpenSshTmuxBackend;

use super::*;

#[tokio::test]
async fn shell_permission_waits_for_a_user_response() -> Result<()> {
    let seen = Arc::new(StdMutex::new(Vec::<serde_json::Value>::new()));
    let sink = seen.clone();
    let channel = Channel::new(move |body| {
        if let InvokeResponseBody::Json(json) = body {
            let event = serde_json::from_str(&json)?;
            sink.lock().expect("test event lock").push(event);
        }
        Ok(())
    });
    let pending = Arc::new(PendingPermissions(Mutex::new(HashMap::new())));
    let handler = UiPermissionHandler {
        channel,
        pending: pending.clone(),
        working_directory: "/remote/repo".into(),
        active: Arc::new(AtomicBool::new(true)),
        allow_all: Arc::new(AtomicBool::new(false)),
        autopilot: Arc::new(AtomicBool::new(false)),
        reviewed_mcp: Arc::new(StdMutex::new(HashMap::new())),
        reviewed_mcp_grant: Arc::new(AtomicBool::new(false)),
        client: None,
    };
    let shell = serde_json::json!({
        "kind": "shell",
        "fullCommandText": "npm test",
        "intention": "Run repository tests",
        "warning": "Review this command",
        "managedSettingsEnabled": false
    });
    let mut data: PermissionRequestData =
        serde_json::from_value(shell.clone()).map_err(anyhow::Error::from)?;
    data.extra = serde_json::json!({
        "requestId": "request-1",
        "permissionRequest": shell
    });
    let task = tokio::spawn(async move {
        handler
            .handle(SessionId::new("sdk-1"), RequestId::new("request-1"), data)
            .await
    });
    for _ in 0..30 {
        if !seen.lock().expect("test event lock").is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    {
        let events = seen.lock().expect("test event lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["type"], "permissionRequested");
        assert_eq!(events[0]["command"], "npm test");
        assert_eq!(events[0]["description"], "Run repository tests");
        assert_eq!(events[0]["warning"], "Review this command");
        assert_eq!(events[0]["approvable"], true);
    }
    assert!(
        !task.is_finished(),
        "Permission was decided without user input"
    );
    let sender = pending
        .0
        .lock()
        .await
        .remove("request-1")
        .ok_or_else(|| AppError::NotFound("Pending permission missing".into()))?;
    assert!(sender.displayable);
    sender
        .sender
        .send(PermissionResult::reject(Some("Denied in test".into())))
        .map_err(|_| AppError::Operation(anyhow::anyhow!("Could not deny test request")))?;
    let result = task.await?;
    assert!(format!("{result:?}").contains("Reject"));
    Ok(())
}

#[tokio::test]
async fn confirmed_allow_all_autoapproves_displayed_shell_action_once() -> Result<()> {
    let seen = Arc::new(StdMutex::new(Vec::<serde_json::Value>::new()));
    let sink = seen.clone();
    let channel = Channel::new(move |body| {
        if let InvokeResponseBody::Json(json) = body {
            sink.lock()
                .expect("test event lock")
                .push(serde_json::from_str(&json)?);
        }
        Ok(())
    });
    let handler = UiPermissionHandler {
        channel,
        pending: Arc::new(PendingPermissions(Mutex::new(HashMap::new()))),
        working_directory: "/remote/repo".into(),
        active: Arc::new(AtomicBool::new(true)),
        allow_all: Arc::new(AtomicBool::new(true)),
        autopilot: Arc::new(AtomicBool::new(false)),
        reviewed_mcp: Arc::new(StdMutex::new(HashMap::new())),
        reviewed_mcp_grant: Arc::new(AtomicBool::new(false)),
        client: None,
    };
    let shell = serde_json::json!({
        "kind": "shell", "fullCommandText": "npm test",
        "managedSettingsEnabled": false
    });
    let mut data: PermissionRequestData =
        serde_json::from_value(shell.clone()).map_err(anyhow::Error::from)?;
    data.extra = serde_json::json!({ "requestId": "request-3", "permissionRequest": shell });
    let result = handler
        .handle(SessionId::new("sdk-1"), RequestId::new("request-3"), data)
        .await;
    assert!(format!("{result:?}").contains("ApproveOnce"));
    let events = seen.lock().expect("test event lock");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["type"], "permissionAutoApproved");
    assert_eq!(events[0]["command"], "npm test");
    assert_eq!(events[0]["source"], "allowAll");
    Ok(())
}

#[tokio::test]
async fn hidden_chat_never_approves_or_prompts_for_a_tool() -> Result<()> {
    let handler = UiPermissionHandler {
        channel: Channel::new(|_| panic!("Hidden chat must not receive an approval request")),
        pending: Arc::new(PendingPermissions(Mutex::new(HashMap::new()))),
        working_directory: "/remote/repo".into(),
        active: Arc::new(AtomicBool::new(false)),
        allow_all: Arc::new(AtomicBool::new(false)),
        autopilot: Arc::new(AtomicBool::new(false)),
        reviewed_mcp: Arc::new(StdMutex::new(HashMap::new())),
        reviewed_mcp_grant: Arc::new(AtomicBool::new(false)),
        client: None,
    };
    let data: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "shell", "fullCommandText": "rm -rf build/",
        "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    let result = handler
        .handle(SessionId::new("sdk-1"), RequestId::new("request-2"), data)
        .await;
    assert!(format!("{result:?}").contains("UserNotAvailable"));
    Ok(())
}

#[test]
fn shell_permission_displays_every_command_before_allowing_approval() -> Result<()> {
    let legacy: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "shell", "command": "npm test", "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    let rendered = render_permission(&legacy, "Shell");
    assert_eq!(rendered.action.as_deref(), Some("npm test"));
    assert!(rendered.approvable);

    let multi: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "shell",
        "commands": [{"commandLine": "npm test"}, {"commandLine": "git status"}],
        "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    let rendered = render_permission(&multi, "Shell");
    assert_eq!(rendered.action.as_deref(), Some("npm test\ngit status"));
    assert!(rendered.approvable);

    let incomplete: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "shell", "commands": [{"missing": "a command"}],
        "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    assert!(!render_permission(&incomplete, "Shell").approvable);

    let mut nested: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "write", "fileName": "src/auth.ts", "diff": "+safe change",
        "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    nested.extra = serde_json::json!({
        "requestId": "write-1",
        "permissionRequest": {
            "kind": "write", "fileName": "src/auth.ts", "diff": "+safe change"
        }
    });
    let rendered = render_permission(&nested, "Write");
    assert_eq!(rendered.action.as_deref(), Some("src/auth.ts"));
    assert!(rendered.approvable);
    assert_eq!(rendered.details[0].value, "+safe change");
    Ok(())
}

#[test]
fn allow_all_never_autoapproves_mcp_or_elevated_commands() -> Result<()> {
    let shell: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "shell", "fullCommandText": "npm test", "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    assert!(can_autoapprove(
        &shell,
        "Shell",
        &render_permission(&shell, "Shell")
    ));
    let mut elevated = shell;
    elevated.extra["requestSandboxBypass"] = serde_json::Value::Bool(true);
    assert!(!can_autoapprove(
        &elevated,
        "Shell",
        &render_permission(&elevated, "Shell")
    ));
    let mcp: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "mcp", "serverName": "unreviewed", "toolName": "run",
        "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    assert!(!can_autoapprove(
        &mcp,
        "Mcp",
        &render_permission(&mcp, "Mcp")
    ));
    let managed: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "read", "path": "/remote/repo/src/main.rs",
        "managedSettingsEnabled": true
    }))
    .map_err(anyhow::Error::from)?;
    assert!(autoapproval_source(
        &managed,
        "Read",
        &render_permission(&managed, "Read"),
        true,
        true
    )
    .is_none());
    let nested_managed: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "shell",
        "permissionRequest": {
            "kind": "shell", "fullCommandText": "npm test", "managedSettingsEnabled": true
        }
    }))
    .map_err(anyhow::Error::from)?;
    assert!(!can_autoapprove(
        &nested_managed,
        "Shell",
        &render_permission(&nested_managed, "Shell")
    ));
    Ok(())
}

#[test]
fn nested_mcp_arguments_are_reviewable_but_redacted_or_oversized_requests_are_not() -> Result<()> {
    let nested: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "mcp", "serverName": "example-tools", "toolName": "search",
        "args": { "query": { "terms": ["oauth", "callback"], "limit": 5 } },
        "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    let displayed = render_permission(&nested, "Mcp");
    assert!(displayed.approvable);
    assert_eq!(displayed.action.as_deref(), Some("search"));
    assert_eq!(displayed.details[0].label, "Server");
    let arguments = displayed
        .details
        .iter()
        .find(|detail| detail.label == "query")
        .ok_or_else(|| AppError::NotFound("Nested MCP query was not displayed".into()))?;
    assert!(arguments.value.contains("oauth"));
    assert!(arguments.value.contains("\"limit\": 5"));

    let protected: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "mcp", "serverName": "example-tools", "toolName": "search",
        "args": { "request": { "authorization": "Bearer secret-value", "term": "oauth" } },
        "warning": "Review elevated access",
        "requestSandboxBypass": true,
        "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    let displayed = render_permission(&protected, "Mcp");
    assert!(!displayed.approvable);
    assert!(displayed
        .warning
        .as_deref()
        .is_some_and(|text| text.contains("redacted")));
    assert!(displayed.warning.as_deref().is_some_and(|text| text
        .contains("Review elevated access")
        && text.contains("sandbox access")));
    let details = serde_json::to_string(&displayed.details).map_err(anyhow::Error::from)?;
    assert!(details.contains("[redacted]"));
    assert!(!details.contains("secret-value"));

    let huge: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "mcp", "serverName": "example-tools", "toolName": "search",
        "args": { "query": "x".repeat(4097) },
        "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    assert!(!render_permission(&huge, "Mcp").approvable);
    Ok(())
}

#[tokio::test]
async fn reviewed_mcp_grant_only_autoapproves_complete_unmanaged_requests() -> Result<()> {
    let seen = Arc::new(StdMutex::new(Vec::<serde_json::Value>::new()));
    let sink = seen.clone();
    let handler = UiPermissionHandler {
        channel: Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                sink.lock()
                    .expect("test event lock")
                    .push(serde_json::from_str(&json)?);
            }
            Ok(())
        }),
        pending: Arc::new(PendingPermissions(Mutex::new(HashMap::new()))),
        working_directory: "/remote/repo".into(),
        active: Arc::new(AtomicBool::new(true)),
        allow_all: Arc::new(AtomicBool::new(true)),
        autopilot: Arc::new(AtomicBool::new(true)),
        reviewed_mcp: Arc::new(StdMutex::new(HashMap::from([(
            "example-tools".into(),
            None,
        )]))),
        reviewed_mcp_grant: Arc::new(AtomicBool::new(true)),
        client: None,
    };
    let input = serde_json::json!({
        "kind": "mcp", "serverName": "example-tools", "toolName": "search",
        "args": {"query": {"keywords": ["oauth", "callback"]}},
        "managedSettingsEnabled": false
    });
    let request: PermissionRequestData =
        serde_json::from_value(input.clone()).map_err(anyhow::Error::from)?;
    let result = handler
        .handle(
            SessionId::new("sdk-1"),
            RequestId::new("mcp-approved"),
            request,
        )
        .await;
    assert!(format!("{result:?}").contains("ApproveOnce"));
    {
        let events = seen.lock().expect("test event lock");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0]["source"], "reviewedMcp");
    }

    for (index, blocked) in [
        serde_json::json!({"kind": "mcp", "serverName": "unreviewed", "toolName": "search", "args": {}, "managedSettingsEnabled": false}),
        serde_json::json!({"kind": "mcp", "serverName": "example-tools", "toolName": "search", "args": {}, "managedSettingsEnabled": true}),
        serde_json::json!({"kind": "mcp", "serverName": "example-tools", "toolName": "search", "args": {}, "canOfferServerWideApproval": false, "managedSettingsEnabled": false}),
        serde_json::json!({"kind": "mcp", "serverName": "example-tools", "toolName": "search", "args": {"query": {"apiKey": "not-displayed"}}, "managedSettingsEnabled": false}),
        serde_json::json!({"kind": "mcp", "serverName": "example-tools", "toolName": "search", "managedSettingsEnabled": false}),
        serde_json::json!({"kind": "mcp", "permissionRequest": {"kind": "mcp", "serverName": "example-tools", "toolName": "search", "args": {}, "managedSettingsEnabled": true}}),
    ].into_iter().enumerate() {
        let data: PermissionRequestData =
            serde_json::from_value(blocked).map_err(anyhow::Error::from)?;
        let result = tokio::time::timeout(
            Duration::from_millis(25),
            handler.handle(
                SessionId::new("sdk-1"),
                RequestId::new(ulid::Ulid::new().to_string()),
                data,
            ),
        )
        .await;
        assert!(result.is_err(), "Restricted MCP request {index} returned {result:?}");
    }
    {
        let events = seen.lock().expect("test event lock");
        assert_eq!(events.len(), 7);
        assert!(events
            .iter()
            .skip(1)
            .all(|event| event["type"] == "permissionRequested"));
    }
    handler.pending.decline_all().await;

    handler
        .reviewed_mcp
        .lock()
        .expect("test grant lock")
        .insert(
            "example-tools".into(),
            Some("mismatched-fingerprint".into()),
        );
    let request: PermissionRequestData =
        serde_json::from_value(input).map_err(anyhow::Error::from)?;
    let result = tokio::time::timeout(
        Duration::from_millis(25),
        handler.handle(
            SessionId::new("sdk-1"),
            RequestId::new("mcp-config-changed"),
            request,
        ),
    )
    .await;
    assert!(
        result.is_err(),
        "Changed CLI MCP configuration must not be auto-approved"
    );
    let events = seen.lock().expect("test event lock");
    assert_eq!(events[7]["type"], "permissionRequested");
    assert!(events[7]["warning"]
        .as_str()
        .is_some_and(|message| message.contains("configuration")));
    Ok(())
}

#[tokio::test]
async fn autopilot_only_autoapproves_complete_unmanaged_reads() -> Result<()> {
    let seen = Arc::new(StdMutex::new(Vec::<serde_json::Value>::new()));
    let sink = seen.clone();
    let handler = UiPermissionHandler {
        channel: Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                sink.lock()
                    .expect("test event lock")
                    .push(serde_json::from_str(&json)?);
            }
            Ok(())
        }),
        pending: Arc::new(PendingPermissions(Mutex::new(HashMap::new()))),
        working_directory: "/remote/repo".into(),
        active: Arc::new(AtomicBool::new(true)),
        allow_all: Arc::new(AtomicBool::new(false)),
        autopilot: Arc::new(AtomicBool::new(true)),
        reviewed_mcp: Arc::new(StdMutex::new(HashMap::new())),
        reviewed_mcp_grant: Arc::new(AtomicBool::new(false)),
        client: None,
    };
    let read: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "read", "path": "/remote/repo/src/main.rs",
        "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    let result = handler
        .handle(SessionId::new("sdk-1"), RequestId::new("read-1"), read)
        .await;
    assert!(format!("{result:?}").contains("ApproveOnce"));
    let events = seen.lock().expect("test event lock");
    assert_eq!(events[0]["type"], "permissionAutoApproved");
    assert_eq!(events[0]["source"], "autopilot");
    assert_eq!(events[0]["command"], "/remote/repo/src/main.rs");
    drop(events);

    let shell: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "shell", "fullCommandText": "npm test", "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    assert!(autoapproval_source(
        &shell,
        "Shell",
        &render_permission(&shell, "Shell"),
        true,
        false
    )
    .is_none());
    let missing: PermissionRequestData = serde_json::from_value(serde_json::json!({
        "kind": "read", "managedSettingsEnabled": false
    }))
    .map_err(anyhow::Error::from)?;
    assert!(autoapproval_source(
        &missing,
        "Read",
        &render_permission(&missing, "Read"),
        true,
        false
    )
    .is_none());
    Ok(())
}

#[tokio::test]
async fn allow_all_is_explicit_and_scoped_to_one_app_session() -> Result<()> {
    let directory = tempfile::tempdir()?;
    let db = Database::open(&directory.path().join("sessions.sqlite3"))?;
    let host = repository::list_hosts(&*db.connection()?)?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Local host missing".into()))?;
    let workspace = repository::add_workspace(&*db.connection()?, &host.id, "/tmp/repo", "repo")?;
    let first = repository::add_session(
        &*db.connection()?,
        &workspace.id,
        "First",
        "crui_first",
        "bash",
    )?;
    let second = repository::add_session(
        &*db.connection()?,
        &workspace.id,
        "Second",
        "crui_second",
        "bash",
    )?;
    let manager = Arc::new(SessionManager {
        db,
        backend: Arc::new(OpenSshTmuxBackend::new()),
    });
    let runtime = Arc::new(RemoteCopilotRuntime::new(manager.clone()));
    let agent = CopilotAgentBackend::new(manager, runtime);
    assert!(agent
        .set_permission_mode(&first.id, PermissionMode::AllowAll, false)
        .is_err());
    assert!(!agent.approval_mode(&first.id)?.load(Ordering::SeqCst));
    agent.set_permission_mode(&first.id, PermissionMode::AllowAll, true)?;
    assert!(agent.approval_mode(&first.id)?.load(Ordering::SeqCst));
    assert!(!agent.approval_mode(&second.id)?.load(Ordering::SeqCst));
    let reviewed_grant = agent.mcp_approval_mode(&first.id)?;
    reviewed_grant.store(true, Ordering::SeqCst);
    agent
        .mcp_grant_scopes
        .lock()
        .expect("test grant scopes")
        .insert(
            first.id.clone(),
            HashMap::from([("example-tools".into(), None)]),
        );
    assert!(!agent.mcp_approval_mode(&second.id)?.load(Ordering::SeqCst));
    assert!(agent
        .set_reviewed_mcp_approval(&second.id, true, false)
        .await
        .is_err());
    agent.set_permission_mode(&first.id, PermissionMode::Ask, false)?;
    assert!(!agent.approval_mode(&first.id)?.load(Ordering::SeqCst));
    assert!(!reviewed_grant.load(Ordering::SeqCst));
    assert!(!agent
        .mcp_grant_scopes
        .lock()
        .expect("test grant scopes")
        .contains_key(&first.id));
    Ok(())
}
