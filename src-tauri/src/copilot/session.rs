use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use async_trait::async_trait;
use github_copilot_sdk::handler::{PermissionHandler, PermissionResult};
use github_copilot_sdk::rpc::{
    McpEnableRequest, McpOauthLoginRequest, McpStartServerRequest, McpStopServerRequest,
    ModeSetRequest, ModelSwitchToRequest, ModelSwitchToResult,
};
use github_copilot_sdk::session::Session;
use github_copilot_sdk::session_events::{
    ContextTier, McpServerStatus, ReasoningSummary, SessionMode,
};
use github_copilot_sdk::subscription::{EventSubscription, RecvErrorKind};
use github_copilot_sdk::{
    Client, PermissionRequestData, RequestId, ResumeSessionConfig, SessionConfig, SessionEvent,
    SessionId, SessionListFilter, SessionMetadata,
};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

use crate::error::{AppError, Result};
use crate::sessions::manager::SessionManager;
use crate::sessions::repository;

use super::display;
use super::events::{self, AgentEvent, PermissionDetail};
use super::mcp::{self, McpAuthResult, McpInventory, McpView};
use super::recent::{self, RecentCliChat};
use super::runtime::RemoteCopilotRuntime;
use super::sleep::SleepInhibitor;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentStatus {
    Connected,
    Unattached,
    SessionMissing,
    Paused,
    Unavailable,
    HostOffline,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    Ask,
    AllowAll,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewedMcpApprovalState {
    pub available: Vec<String>,
    pub enabled: bool,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ChatMode {
    Interactive,
    Autopilot,
    Unsupported,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSnapshot {
    pub status: AgentStatus,
    pub copilot_session_id: Option<String>,
    pub events: Vec<AgentEvent>,
    pub error: Option<String>,
    pub mode: ChatMode,
    pub mode_error: Option<String>,
    pub mcp_warnings: Vec<String>,
    pub model: Option<String>,
    pub context_tier: Option<String>,
    pub reasoning_effort: Option<String>,
    pub model_error: Option<String>,
}

impl AgentSnapshot {
    fn unavailable(status: AgentStatus, id: Option<String>, reason: String) -> Self {
        Self {
            status,
            copilot_session_id: id,
            events: Vec::new(),
            error: Some(reason),
            mode: ChatMode::Unsupported,
            mode_error: None,
            mcp_warnings: Vec::new(),
            model: None,
            context_tier: None,
            reasoning_effort: None,
            model_error: None,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CopilotModel {
    pub id: String,
    pub name: String,
    pub supported_context_tiers: Vec<String>,
    pub max_context_window_tokens: Option<i64>,
    pub supported_reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSelection {
    pub current_model: Option<String>,
    pub context_tier: Option<String>,
    pub reasoning_effort: Option<String>,
    pub pending: bool,
    pub queued: bool,
    pub warning: Option<String>,
}

fn context_tier_name(tier: &ContextTier) -> &'static str {
    match tier {
        ContextTier::Default => "default",
        ContextTier::LongContext => "long_context",
        ContextTier::Unknown => "unknown",
    }
}

fn model_switch_result(
    result: ModelSwitchToResult,
    model_id: &str,
    effort: Option<&str>,
    tier: Option<&str>,
) -> Result<ModelSelection> {
    if result.status.as_deref() == Some("confirmation_required") {
        let detail = result
            .confirmation
            .map(|confirmation| {
                format!(
                    " Current conversation: {:.0} tokens; new limit: {:.0} tokens ({}).",
                    confirmation.current_tokens,
                    confirmation.target_limit,
                    confirmation.target_model_display_name
                )
            })
            .unwrap_or_default();
        return Err(AppError::Operation(anyhow::anyhow!(
            "Copilot requires confirmation before compacting this conversation.{detail} This SDK version does not document a safe confirmation response, so the switch was not applied."
        )));
    }
    if let Some(error) = result.persistence_error {
        return Err(AppError::Operation(anyhow::anyhow!(
            "Copilot could not save the model change: {error}"
        )));
    }
    let state = result.model_state;
    let state_model = state.as_ref().and_then(|model| model.model_id.as_deref());
    let state_matches = state_model == Some(model_id)
        && effort.is_none_or(|value| {
            state
                .as_ref()
                .and_then(|model| model.reasoning_effort.as_deref())
                == Some(value)
        })
        && tier.is_none_or(|value| {
            state
                .as_ref()
                .and_then(|model| model.context_tier.as_ref().map(context_tier_name))
                == Some(value)
        });
    let applied = result.deferred != Some(true)
        && (state_matches
            || state.is_none()
                && effort.is_none()
                && tier.is_none()
                && result.model_id.as_deref() == Some(model_id));
    if !applied && result.deferred != Some(true) {
        if let Some(status) = result.status.as_deref() {
            return Err(AppError::Operation(anyhow::anyhow!(
                "Copilot reported model switch status {status}: {}",
                result
                    .message
                    .unwrap_or_else(|| "no further details".into())
            )));
        }
    }
    let pending = !applied;
    Ok(ModelSelection {
        current_model: if applied {
            Some(model_id.into())
        } else {
            state.as_ref().and_then(|model| model.model_id.clone())
        },
        context_tier: state
            .as_ref()
            .and_then(|model| model.context_tier.as_ref().map(context_tier_name))
            .map(str::to_owned),
        reasoning_effort: state
            .as_ref()
            .and_then(|model| model.reasoning_effort.clone()),
        pending,
        queued: result.deferred == Some(true),
        warning: result.warning.or_else(|| {
            (pending && result.deferred != Some(true)).then(|| {
                result.message.unwrap_or_else(|| {
                    "Copilot accepted the request but has not confirmed the new settings yet".into()
                })
            })
        }),
    })
}

struct PendingRequest {
    sender: oneshot::Sender<PermissionResult>,
    displayable: bool,
}

struct PendingPermissions(Mutex<HashMap<String, PendingRequest>>);

impl PendingPermissions {
    async fn decline_all(&self) {
        for (_, request) in self.0.lock().await.drain() {
            if request
                .sender
                .send(PermissionResult::user_not_available())
                .is_err()
            {
                tracing::warn!("Permission request closed before its UI disconnected");
            }
        }
    }
}

struct UiPermissionHandler {
    channel: Channel<AgentEvent>,
    pending: Arc<PendingPermissions>,
    working_directory: String,
    active: Arc<AtomicBool>,
    allow_all: Arc<AtomicBool>,
    autopilot: Arc<AtomicBool>,
    reviewed_mcp: Arc<StdMutex<HashMap<String, Option<String>>>>,
    reviewed_mcp_grant: Arc<AtomicBool>,
    client: Option<Client>,
}

struct RenderedPermission {
    action: Option<String>,
    description: Option<String>,
    warning: Option<String>,
    details: Vec<PermissionDetail>,
    approvable: bool,
}

fn permission_payload(data: &PermissionRequestData) -> &serde_json::Value {
    data.extra.get("permissionRequest").unwrap_or(&data.extra)
}

fn managed_flag(data: &PermissionRequestData, key: &str, outer: bool) -> bool {
    outer
        || permission_payload(data)
            .get(key)
            .is_some_and(|value| value.as_bool() != Some(false))
}

fn managed_settings_enabled(data: &PermissionRequestData) -> bool {
    managed_flag(
        data,
        "managedSettingsEnabled",
        data.managed_settings_enabled,
    )
}

fn managed_approval_required(data: &PermissionRequestData) -> bool {
    managed_flag(
        data,
        "managedApprovalRequired",
        data.managed_approval_required == Some(true),
    )
}

fn permission_text(data: &PermissionRequestData, key: &str) -> Option<String> {
    permission_payload(data)
        .get(key)
        .and_then(|value| value.as_str())
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
}

fn add_permission_warning(warning: &mut Option<String>, additional: &str) {
    match warning {
        Some(existing) => {
            existing.push(' ');
            existing.push_str(additional);
        }
        None => *warning = Some(additional.into()),
    }
}

fn shell_action(data: &PermissionRequestData) -> Option<String> {
    for key in ["fullCommandText", "command", "commandLine"] {
        if let Some(command) = permission_text(data, key) {
            return Some(command);
        }
    }
    let commands = permission_payload(data).get("commands")?.as_array()?;
    if commands.is_empty() {
        return None;
    }
    let lines: Option<Vec<_>> = commands
        .iter()
        .map(|command| {
            command
                .as_str()
                .or_else(|| {
                    ["fullCommandText", "commandLine", "command"]
                        .into_iter()
                        .find_map(|key| command.get(key).and_then(|value| value.as_str()))
                })
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
        })
        .collect();
    lines.map(|commands| commands.join("\n"))
}

fn argument_details(data: &PermissionRequestData) -> Option<(Vec<PermissionDetail>, bool)> {
    let Some(arguments) = permission_payload(data).get("args") else {
        return Some((Vec::new(), false));
    };
    let rendered = display::argument_details(arguments)?;
    Some((
        rendered
            .fields
            .into_iter()
            .map(|(label, value)| PermissionDetail { label, value })
            .collect(),
        rendered.complete,
    ))
}

fn render_permission(data: &PermissionRequestData, kind: &str) -> RenderedPermission {
    let description = permission_text(data, "intention");
    let mut warning = permission_text(data, "warning");
    if permission_payload(data)
        .get("requestSandboxBypass")
        .and_then(|value| value.as_bool())
        == Some(true)
        || permission_payload(data)
            .get("requestSandboxPermissive")
            .and_then(|value| value.as_bool())
            == Some(true)
    {
        add_permission_warning(&mut warning, "Requests elevated sandbox access.");
    }
    let mut details = Vec::new();
    let mut details_complete = true;
    let action = match kind {
        "Shell" => shell_action(data),
        "Write" => {
            if let Some(diff) =
                permission_text(data, "diff").or_else(|| permission_text(data, "newFileContents"))
            {
                details.push(PermissionDetail {
                    label: "Proposed change".into(),
                    value: diff,
                });
            }
            permission_text(data, "fileName")
        }
        "Read" => permission_text(data, "path"),
        "Url" => {
            if let Some(redirect) = permission_text(data, "redirectedFrom") {
                details.push(PermissionDetail {
                    label: "Redirected from".into(),
                    value: redirect,
                });
            }
            permission_text(data, "url")
        }
        "Mcp" | "CustomTool" => {
            if warning.is_none() {
                warning =
                    Some("Approve only integrations reviewed under your enterprise policy.".into());
            }
            let server = permission_text(data, "serverName");
            let tool = permission_text(data, "toolName");
            if kind == "Mcp" {
                details_complete &= server.is_some();
            }
            if let Some(name) = server {
                details.push(PermissionDetail {
                    label: "Server".into(),
                    value: name,
                });
            }
            if let Some((arguments, complete)) = argument_details(data) {
                details.extend(arguments);
                details_complete &= complete;
                if !complete {
                    add_permission_warning(
                        &mut warning,
                        "MCP tool arguments contain redacted or missing fields; approval is disabled.",
                    );
                }
            } else {
                details_complete = false;
                add_permission_warning(
                    &mut warning,
                    "Tool arguments could not be displayed completely; approval is disabled.",
                );
            }
            tool
        }
        _ => None,
    };
    let approvable =
        action.is_some() && !(kind == "Write" && details.is_empty()) && details_complete;
    RenderedPermission {
        action,
        description,
        warning,
        details,
        approvable,
    }
}

fn can_autoapprove(
    data: &PermissionRequestData,
    kind: &str,
    rendered: &RenderedPermission,
) -> bool {
    let elevated = permission_payload(data)
        .get("requestSandboxBypass")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
        || permission_payload(data)
            .get("requestSandboxPermissive")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
    rendered.approvable
        && !elevated
        && !managed_settings_enabled(data)
        && !managed_approval_required(data)
        && matches!(kind, "Shell" | "Write" | "Read" | "Url")
}

fn can_autoapprove_reviewed_mcp(
    data: &PermissionRequestData,
    kind: &str,
    rendered: &RenderedPermission,
) -> bool {
    kind == "Mcp"
        && rendered.approvable
        && !managed_settings_enabled(data)
        && !managed_approval_required(data)
        && permission_payload(data)
            .get("canOfferServerWideApproval")
            .and_then(serde_json::Value::as_bool)
            != Some(false)
        && permission_payload(data)
            .get("requestSandboxBypass")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
        && permission_payload(data)
            .get("requestSandboxPermissive")
            .and_then(serde_json::Value::as_bool)
            != Some(true)
}

fn autoapproval_source<'a>(
    data: &PermissionRequestData,
    kind: &str,
    rendered: &RenderedPermission,
    autopilot: bool,
    allow_all: bool,
) -> Option<&'a str> {
    if !can_autoapprove(data, kind, rendered) {
        None
    } else if autopilot && kind == "Read" {
        Some("autopilot")
    } else if allow_all {
        Some("allowAll")
    } else {
        None
    }
}

#[async_trait]
impl PermissionHandler for UiPermissionHandler {
    async fn handle(
        &self,
        _session_id: SessionId,
        request_id: RequestId,
        data: PermissionRequestData,
    ) -> PermissionResult {
        if !self.active.load(Ordering::SeqCst) {
            return PermissionResult::user_not_available();
        }
        if managed_settings_enabled(&data) && managed_approval_required(&data) {
            return PermissionResult::user_not_available();
        }
        let kind = data
            .kind
            .as_ref()
            .map(|value| format!("{value:?}"))
            .unwrap_or_else(|| "Other".into());
        let mut rendered = render_permission(&data, &kind);
        if kind == "Mcp" && self.reviewed_mcp_grant.load(Ordering::SeqCst) {
            if managed_settings_enabled(&data) {
                add_permission_warning(
                    &mut rendered.warning,
                    "Managed settings require per-call review; the session grant cannot approve this tool.",
                );
            } else if permission_payload(&data)
                .get("canOfferServerWideApproval")
                .and_then(serde_json::Value::as_bool)
                == Some(false)
            {
                add_permission_warning(
                    &mut rendered.warning,
                    "Server policy forbids blanket approvals; review this tool manually.",
                );
            }
        }
        let key = request_id.to_string();
        let mut source = autoapproval_source(
            &data,
            &kind,
            &rendered,
            self.autopilot.load(Ordering::SeqCst),
            self.allow_all.load(Ordering::SeqCst),
        );
        if source.is_none()
            && self.autopilot.load(Ordering::SeqCst)
            && self.allow_all.load(Ordering::SeqCst)
            && self.reviewed_mcp_grant.load(Ordering::SeqCst)
            && can_autoapprove_reviewed_mcp(&data, &kind, &rendered)
        {
            if let Some(server) = permission_text(&data, "serverName") {
                let reviewed = match self.reviewed_mcp.lock() {
                    Ok(servers) => servers.get(&server).cloned(),
                    Err(_) => {
                        tracing::warn!(
                            "Reviewed MCP grant lock poisoned; requesting manual approval"
                        );
                        add_permission_warning(
                            &mut rendered.warning,
                            "Could not verify reviewed MCP grant; manual approval required.",
                        );
                        None
                    }
                };
                if let Some(fingerprint) = reviewed {
                    let eligible = if let Some(expected) = fingerprint {
                        match self.client.as_ref() {
                            Some(client) => match mcp::user_config(client).await {
                                Ok(global) => match global.get(&server) {
                                    Some(config) => match mcp::config_fingerprint(config) {
                                        Ok(actual) => actual == expected,
                                        Err(error) => {
                                            tracing::warn!(%error, "Could not fingerprint reviewed MCP configuration");
                                            false
                                        }
                                    },
                                    None => false,
                                },
                                Err(error) => {
                                    tracing::warn!(%error, "Could not verify remote MCP configuration");
                                    false
                                }
                            },
                            None => false,
                        }
                    } else {
                        true
                    };
                    if eligible {
                        source = Some("reviewedMcp");
                    } else {
                        add_permission_warning(
                            &mut rendered.warning,
                            "Remote MCP configuration could not be verified or changed; review it in Integrations. Automatic approval is disabled."
                        );
                    }
                }
            }
        }
        if !self.active.load(Ordering::SeqCst) {
            return PermissionResult::user_not_available();
        }
        let still_granted = match source {
            Some("reviewedMcp") => {
                self.reviewed_mcp_grant.load(Ordering::SeqCst)
                    && self.allow_all.load(Ordering::SeqCst)
                    && self.autopilot.load(Ordering::SeqCst)
                    && permission_text(&data, "serverName").is_some_and(|server| {
                        self.reviewed_mcp
                            .lock()
                            .is_ok_and(|servers| servers.contains_key(&server))
                    })
            }
            Some("allowAll") => self.allow_all.load(Ordering::SeqCst),
            Some("autopilot") => self.autopilot.load(Ordering::SeqCst),
            _ => true,
        };
        if !still_granted {
            source = None;
            add_permission_warning(
                &mut rendered.warning,
                "The automatic approval grant changed; review this request manually.",
            );
        }
        if let Some(source) = source {
            if let Some(command) = rendered.action.as_ref() {
                let event = AgentEvent::PermissionAutoApproved {
                    event_id: format!("auto-permission-{key}"),
                    request_id: key.clone(),
                    kind: kind.clone(),
                    command: command.clone(),
                    working_directory: self.working_directory.clone(),
                    source: source.into(),
                };
                if self.channel.send(event).is_ok() {
                    tracing::info!(request_id = %key, %source, "Session submitted a permitted approval");
                    return PermissionResult::approve_once();
                }
            }
            return PermissionResult::user_not_available();
        }
        tracing::info!(request_id = %key, permission_kind = %kind, "Copilot needs user approval");
        let (sender, receiver) = oneshot::channel();
        self.pending.0.lock().await.insert(
            key.clone(),
            PendingRequest {
                sender,
                displayable: rendered.approvable,
            },
        );
        let event = AgentEvent::PermissionRequested {
            event_id: format!("permission-{key}"),
            request_id: key.clone(),
            kind,
            command: rendered.action,
            description: rendered.description,
            warning: rendered.warning,
            details: rendered.details,
            approvable: rendered.approvable,
            working_directory: self.working_directory.clone(),
        };
        if self.channel.send(event).is_err() {
            self.pending.0.lock().await.remove(&key);
            return PermissionResult::user_not_available();
        }
        let decision = tokio::time::timeout(Duration::from_secs(300), receiver).await;
        self.pending.0.lock().await.remove(&key);
        match decision {
            Ok(Ok(result)) => result,
            Ok(Err(_)) | Err(_) => PermissionResult::user_not_available(),
        }
    }
}

struct ActiveAgent {
    session: Arc<Session>,
    subscription: JoinHandle<()>,
    pending: Arc<PendingPermissions>,
    permission_active: Arc<AtomicBool>,
    autopilot: Arc<AtomicBool>,
    reviewed_mcp: Arc<StdMutex<HashMap<String, Option<String>>>>,
    reviewed_mcp_grant: Arc<AtomicBool>,
    channel: Channel<AgentEvent>,
}

fn update_sleep(
    sleep: &SleepInhibitor,
    session_id: &str,
    channel: &Channel<AgentEvent>,
    active: bool,
) {
    let result = sleep.set_working(session_id, active);
    if let Err(error) = &result {
        tracing::error!(%error, %session_id, "Could not update Mac idle-sleep protection");
    }
    if channel
        .send(AgentEvent::SleepStatus {
            event_id: ulid::Ulid::new().to_string(),
            active: active && result.is_ok(),
            error: result.err().map(|error| error.to_string()),
        })
        .is_err()
    {
        tracing::warn!(%session_id, "Could not display idle-sleep protection status");
    }
}

async fn verify_working_directory(
    events: &mut EventSubscription,
    lifecycle_event: &str,
    expected: &str,
    host: &str,
) -> Result<Vec<SessionEvent>> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let mut buffered = Vec::new();
    loop {
        let event = tokio::time::timeout_at(deadline, events.recv())
            .await
            .map_err(|_| AppError::CopilotRuntime {
                host: host.into(),
                reason: "Copilot did not confirm the remote working directory".into(),
            })?
            .map_err(|_| AppError::CopilotRuntime {
                host: host.into(),
                reason: "Copilot lost the startup event needed to verify its working directory"
                    .into(),
            })?;
        if event.event_type == lifecycle_event {
            if let Some(actual) = event
                .data
                .pointer("/context/cwd")
                .and_then(|cwd| cwd.as_str())
            {
                if actual.trim_end_matches('/') != expected.trim_end_matches('/') {
                    return Err(AppError::CopilotRuntime {
                        host: host.into(),
                        reason: format!(
                            "Copilot resolved {actual} instead of the selected remote repository {expected}; refusing to send messages"
                        ),
                    });
                }
                return Ok(buffered);
            }
        }
        if buffered.len() == 1024 {
            return Err(AppError::CopilotRuntime {
                host: host.into(),
                reason:
                    "Copilot emitted too many startup events before reporting its working directory"
                        .into(),
            });
        }
        buffered.push(event);
    }
}

pub struct CopilotAgentBackend {
    sessions: Arc<SessionManager>,
    runtime: Arc<RemoteCopilotRuntime>,
    active: Mutex<HashMap<String, ActiveAgent>>,
    connect_guards: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    generation: AtomicU64,
    latest: StdMutex<HashMap<String, u64>>,
    approval_modes: StdMutex<HashMap<String, Arc<AtomicBool>>>,
    mcp_approval_modes: StdMutex<HashMap<String, Arc<AtomicBool>>>,
    mcp_grant_scopes: StdMutex<HashMap<String, HashMap<String, Option<String>>>>,
    sleep: Arc<SleepInhibitor>,
}

impl CopilotAgentBackend {
    pub fn new(sessions: Arc<SessionManager>, runtime: Arc<RemoteCopilotRuntime>) -> Self {
        Self {
            sessions,
            runtime,
            active: Mutex::new(HashMap::new()),
            connect_guards: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            latest: StdMutex::new(HashMap::new()),
            approval_modes: StdMutex::new(HashMap::new()),
            mcp_approval_modes: StdMutex::new(HashMap::new()),
            mcp_grant_scopes: StdMutex::new(HashMap::new()),
            sleep: Arc::new(SleepInhibitor::new()),
        }
    }

    fn approval_mode(&self, app_session_id: &str) -> Result<Arc<AtomicBool>> {
        let mut modes = self
            .approval_modes
            .lock()
            .map_err(|_| AppError::InvalidInput("Permission mode lock poisoned".into()))?;
        Ok(modes
            .entry(app_session_id.into())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone())
    }

    fn mcp_approval_mode(&self, app_session_id: &str) -> Result<Arc<AtomicBool>> {
        let mut modes = self
            .mcp_approval_modes
            .lock()
            .map_err(|_| AppError::InvalidInput("Reviewed MCP grant lock poisoned".into()))?;
        Ok(modes
            .entry(app_session_id.into())
            .or_insert_with(|| Arc::new(AtomicBool::new(false)))
            .clone())
    }

    fn revoke_mcp_grant(&self, app_session_id: &str) -> Result<()> {
        self.mcp_approval_mode(app_session_id)?
            .store(false, Ordering::SeqCst);
        self.mcp_grant_scopes
            .lock()
            .map_err(|_| AppError::InvalidInput("Reviewed MCP grant scope lock poisoned".into()))?
            .remove(app_session_id);
        Ok(())
    }

    pub fn set_permission_mode(
        &self,
        app_session_id: &str,
        mode: PermissionMode,
        confirmed: bool,
    ) -> Result<PermissionMode> {
        self.sessions.get(app_session_id)?;
        if matches!(mode, PermissionMode::AllowAll) && !confirmed {
            return Err(AppError::InvalidInput(
                "Allow all requires explicit confirmation".into(),
            ));
        }
        self.approval_mode(app_session_id)?
            .store(matches!(mode, PermissionMode::AllowAll), Ordering::SeqCst);
        if matches!(mode, PermissionMode::Ask) {
            self.revoke_mcp_grant(app_session_id)?;
        }
        tracing::info!(session_id = %app_session_id, allow_all = matches!(mode, PermissionMode::AllowAll), "Updated session permission mode");
        Ok(mode)
    }

    pub async fn reviewed_mcp_approval_state(
        &self,
        app_session_id: &str,
    ) -> Result<ReviewedMcpApprovalState> {
        let active = self.active.lock().await;
        let chat = active.get(app_session_id).ok_or_else(|| {
            AppError::NotFound("Connect Copilot Chat before reviewing MCP permissions".into())
        })?;
        let mut available: Vec<_> = chat
            .reviewed_mcp
            .lock()
            .map_err(|_| AppError::InvalidInput("Reviewed MCP grant lock poisoned".into()))?
            .keys()
            .cloned()
            .collect();
        available.sort();
        Ok(ReviewedMcpApprovalState {
            available,
            enabled: chat.reviewed_mcp_grant.load(Ordering::SeqCst),
        })
    }

    pub async fn set_reviewed_mcp_approval(
        &self,
        app_session_id: &str,
        enabled: bool,
        confirmed: bool,
    ) -> Result<ReviewedMcpApprovalState> {
        if enabled && !confirmed {
            return Err(AppError::InvalidInput(
                "Automatic reviewed MCP approvals require explicit confirmation".into(),
            ));
        }
        let active = self.active.lock().await;
        let chat = active.get(app_session_id).ok_or_else(|| {
            AppError::NotFound("Connect Copilot Chat before changing MCP permissions".into())
        })?;
        if enabled
            && (!chat.autopilot.load(Ordering::SeqCst)
                || !self.approval_mode(app_session_id)?.load(Ordering::SeqCst))
        {
            return Err(AppError::InvalidInput(
                "Enable both Autopilot and Allow all before approving reviewed MCP tools automatically".into(),
            ));
        }
        let eligible = chat
            .reviewed_mcp
            .lock()
            .map_err(|_| AppError::InvalidInput("Reviewed MCP grant lock poisoned".into()))?
            .clone();
        let mut available: Vec<_> = eligible.keys().cloned().collect();
        available.sort();
        if enabled && available.is_empty() {
            return Err(AppError::InvalidInput(
                "No reviewed MCP servers are available in this Chat; review or activate one first"
                    .into(),
            ));
        }
        let mut scopes = self
            .mcp_grant_scopes
            .lock()
            .map_err(|_| AppError::InvalidInput("Reviewed MCP grant scope lock poisoned".into()))?;
        if enabled {
            scopes.insert(app_session_id.into(), eligible);
        } else {
            scopes.remove(app_session_id);
        }
        chat.reviewed_mcp_grant.store(enabled, Ordering::SeqCst);
        tracing::info!(session_id = %app_session_id, enabled, "Updated session-reviewed MCP approval grant");
        Ok(ReviewedMcpApprovalState { available, enabled })
    }
    fn reserve(&self, app_session_id: &str) -> Result<u64> {
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.latest
            .lock()
            .map_err(|_| AppError::InvalidInput("Copilot session lock poisoned".into()))?
            .insert(app_session_id.into(), generation);
        Ok(generation)
    }

    fn is_current(&self, app_session_id: &str, generation: u64) -> Result<bool> {
        Ok(self
            .latest
            .lock()
            .map_err(|_| AppError::InvalidInput("Copilot session lock poisoned".into()))?
            .get(app_session_id)
            .copied()
            == Some(generation))
    }

    async fn cli_session_list(
        client: &Client,
        repo_path: &str,
    ) -> Result<(Vec<SessionMetadata>, std::collections::HashSet<String>)> {
        let all = tokio::time::timeout(Duration::from_secs(15), client.list_sessions(None))
            .await
            .map_err(|_| {
                AppError::CopilotResume(
                    "Timed out listing recent Copilot CLI sessions on this remote host".into(),
                )
            })?
            .map_err(|error| {
                AppError::CopilotResume(format!(
                    "Could not list recent Copilot CLI sessions on this remote host: {error}"
                ))
            })?;
        let same_repo = tokio::time::timeout(
            Duration::from_secs(15),
            client.list_sessions(Some(SessionListFilter {
                working_directory: Some(repo_path.into()),
                ..Default::default()
            })),
        )
        .await
        .map_err(|_| {
            AppError::CopilotResume(
                "Timed out checking Copilot CLI sessions for this repository".into(),
            )
        })?
        .map_err(|error| {
            AppError::CopilotResume(format!(
                "Could not verify Copilot CLI session repositories: {error}"
            ))
        })?;
        let matching = same_repo
            .into_iter()
            .map(|entry| entry.session_id.to_string())
            .collect();
        Ok((all, matching))
    }

    pub async fn recent_cli_chats(&self, app_session_id: &str) -> Result<Vec<RecentCliChat>> {
        let app = self.sessions.get(app_session_id)?;
        let client = self.runtime.ensure(&app.host).await?;
        let (all, same_repo) = Self::cli_session_list(&client, &app.workspace.repo_path).await?;
        let known = repository::list_sessions(&*self.sessions.db.connection()?)?;
        Ok(recent::summarize(all, &same_repo, &known, app_session_id))
    }

    pub async fn connect(
        &self,
        app_session_id: &str,
        on_event: Channel<AgentEvent>,
        create_if_missing: bool,
        replace_existing: bool,
    ) -> Result<AgentSnapshot> {
        self.connect_selected(
            app_session_id,
            on_event,
            create_if_missing,
            replace_existing,
            None,
        )
        .await
    }

    pub async fn attach_cli_chat(
        &self,
        app_session_id: &str,
        selected_id: &str,
        allow_different_repo: bool,
        on_event: Channel<AgentEvent>,
    ) -> Result<AgentSnapshot> {
        self.connect_selected(
            app_session_id,
            on_event,
            false,
            false,
            Some((selected_id, allow_different_repo)),
        )
        .await
    }

    async fn connect_selected(
        &self,
        app_session_id: &str,
        on_event: Channel<AgentEvent>,
        create_if_missing: bool,
        replace_existing: bool,
        selected: Option<(&str, bool)>,
    ) -> Result<AgentSnapshot> {
        let generation = self.reserve(app_session_id)?;
        let gate = {
            let mut guards = self.connect_guards.lock().await;
            guards
                .entry(app_session_id.into())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = gate.lock().await;
        if !self.is_current(app_session_id, generation)? {
            return Err(AppError::InvalidInput(
                "Copilot connection was superseded".into(),
            ));
        }
        let app_session = self.sessions.get(app_session_id)?;
        let prior_id = app_session.copilot_session_id.clone();
        let resume_id = selected
            .map(|(id, _)| id.to_owned())
            .or_else(|| prior_id.clone());
        let preferences =
            repository::get_copilot_preferences(&*self.sessions.db.connection()?, app_session_id)?;
        if resume_id.is_none() && !create_if_missing && !replace_existing {
            return Ok(AgentSnapshot {
                status: AgentStatus::Unattached,
                copilot_session_id: None,
                events: Vec::new(),
                error: None,
                mode: ChatMode::Unsupported,
                mode_error: None,
                mcp_warnings: Vec::new(),
                model: None,
                context_tier: None,
                reasoning_effort: None,
                model_error: None,
            });
        }
        let client = match self.runtime.ensure(&app_session.host).await {
            Ok(client) => client,
            Err(error @ AppError::ChatPaused { .. }) => {
                return Ok(AgentSnapshot::unavailable(
                    AgentStatus::Paused,
                    prior_id,
                    error.to_string(),
                ));
            }
            Err(error @ AppError::SshTunnel { .. }) => {
                tracing::warn!(session_id = %app_session_id, "Copilot host connection unavailable");
                return Ok(AgentSnapshot::unavailable(
                    AgentStatus::HostOffline,
                    prior_id,
                    error.to_string(),
                ));
            }
            Err(error) => {
                tracing::warn!(session_id = %app_session_id, "Copilot headless runtime unavailable");
                return Ok(AgentSnapshot::unavailable(
                    AgentStatus::Unavailable,
                    prior_id,
                    error.to_string(),
                ));
            }
        };
        if !self.is_current(app_session_id, generation)? {
            return Err(AppError::InvalidInput(
                "Copilot connection was superseded".into(),
            ));
        }
        if let Some((selected_id, allow_different_repo)) = selected {
            if self.sleep.is_working(app_session_id)? {
                return Err(AppError::InvalidInput(
                    "Stop active Copilot work before switching this Chat conversation".into(),
                ));
            }
            let existing_pending = self
                .active
                .lock()
                .await
                .get(app_session_id)
                .map(|active| active.pending.clone());
            if let Some(pending) = existing_pending {
                if !pending.0.lock().await.is_empty() {
                    return Err(AppError::InvalidInput(
                        "Respond to outstanding Copilot permissions before switching conversations"
                            .into(),
                    ));
                }
            }
            let (all, matching) =
                Self::cli_session_list(&client, &app_session.workspace.repo_path).await?;
            let known = repository::list_sessions(&*self.sessions.db.connection()?)?;
            recent::validate_selection(
                selected_id,
                &all,
                &matching,
                &known,
                app_session_id,
                allow_different_repo,
            )?;
        }
        let reviewed_mcp = mcp::list(&*self.sessions.db.connection()?, &app_session.host.id)?;
        let imported_mcp =
            mcp::list_imports(&*self.sessions.db.connection()?, &app_session.host.id)?;
        let settings = mcp::session_settings(&client, &reviewed_mcp, &imported_mcp).await?;
        let mcp_servers = settings.servers;
        let disabled_mcp_servers = settings.disabled;
        let mcp_warnings = settings.needs_review;
        let mut eligible_mcp: HashMap<String, Option<String>> = reviewed_mcp
            .into_iter()
            .map(|server| (server.name, None))
            .collect();
        for imported in imported_mcp {
            if !mcp_warnings.contains(&imported.name) {
                eligible_mcp.insert(imported.name, Some(imported.config_sha256));
            }
        }
        let granted_scope = self
            .mcp_grant_scopes
            .lock()
            .map_err(|_| AppError::InvalidInput("Reviewed MCP grant scope lock poisoned".into()))?
            .get(app_session_id)
            .cloned();
        if granted_scope.as_ref() != Some(&eligible_mcp) || eligible_mcp.is_empty() {
            self.revoke_mcp_grant(app_session_id)?;
        }
        let reviewed_mcp = Arc::new(StdMutex::new(eligible_mcp));
        let pending = Arc::new(PendingPermissions(Mutex::new(HashMap::new())));
        let permission_active = Arc::new(AtomicBool::new(false));
        let autopilot = Arc::new(AtomicBool::new(false));
        let allow_all = if selected.is_some() {
            Arc::new(AtomicBool::new(false))
        } else {
            self.approval_mode(app_session_id)?
        };
        let reviewed_mcp_grant = if selected.is_some() {
            Arc::new(AtomicBool::new(false))
        } else {
            self.mcp_approval_mode(app_session_id)?
        };
        let handler: Arc<dyn PermissionHandler> = Arc::new(UiPermissionHandler {
            channel: on_event.clone(),
            pending: pending.clone(),
            working_directory: app_session.workspace.repo_path.clone(),
            active: permission_active.clone(),
            allow_all,
            autopilot: autopilot.clone(),
            reviewed_mcp: reviewed_mcp.clone(),
            reviewed_mcp_grant: reviewed_mcp_grant.clone(),
            client: Some(client.clone()),
        });
        let (session, mut subscription, lifecycle_event) = if resume_id.is_none()
            || replace_existing
        {
            let requested_id = SessionId::new(format!("remote-chat-{}", ulid::Ulid::new()));
            let config = SessionConfig::default()
                .with_session_id(requested_id.clone())
                .with_enable_session_store(true)
                .with_working_directory(&app_session.workspace.repo_path)
                .with_reasoning_summary(ReasoningSummary::Concise)
                .with_mcp_servers(mcp_servers.clone())
                .with_disabled_mcp_servers(disabled_mcp_servers.clone())
                .with_permission_handler(handler);
            let config = match preferences.model.as_deref() {
                Some(model) => config.with_model(model),
                None => config,
            };
            let config = match preferences.reasoning_effort.as_deref() {
                Some(effort) => config.with_reasoning_effort(effort),
                None => config,
            };
            let config = match preferences.context_tier.as_deref() {
                Some(tier) => config.with_context_tier(tier),
                None => config,
            };
            let prepared =
                client
                    .prepare_session(config)
                    .map_err(|_error| AppError::CopilotRuntime {
                        host: app_session.host.name.clone(),
                        reason: "Could not prepare a Copilot SDK session".into(),
                    })?;
            let subscription = prepared.subscribe();
            let created = prepared
                .start()
                .await
                .map_err(|_error| AppError::CopilotRuntime {
                    host: app_session.host.name.clone(),
                    reason: "Could not create a Copilot SDK session".into(),
                })?;
            if created.id() != requested_id {
                return Err(AppError::CopilotRuntime {
                    host: app_session.host.name.clone(),
                    reason: "Copilot did not honor the resumable session ID".into(),
                });
            }
            let sdk_id = created.id().to_string();
            repository::set_copilot_session_id(
                &*self.sessions.db.connection()?,
                app_session_id,
                &sdk_id,
            )?;
            tracing::info!(session_id = %app_session_id, "Created Copilot SDK conversation");
            (created, subscription, "session.start")
        } else {
            let Some(sdk_id) = resume_id.as_deref() else {
                return Err(AppError::NotFound("No Copilot session ID is stored".into()));
            };
            let config = ResumeSessionConfig::new(SessionId::new(sdk_id))
                .with_enable_session_store(true)
                .with_working_directory(&app_session.workspace.repo_path)
                .with_reasoning_summary(ReasoningSummary::Concise)
                .with_mcp_servers(mcp_servers.clone())
                .with_disabled_mcp_servers(disabled_mcp_servers.clone())
                .with_permission_handler(handler.clone());
            let model = if selected.is_some() {
                None
            } else {
                preferences.model.as_deref()
            };
            let config = match model {
                Some(model) => config.with_model(model),
                None => config,
            };
            let effort = if selected.is_some() {
                None
            } else {
                preferences.reasoning_effort.as_deref()
            };
            let config = match effort {
                Some(effort) => config.with_reasoning_effort(effort),
                None => config,
            };
            let tier = if selected.is_some() {
                None
            } else {
                preferences.context_tier.as_deref()
            };
            let config = match tier {
                Some(tier) => config.with_context_tier(tier),
                None => config,
            };
            let prepared = client.prepare_resume_session(config).map_err(|error| {
                if selected.is_some() {
                    AppError::CopilotResume(format!(
                        "Could not prepare selected Copilot CLI conversation: {error}. Previous app chat link unchanged"
                    ))
                } else {
                    AppError::CopilotResume(format!(
                        "Could not prepare stored Copilot session: {error}"
                    ))
                }
            })?;
            let subscription = prepared.subscribe();
            let resumed = tokio::time::timeout(Duration::from_secs(20), prepared.start())
                .await
                .map_err(|_| AppError::CopilotResume(
                    "Timed out resuming the selected Copilot conversation; no app session link was changed".into()
                ))?;
            match resumed {
                Ok(resumed) => {
                    tracing::info!(session_id = %app_session_id, "Resumed stored Copilot conversation");
                    (resumed, subscription, "session.resume")
                }
                Err(error) => {
                    if selected.is_some() {
                        return Err(AppError::CopilotResume(format!(
                            "Selected Copilot CLI conversation could not be resumed: {error}. The previous app chat link was not changed"
                        )));
                    }
                    if !app_session.copilot_has_messages {
                        let empty = SessionConfig::default()
                            .with_session_id(SessionId::new(sdk_id))
                            .with_enable_session_store(true)
                            .with_working_directory(&app_session.workspace.repo_path)
                            .with_reasoning_summary(ReasoningSummary::Concise)
                            .with_mcp_servers(mcp_servers)
                            .with_disabled_mcp_servers(disabled_mcp_servers)
                            .with_permission_handler(handler);
                        let empty = match preferences.model.as_deref() {
                            Some(model) => empty.with_model(model),
                            None => empty,
                        };
                        let empty = match preferences.reasoning_effort.as_deref() {
                            Some(effort) => empty.with_reasoning_effort(effort),
                            None => empty,
                        };
                        let empty = match preferences.context_tier.as_deref() {
                            Some(tier) => empty.with_context_tier(tier),
                            None => empty,
                        };
                        let prepared = client.prepare_session(empty).map_err(|_error| {
                            AppError::CopilotResume(
                                "Empty Copilot session could not be prepared with its saved ID"
                                    .into(),
                            )
                        })?;
                        let subscription = prepared.subscribe();
                        let reopened = prepared.start().await.map_err(|_error| {
                            AppError::CopilotResume(
                                "Empty Copilot session could not be reopened with its saved ID"
                                    .into(),
                            )
                        })?;
                        if reopened.id().as_ref() != sdk_id {
                            return Err(AppError::CopilotResume(
                                "Copilot did not honor the saved session ID".into(),
                            ));
                        }
                        tracing::info!(session_id = %app_session_id, "Reopened empty Copilot session with its saved ID");
                        (reopened, subscription, "session.start")
                    } else {
                        tracing::warn!(session_id = %app_session_id, %error, "Stored Copilot session could not be resumed");
                        return Ok(AgentSnapshot::unavailable(
                            AgentStatus::SessionMissing,
                            prior_id,
                            "Copilot session could not be restored; the terminal is unaffected"
                                .into(),
                        ));
                    }
                }
            }
        };
        let early = verify_working_directory(
            &mut subscription,
            lifecycle_event,
            &app_session.workspace.repo_path,
            &app_session.host.name,
        )
        .await?;
        permission_active.store(true, Ordering::SeqCst);
        let history = tokio::time::timeout(Duration::from_secs(30), session.get_events())
            .await
            .map_err(|_| {
                AppError::CopilotResume("Timed out loading Copilot conversation history".into())
            })?
            .map_err(|_error| {
                AppError::CopilotResume(
                    "Copilot session connected, but conversation history could not be loaded"
                        .into(),
                )
            })?;
        if !self.is_current(app_session_id, generation)? {
            return Err(AppError::InvalidInput(
                "Copilot connection was superseded".into(),
            ));
        }
        let (mode, mode_error) = match Self::apply_mode(&session, preferences.autopilot).await {
            Ok(mode) => {
                autopilot.store(matches!(mode, ChatMode::Autopilot), Ordering::SeqCst);
                (mode, None)
            }
            Err(error) => {
                tracing::warn!(session_id = %app_session_id, %error, "Could not apply preferred Copilot mode");
                (ChatMode::Unsupported, Some(error.to_string()))
            }
        };
        if !matches!(mode, ChatMode::Autopilot) {
            reviewed_mcp_grant.store(false, Ordering::SeqCst);
        }
        let (model_state, model_error) = match Self::current_model(&session).await {
            Ok(state) => (Some(state), None),
            Err(error) => {
                tracing::warn!(session_id = %app_session_id, %error, "Could not read Copilot's active model");
                (None, Some(error.to_string()))
            }
        };
        let snapshot = AgentSnapshot {
            status: AgentStatus::Connected,
            copilot_session_id: Some(session.id().to_string()),
            events: events::history(&history),
            error: None,
            model: model_state
                .as_ref()
                .and_then(|state| state.current_model.clone()),
            context_tier: model_state
                .as_ref()
                .and_then(|state| state.context_tier.clone()),
            reasoning_effort: model_state
                .as_ref()
                .and_then(|state| state.reasoning_effort.clone()),
            model_error,
            mode,
            mode_error,
            mcp_warnings,
        };
        for event in early.iter().filter_map(events::normalize) {
            if on_event.send(event).is_err() {
                return Err(AppError::NotFound(
                    "Copilot chat UI disconnected during startup".into(),
                ));
            }
        }
        if let Some((selected_id, _)) = selected {
            if !self.is_current(app_session_id, generation)? {
                return Err(AppError::InvalidInput(
                    "Copilot connection was superseded before a CLI chat could be linked".into(),
                ));
            }
            repository::link_existing_copilot_session(
                &*self.sessions.db.connection()?,
                app_session_id,
                selected_id,
            )?;
            self.approval_mode(app_session_id)?
                .store(false, Ordering::SeqCst);
            self.revoke_mcp_grant(app_session_id)?;
            tracing::info!(app_session_id, copilot_session_id = %selected_id, "Linked selected existing CLI conversation");
        }
        if let Some(previous) = self.active.lock().await.remove(app_session_id) {
            previous.subscription.abort();
            previous.permission_active.store(false, Ordering::SeqCst);
            previous.pending.decline_all().await;
            update_sleep(&self.sleep, app_session_id, &previous.channel, false);
        }
        let session = Arc::new(session);
        let sleep = Arc::clone(&self.sleep);
        let stream_id = app_session_id.to_owned();
        let stream_channel = on_event.clone();
        let active_channel = on_event.clone();
        let stream = tokio::spawn(async move {
            loop {
                match subscription.recv().await {
                    Ok(event) => {
                        if events::child_scope(&event).is_none() {
                            match event.event_type.as_str() {
                                "assistant.turn_start" => {
                                    update_sleep(&sleep, &stream_id, &stream_channel, true)
                                }
                                "session.idle" | "session.error" | "abort" => {
                                    update_sleep(&sleep, &stream_id, &stream_channel, false)
                                }
                                _ => {}
                            }
                        }
                        if let Some(event) = events::normalize(&event) {
                            if on_event.send(event).is_err() {
                                break;
                            }
                        }
                    }
                    Err(error) => {
                        let message = match error.kind() {
                            RecvErrorKind::Lagged(_) => {
                                "Copilot event stream fell behind; reconnect to reload the stored history"
                            }
                            RecvErrorKind::Closed => "Copilot connection closed; reconnect to continue",
                            _ => "Copilot event stream failed; reconnect to continue",
                        };
                        let _ = on_event.send(AgentEvent::Disconnected {
                            event_id: ulid::Ulid::new().to_string(),
                            message: message.into(),
                        });
                        break;
                    }
                }
            }
            update_sleep(&sleep, &stream_id, &stream_channel, false);
        });
        self.active.lock().await.insert(
            app_session_id.into(),
            ActiveAgent {
                session,
                subscription: stream,
                pending,
                permission_active,
                autopilot,
                reviewed_mcp,
                reviewed_mcp_grant,
                channel: active_channel,
            },
        );
        Ok(snapshot)
    }

    pub async fn send(&self, app_session_id: &str, text: &str) -> Result<()> {
        if text.trim().is_empty() {
            return Err(AppError::InvalidInput("Message cannot be empty".into()));
        }
        let host_name = self.sessions.get(app_session_id)?.host.name;
        let (session, channel) = self
            .active
            .lock()
            .await
            .get(app_session_id)
            .map(|active| (active.session.clone(), active.channel.clone()))
            .ok_or_else(|| {
                AppError::NotFound("Connect Copilot chat before sending a message".into())
            })?;
        repository::mark_copilot_has_messages(&*self.sessions.db.connection()?, app_session_id)?;
        update_sleep(&self.sleep, app_session_id, &channel, true);
        let result = tokio::time::timeout(Duration::from_secs(20), session.send(text))
            .await
            .map_err(|_| AppError::CopilotRuntime {
                host: host_name.clone(),
                reason: "Message send timed out; it may have been accepted. Check history before resending".into(),
            })?
            .map_err(|_error| AppError::CopilotRuntime {
                host: host_name,
                reason: "Could not send message to Copilot".into(),
            });
        if result.is_err() {
            update_sleep(&self.sleep, app_session_id, &channel, false);
        }
        result?;
        Ok(())
    }

    pub async fn health(&self, app_session_id: &str) -> Result<bool> {
        let session = self.sessions.get(app_session_id)?;
        if !self.active.lock().await.contains_key(app_session_id) {
            return Ok(false);
        }
        self.runtime.is_healthy(&session.host.id).await
    }

    pub async fn pause_chat_tunnel(&self, app_session_id: &str) -> Result<()> {
        let host_id = self.sessions.get(app_session_id)?.host.id;
        let active_ids: Vec<_> = self.active.lock().await.keys().cloned().collect();
        let mut connected = Vec::new();
        for id in active_ids {
            if self.sessions.get(&id)?.host.id == host_id {
                if self.sleep.is_working(&id)? {
                    return Err(AppError::InvalidInput(
                        "Stop active Copilot work before releasing this host's Chat tunnel".into(),
                    ));
                }
                connected.push(id);
            }
        }
        let pause_result = self.runtime.pause_host(&host_id).await;
        for id in connected {
            if let Some(active) = self.active.lock().await.get(&id) {
                let _ = active.channel.send(AgentEvent::Disconnected {
                    event_id: ulid::Ulid::new().to_string(),
                    message: "Chat tunnel paused; resume it from Ports to reconnect".into(),
                });
            }
            self.disconnect(&id).await;
        }
        pause_result
    }

    pub async fn list_mcp(&self, app_session_id: &str) -> Result<McpInventory> {
        let host = self.sessions.get(app_session_id)?.host;
        let reviewed = mcp::list(&*self.sessions.db.connection()?, &host.id)?;
        let imports = mcp::list_imports(&*self.sessions.db.connection()?, &host.id)?;
        let client = match self.runtime.ensure(&host).await {
            Ok(client) => client,
            Err(error) => {
                return Ok(McpInventory {
                    reviewed: reviewed
                        .into_iter()
                        .map(|server| McpView {
                            name: server.name,
                            url: server.url,
                            status: "disconnected".into(),
                            error: None,
                        })
                        .collect(),
                    imported: imports
                        .into_iter()
                        .map(|item| mcp::ImportedMcpView {
                            name: item.name,
                            status: "disconnected".into(),
                            needs_review: false,
                            error: None,
                        })
                        .collect(),
                    available_cli: Vec::new(),
                    external: Vec::new(),
                    warning: Some(error.to_string()),
                });
            }
        };
        let global = mcp::user_config(&client).await?;
        let imported_by_name: HashMap<_, _> = imports
            .iter()
            .map(|item| (item.name.as_str(), item))
            .collect();
        let mut available_cli = Vec::new();
        for (name, config) in &global {
            available_cli.push(mcp::candidate(
                name.clone(),
                config,
                imported_by_name.get(name.as_str()).copied(),
            )?);
        }
        available_cli.sort_by(|a, b| a.name.cmp(&b.name));
        let reviewed_names: std::collections::HashSet<_> = reviewed
            .iter()
            .map(|server| server.name.as_str())
            .chain(imports.iter().map(|item| item.name.as_str()))
            .collect();
        let mut other: HashMap<String, String> = global
            .keys()
            .filter(|name| !reviewed_names.contains(name.as_str()))
            .map(|name| (name.clone(), format!("{name} - user configuration")))
            .collect();
        let session = self
            .active
            .lock()
            .await
            .get(app_session_id)
            .map(|active| active.session.clone());
        let mut statuses = HashMap::new();
        if let Some(session) = session {
            let live = tokio::time::timeout(Duration::from_secs(5), session.rpc().mcp().list())
                .await
                .map_err(|_| {
                    AppError::Operation(anyhow::anyhow!("Timed out listing active MCP servers"))
                })?
                .map_err(|error| {
                    AppError::Operation(anyhow::anyhow!(
                        "Could not list active MCP servers: {error}"
                    ))
                })?;
            for server in live.servers {
                let encoded = serde_json::to_value(&server.status).map_err(anyhow::Error::from)?;
                let status = encoded
                    .as_str()
                    .ok_or_else(|| {
                        AppError::InvalidInput("Copilot returned an invalid MCP status".into())
                    })?
                    .to_owned();
                if !reviewed_names.contains(server.name.as_str()) {
                    let source = server
                        .source
                        .map(|value| format!("{value:?}"))
                        .unwrap_or_else(|| "unknown source".into());
                    other.insert(
                        server.name.clone(),
                        format!("{} - {status} - {source}", server.name),
                    );
                }
                statuses.insert(
                    server.name,
                    (
                        status,
                        server.error.map(|error| error.chars().take(500).collect()),
                    ),
                );
            }
        }
        let mut external: Vec<_> = other.into_values().collect();
        external.sort();
        let imported = imports
            .into_iter()
            .map(|item| {
                let needs_review = global
                    .get(&item.name)
                    .map(|config| {
                        mcp::config_fingerprint(config).map(|current| current != item.config_sha256)
                    })
                    .transpose()?
                    .unwrap_or(true);
                let (status, error) = statuses
                    .remove(&item.name)
                    .unwrap_or_else(|| ("not-attached".into(), None));
                Ok(mcp::ImportedMcpView {
                    name: item.name,
                    status,
                    needs_review,
                    error,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(McpInventory {
            reviewed: reviewed
                .into_iter()
                .map(|server| {
                    let (status, error) = statuses
                        .remove(&server.name)
                        .unwrap_or_else(|| ("not-attached".into(), None));
                    McpView {
                        name: server.name,
                        url: server.url,
                        status,
                        error,
                    }
                })
                .collect(),
            imported,
            available_cli,
            external,
            warning: None,
        })
    }

    pub async fn add_mcp(
        &self,
        app_session_id: &str,
        name: &str,
        endpoint: &str,
        confirmed: bool,
    ) -> Result<()> {
        if !confirmed {
            return Err(AppError::InvalidInput(
                "Review and confirm the MCP server before adding it".into(),
            ));
        }
        let host = self.sessions.get(app_session_id)?.host;
        let url = mcp::validate(name, endpoint)?;
        let active_ids: Vec<_> = self.active.lock().await.keys().cloned().collect();
        for id in &active_ids {
            if self.sessions.get(id)?.host.id == host.id && self.sleep.is_working(id)? {
                return Err(AppError::InvalidInput(
                    "Stop active Copilot work before changing MCP servers on this host".into(),
                ));
            }
        }
        let client = self.runtime.ensure(&host).await?;
        mcp::session_settings(
            &client,
            &[mcp::ReviewedMcp {
                name: name.into(),
                url: url.clone(),
            }],
            &[],
        )
        .await?;
        mcp::add(&*self.sessions.db.connection()?, &host.id, name, &url)?;
        for id in &active_ids {
            if self.sessions.get(id)?.host.id == host.id {
                self.revoke_mcp_grant(id)?;
            }
        }
        let sessions: Vec<_> = self
            .active
            .lock()
            .await
            .iter()
            .map(|(id, active)| (id.clone(), active.session.clone()))
            .collect();
        for (id, session) in sessions {
            if self.sessions.get(&id)?.host.id != host.id {
                continue;
            }
            tokio::time::timeout(
                Duration::from_secs(10),
                session.rpc().mcp().start_server(McpStartServerRequest {
                    server_name: name.into(),
                    config: Some(serde_json::json!({ "type": "http", "url": url })),
                }),
            ).await
                .map_err(|_| AppError::Operation(anyhow::anyhow!("Reviewed MCP saved but live startup timed out; reconnect Chat to retry")))?
                .map_err(|error| AppError::Operation(anyhow::anyhow!(
                    "Reviewed MCP saved but could not start on this Chat: {error}; reconnect to retry"
                )))?;
            if let Some(active) = self.active.lock().await.get(&id) {
                active
                    .reviewed_mcp
                    .lock()
                    .map_err(|_| AppError::InvalidInput("Reviewed MCP grant lock poisoned".into()))?
                    .insert(name.into(), None);
            }
        }
        Ok(())
    }

    pub async fn import_cli_mcp(
        &self,
        app_session_id: &str,
        name: &str,
        confirmed: bool,
    ) -> Result<()> {
        if !confirmed {
            return Err(AppError::InvalidInput(
                "Review and confirm the remote Copilot CLI MCP server before enabling it".into(),
            ));
        }
        let host = self.sessions.get(app_session_id)?.host;
        if mcp::list(&*self.sessions.db.connection()?, &host.id)?
            .iter()
            .any(|item| item.name == name)
        {
            return Err(AppError::InvalidInput(
                "This name belongs to an app-managed HTTPS server".into(),
            ));
        }
        let client = self.runtime.ensure(&host).await?;
        let config = mcp::user_config(&client)
            .await?
            .remove(name)
            .ok_or_else(|| {
                AppError::NotFound(format!(
                    "No Copilot CLI MCP server named {name} is configured on this remote host"
                ))
            })?;
        let summary = mcp::candidate(name.into(), &config, None)?;
        if !matches!(
            summary.transport.as_str(),
            "stdio" | "local" | "http" | "sse"
        ) {
            return Err(AppError::InvalidInput(
                "Copilot CLI MCP transport is not supported for review in this app".into(),
            ));
        }
        if matches!(summary.transport.as_str(), "stdio" | "local")
            && config
                .get("command")
                .and_then(|value| value.as_str())
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err(AppError::InvalidInput(
                "Copilot CLI MCP stdio configuration is missing its command".into(),
            ));
        }
        if matches!(summary.transport.as_str(), "http" | "sse")
            && config
                .get("url")
                .and_then(|value| value.as_str())
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err(AppError::InvalidInput(
                "Copilot CLI MCP HTTP configuration is missing its URL".into(),
            ));
        }
        let fingerprint = mcp::config_fingerprint(&config)?;
        mcp::save_import(
            &*self.sessions.db.connection()?,
            &host.id,
            name,
            &fingerprint,
        )?;
        tracing::info!(host_id = %host.id, server_name = %name, "Reviewed existing Copilot CLI MCP for future Chat connections");
        Ok(())
    }

    pub async fn activate_cli_mcp(&self, app_session_id: &str, name: &str) -> Result<String> {
        let host = self.sessions.get(app_session_id)?.host;
        if self.sleep.is_working(app_session_id)? {
            return Err(AppError::InvalidInput(
                "Wait for active Copilot work before enabling an MCP server".into(),
            ));
        }
        let approved = mcp::list_imports(&*self.sessions.db.connection()?, &host.id)?
            .into_iter()
            .find(|item| item.name == name)
            .ok_or_else(|| {
                AppError::NotFound("Review this remote CLI MCP before enabling it".into())
            })?;
        let client = self.runtime.ensure(&host).await?;
        let config = mcp::user_config(&client)
            .await?
            .remove(name)
            .ok_or_else(|| {
                AppError::NotFound("Remote Copilot CLI MCP configuration is missing".into())
            })?;
        if mcp::config_fingerprint(&config)? != approved.config_sha256 {
            return Err(AppError::InvalidInput(
                "Remote CLI MCP configuration changed; review it again before enabling".into(),
            ));
        }
        let session = self
            .active
            .lock()
            .await
            .get(app_session_id)
            .map(|active| active.session.clone())
            .ok_or_else(|| {
                AppError::NotFound("Connect Chat before enabling its reviewed MCP".into())
            })?;
        tokio::time::timeout(
            Duration::from_secs(10),
            session.rpc().mcp().enable(McpEnableRequest {
                server_name: name.into(),
            }),
        )
        .await
        .map_err(|_| {
            AppError::Operation(anyhow::anyhow!("Timed out enabling reviewed MCP in Chat"))
        })?
        .map_err(|error| {
            AppError::Operation(anyhow::anyhow!(
                "Could not enable reviewed MCP in Chat: {error}"
            ))
        })?;
        let mut live = tokio::time::timeout(Duration::from_secs(5), session.rpc().mcp().list())
            .await
            .map_err(|_| {
                AppError::Operation(anyhow::anyhow!("Timed out verifying reviewed MCP status"))
            })?
            .map_err(|error| {
                AppError::Operation(anyhow::anyhow!(
                    "Could not verify reviewed MCP status: {error}"
                ))
            })?;
        let state = live
            .servers
            .iter()
            .find(|server| server.name == name)
            .map(|server| &server.status);
        if matches!(state, Some(McpServerStatus::NotConfigured) | None) {
            tokio::time::timeout(
                Duration::from_secs(10),
                session.rpc().mcp().start_server(McpStartServerRequest {
                    server_name: name.into(),
                    config: None,
                }),
            ).await
                .map_err(|_| AppError::Operation(anyhow::anyhow!("Timed out starting reviewed MCP from Copilot CLI configuration")))?
                .map_err(|error| AppError::Operation(anyhow::anyhow!(
                    "Could not start reviewed MCP from its remote Copilot CLI configuration: {error}"
                )))?;
            live = tokio::time::timeout(Duration::from_secs(5), session.rpc().mcp().list())
                .await
                .map_err(|_| {
                    AppError::Operation(anyhow::anyhow!(
                        "Timed out checking reviewed MCP after startup"
                    ))
                })?
                .map_err(|error| {
                    AppError::Operation(anyhow::anyhow!(
                        "Could not check reviewed MCP after startup: {error}"
                    ))
                })?;
        }
        let state = live
            .servers
            .iter()
            .find(|server| server.name == name)
            .ok_or_else(|| {
                AppError::NotFound(
                    "Reviewed MCP was not listed after activation; reconnect Chat to retry".into(),
                )
            })?;
        if matches!(
            &state.status,
            McpServerStatus::Disabled
                | McpServerStatus::Stopped
                | McpServerStatus::NotConfigured
                | McpServerStatus::Unknown
        ) {
            return Err(AppError::InvalidInput(format!(
                "Remote policy or session state still prevents {name} from starting; reconnect Chat and check its MCP status"
            )));
        }
        let status = serde_json::to_value(&state.status).map_err(anyhow::Error::from)?;
        self.revoke_mcp_grant(app_session_id)?;
        if let Some(active) = self.active.lock().await.get(app_session_id) {
            active
                .reviewed_mcp
                .lock()
                .map_err(|_| AppError::InvalidInput("Reviewed MCP grant lock poisoned".into()))?
                .insert(name.into(), Some(approved.config_sha256));
        }
        status
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| AppError::InvalidInput("Copilot returned an invalid MCP status".into()))
    }

    pub async fn remove_mcp(&self, app_session_id: &str, name: &str) -> Result<()> {
        let host = self.sessions.get(app_session_id)?.host;
        let is_http = mcp::list(&*self.sessions.db.connection()?, &host.id)?
            .iter()
            .any(|server| server.name == name);
        let is_imported = mcp::list_imports(&*self.sessions.db.connection()?, &host.id)?
            .iter()
            .any(|server| server.name == name);
        if !is_http && !is_imported {
            return Err(AppError::NotFound("Reviewed MCP server not found".into()));
        }
        let active_ids: Vec<_> = self.active.lock().await.keys().cloned().collect();
        for id in &active_ids {
            if self.sessions.get(id)?.host.id != host.id {
                continue;
            }
            if self.sleep.is_working(id)? {
                return Err(AppError::InvalidInput(
                    "Stop active Copilot work before removing an MCP server".into(),
                ));
            }
            let session = self
                .active
                .lock()
                .await
                .get(id)
                .map(|active| active.session.clone());
            if let Some(session) = session {
                let live = tokio::time::timeout(Duration::from_secs(5), session.rpc().mcp().list())
                    .await
                    .map_err(|_| {
                        AppError::Operation(anyhow::anyhow!("Timed out checking live MCP servers"))
                    })?
                    .map_err(|error| {
                        AppError::Operation(anyhow::anyhow!(
                            "Could not check live MCP servers: {error}"
                        ))
                    })?;
                if live.servers.iter().any(|server| {
                    server.name == name
                        && !matches!(
                            server.status,
                            McpServerStatus::Disabled
                                | McpServerStatus::Stopped
                                | McpServerStatus::NotConfigured
                                | McpServerStatus::Unknown
                        )
                }) {
                    tokio::time::timeout(
                        Duration::from_secs(5),
                        session.rpc().mcp().stop_server(McpStopServerRequest {
                            server_name: name.into(),
                        }),
                    )
                    .await
                    .map_err(|_| {
                        AppError::Operation(anyhow::anyhow!("Timed out stopping the MCP server"))
                    })?
                    .map_err(|error| {
                        AppError::Operation(anyhow::anyhow!(
                            "Could not stop the MCP server: {error}"
                        ))
                    })?;
                }
            }
        }
        if is_http {
            mcp::remove(&*self.sessions.db.connection()?, &host.id, name)?;
        } else {
            mcp::remove_import(&*self.sessions.db.connection()?, &host.id, name)?;
        }
        for id in &active_ids {
            if self.sessions.get(id)?.host.id == host.id {
                self.revoke_mcp_grant(id)?;
                if let Some(active) = self.active.lock().await.get(id) {
                    active
                        .reviewed_mcp
                        .lock()
                        .map_err(|_| {
                            AppError::InvalidInput("Reviewed MCP grant lock poisoned".into())
                        })?
                        .remove(name);
                }
            }
        }
        Ok(())
    }

    pub async fn auth_mcp(
        &self,
        app_session_id: &str,
        name: &str,
        force: bool,
    ) -> Result<McpAuthResult> {
        let host = self.sessions.get(app_session_id)?.host;
        if self.sleep.is_working(app_session_id)? {
            return Err(AppError::InvalidInput(
                "Wait for active Copilot work to finish before authenticating MCP".into(),
            ));
        }
        let is_http = mcp::list(&*self.sessions.db.connection()?, &host.id)?
            .iter()
            .any(|server| server.name == name);
        let imported = mcp::list_imports(&*self.sessions.db.connection()?, &host.id)?
            .into_iter()
            .find(|server| server.name == name);
        if !is_http && imported.is_none() {
            return Err(AppError::NotFound(
                "Reviewed MCP server not found on this host".into(),
            ));
        }
        if let Some(imported) = imported {
            let client = self.runtime.ensure(&host).await?;
            let config = mcp::user_config(&client)
                .await?
                .remove(name)
                .ok_or_else(|| {
                    AppError::NotFound("Imported Copilot CLI MCP configuration is missing".into())
                })?;
            if mcp::config_fingerprint(&config)? != imported.config_sha256 {
                return Err(AppError::InvalidInput(
                    "Copilot CLI MCP configuration changed; review it again before authenticating"
                        .into(),
                ));
            }
        }
        let session = self
            .active
            .lock()
            .await
            .get(app_session_id)
            .map(|active| active.session.clone())
            .ok_or_else(|| {
                AppError::NotFound("Connect Chat before authenticating its MCP server".into())
            })?;
        let auth = tokio::time::timeout(
            Duration::from_secs(10),
            session.rpc().mcp().oauth().login(McpOauthLoginRequest {
                server_name: name.into(),
                force_reauth: Some(force),
                client_name: Some("Copilot Remote UI".into()),
                callback_success_message: Some("Return to Copilot Remote UI on your Mac".into()),
                ..Default::default()
            }),
        )
        .await
        .map_err(|_| AppError::Operation(anyhow::anyhow!("Timed out starting MCP sign-in")))?
        .map_err(|error| {
            AppError::Operation(anyhow::anyhow!("Could not start MCP sign-in: {error}"))
        })?;
        let Some(url) = auth.authorization_url else {
            return Ok(McpAuthResult {
                authorization_url: None,
                note: "MCP is already authenticated; refresh its status to confirm.".into(),
            });
        };
        let note = match mcp::callback_port(&url)? {
            Some(port) => format!(
                "The callback listens on remote host's loopback port {port}. Open this URL in an approved browser on remote host; a Mac browser cannot complete this sign-in."
            ),
            None => "The SDK did not provide a safe local callback route. Open this URL in an approved browser on remote host; do not assume a Mac browser can complete it.".into(),
        };
        Ok(McpAuthResult {
            authorization_url: Some(url),
            note,
        })
    }

    pub async fn resume_chat_tunnel(&self, app_session_id: &str) -> Result<()> {
        self.runtime.resume_host(app_session_id).await
    }

    async fn current_model(session: &Session) -> Result<ModelSelection> {
        let current =
            tokio::time::timeout(Duration::from_secs(5), session.rpc().model().get_current())
                .await
                .map_err(|_| {
                    AppError::Operation(anyhow::anyhow!("Timed out reading Copilot's active model"))
                })?
                .map_err(|error| {
                    AppError::Operation(anyhow::anyhow!(
                        "Could not read Copilot's active model: {error}"
                    ))
                })?;
        Ok(ModelSelection {
            current_model: current.model_id,
            context_tier: current
                .context_tier
                .as_ref()
                .map(context_tier_name)
                .map(str::to_owned),
            reasoning_effort: current.reasoning_effort,
            pending: false,
            queued: false,
            warning: None,
        })
    }

    pub async fn list_models(&self, app_session_id: &str) -> Result<Vec<CopilotModel>> {
        let app_session = self.sessions.get(app_session_id)?;
        if !self.active.lock().await.contains_key(app_session_id) {
            return Err(AppError::NotFound(
                "Connect Copilot chat before listing models".into(),
            ));
        }
        let client = self.runtime.ensure(&app_session.host).await?;
        let models = tokio::time::timeout(Duration::from_secs(10), client.list_models())
            .await
            .map_err(|_| AppError::Operation(anyhow::anyhow!("Timed out loading Copilot models")))?
            .map_err(|error| {
                AppError::Operation(anyhow::anyhow!("Could not load Copilot models: {error}"))
            })?;
        Ok(models
            .into_iter()
            .map(|model| CopilotModel {
                name: if model.name.trim().is_empty() {
                    model.id.clone()
                } else {
                    model.name
                },
                id: model.id,
                supported_context_tiers: model
                    .supported_context_tiers
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|tier| matches!(tier.as_str(), "default" | "long_context"))
                    .collect(),
                max_context_window_tokens: model
                    .capabilities
                    .limits
                    .and_then(|limits| limits.max_context_window_tokens)
                    .filter(|limit| *limit > 0),
                supported_reasoning_efforts: model.supported_reasoning_efforts.unwrap_or_default(),
                default_reasoning_effort: model.default_reasoning_effort,
            })
            .collect())
    }

    pub async fn model_state(&self, app_session_id: &str) -> Result<ModelSelection> {
        let session = self
            .active
            .lock()
            .await
            .get(app_session_id)
            .map(|active| active.session.clone())
            .ok_or_else(|| AppError::NotFound("Copilot chat is not connected".into()))?;
        Self::current_model(&session).await
    }

    pub async fn set_model(
        &self,
        app_session_id: &str,
        model_id: &str,
        reasoning_effort: Option<&str>,
        context_tier: Option<&str>,
    ) -> Result<ModelSelection> {
        let model_id = model_id.trim();
        if model_id.is_empty() {
            return Err(AppError::InvalidInput("Choose a Copilot model".into()));
        }
        let context_tier = match context_tier {
            Some("default") => Some(ContextTier::Default),
            Some("long_context") => Some(ContextTier::LongContext),
            Some(_) => {
                return Err(AppError::InvalidInput(
                    "Context window must be Default or Long context".into(),
                ))
            }
            None => None,
        };
        let reasoning_effort = reasoning_effort
            .map(str::trim)
            .filter(|effort| !effort.is_empty());
        let session = self
            .active
            .lock()
            .await
            .get(app_session_id)
            .map(|active| active.session.clone())
            .ok_or_else(|| {
                AppError::NotFound("Connect Copilot chat before changing its model".into())
            })?;
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            session.rpc().model().switch_to(ModelSwitchToRequest {
                model_id: model_id.into(),
                reasoning_summary: Some(ReasoningSummary::Concise),
                reasoning_effort: reasoning_effort.map(str::to_owned),
                context_tier: context_tier.clone(),
                ..Default::default()
            }),
        )
        .await
        .map_err(|_| AppError::Operation(anyhow::anyhow!("Timed out changing Copilot model")))?
        .map_err(|error| {
            AppError::Operation(anyhow::anyhow!("Could not change Copilot model: {error}"))
        })?;
        let selection = model_switch_result(
            result,
            model_id,
            reasoning_effort,
            context_tier.as_ref().map(context_tier_name),
        )?;
        repository::set_copilot_preferences(
            &*self.sessions.db.connection()?,
            app_session_id,
            model_id,
            reasoning_effort,
            context_tier.as_ref().map(context_tier_name),
        )?;
        Ok(selection)
    }

    async fn apply_mode(session: &Session, enabled: bool) -> Result<ChatMode> {
        let current = tokio::time::timeout(Duration::from_secs(5), session.rpc().mode().get())
            .await
            .map_err(|_| {
                AppError::Operation(anyhow::anyhow!("Timed out reading Copilot session mode"))
            })?
            .map_err(|_| {
                AppError::Operation(anyhow::anyhow!(
                    "This Copilot headless runtime does not support session modes"
                ))
            })?;
        let requested = if enabled {
            SessionMode::Autopilot
        } else {
            SessionMode::Interactive
        };
        if current != requested {
            tokio::time::timeout(
                Duration::from_secs(5),
                session.rpc().mode().set(ModeSetRequest {
                    mode: requested.clone(),
                    expected_mode: Some(current),
                    ..Default::default()
                }),
            )
            .await
            .map_err(|_| {
                AppError::Operation(anyhow::anyhow!("Timed out changing Copilot session mode"))
            })?
            .map_err(|_| {
                AppError::Operation(anyhow::anyhow!("Could not change Copilot session mode"))
            })?;
        }
        let actual = tokio::time::timeout(Duration::from_secs(5), session.rpc().mode().get())
            .await
            .map_err(|_| {
                AppError::Operation(anyhow::anyhow!("Timed out verifying Copilot session mode"))
            })?
            .map_err(|_| {
                AppError::Operation(anyhow::anyhow!("Could not verify Copilot's session mode"))
            })?;
        if actual == requested {
            Ok(if enabled {
                ChatMode::Autopilot
            } else {
                ChatMode::Interactive
            })
        } else {
            Err(AppError::Operation(anyhow::anyhow!(
                "Copilot did not apply the requested session mode"
            )))
        }
    }

    pub async fn set_autopilot(&self, app_session_id: &str, enabled: bool) -> Result<ChatMode> {
        let (session, autopilot) = self
            .active
            .lock()
            .await
            .get(app_session_id)
            .map(|active| (active.session.clone(), active.autopilot.clone()))
            .ok_or_else(|| {
                AppError::NotFound("Connect Copilot chat before changing its mode".into())
            })?;
        let applied = Self::apply_mode(&session, enabled).await?;
        repository::set_copilot_autopilot(
            &*self.sessions.db.connection()?,
            app_session_id,
            enabled,
        )?;
        autopilot.store(enabled, Ordering::SeqCst);
        if !enabled {
            self.mcp_approval_mode(app_session_id)?
                .store(false, Ordering::SeqCst);
        }
        Ok(applied)
    }

    pub async fn abort(&self, app_session_id: &str) -> Result<()> {
        let (session, channel) = self
            .active
            .lock()
            .await
            .get(app_session_id)
            .map(|active| (active.session.clone(), active.channel.clone()))
            .ok_or_else(|| AppError::NotFound("Copilot chat is not connected".into()))?;
        tokio::time::timeout(Duration::from_secs(5), session.abort())
            .await
            .map_err(|_| {
                AppError::Operation(anyhow::anyhow!("Timed out stopping Copilot generation"))
            })?
            .map_err(|_error| {
                AppError::Operation(anyhow::anyhow!("Could not stop Copilot generation"))
            })?;
        update_sleep(&self.sleep, app_session_id, &channel, false);
        Ok(())
    }

    pub async fn respond_permission(
        &self,
        app_session_id: &str,
        request_id: &str,
        allow: bool,
    ) -> Result<()> {
        let pending = self
            .active
            .lock()
            .await
            .get(app_session_id)
            .map(|active| active.pending.clone())
            .ok_or_else(|| AppError::NotFound("Copilot chat is not connected".into()))?;
        let mut requests = pending.0.lock().await;
        if allow
            && requests
                .get(request_id)
                .is_some_and(|request| !request.displayable)
        {
            return Err(AppError::InvalidInput(
                "Cannot approve a request whose action details are unavailable".into(),
            ));
        }
        let request = requests
            .remove(request_id)
            .ok_or_else(|| AppError::NotFound("Permission request has expired".into()))?;
        drop(requests);
        let decision = if allow {
            PermissionResult::approve_once()
        } else {
            PermissionResult::reject(Some("Denied in Copilot Remote UI".into()))
        };
        request
            .sender
            .send(decision)
            .map_err(|_| AppError::NotFound("Permission request has expired".into()))
    }

    pub async fn disconnect(&self, app_session_id: &str) {
        if let Err(error) = self.reserve(app_session_id) {
            tracing::warn!(%error, "Could not cancel pending Copilot connection");
        }
        if let Some(active) = self.active.lock().await.remove(app_session_id) {
            active.subscription.abort();
            active.permission_active.store(false, Ordering::SeqCst);
            active.pending.decline_all().await;
            update_sleep(&self.sleep, app_session_id, &active.channel, false);
            tracing::info!(session_id = %app_session_id, "Detached Copilot chat UI");
        }
    }
}

#[cfg(test)]
#[path = "permission_tests.rs"]
mod tests;

#[cfg(test)]
mod model_tests {
    use super::*;
    use serde_json::{json, Value};

    fn response(value: Value) -> ModelSwitchToResult {
        serde_json::from_value(value).expect("valid SDK model switch response")
    }

    #[test]
    fn deferred_switch_keeps_old_model_without_a_false_policy_error() {
        let result = model_switch_result(
            response(json!({
                "deferred": true,
                "modelState": {
                    "modelId": "gpt-5.4", "contextTier": "default", "reasoningEffort": "medium"
                }
            })),
            "gpt-6-sol",
            None,
            None,
        )
        .expect("queued switch");
        assert!(result.pending);
        assert!(result.queued);
        assert_eq!(result.current_model.as_deref(), Some("gpt-5.4"));
        assert_eq!(result.context_tier.as_deref(), Some("default"));
        assert_eq!(result.reasoning_effort.as_deref(), Some("medium"));
    }

    #[test]
    fn immediate_switch_uses_the_sdk_response_as_authoritative_state() {
        let result = model_switch_result(
            response(json!({
                "modelId": "gpt-6-sol",
                "modelState": {
                    "modelId": "gpt-6-sol",
                    "contextTier": "long_context", "reasoningEffort": "xhigh"
                }
            })),
            "gpt-6-sol",
            Some("xhigh"),
            Some("long_context"),
        )
        .expect("applied switch");
        assert!(!result.pending);
        assert_eq!(result.current_model.as_deref(), Some("gpt-6-sol"));
        assert_eq!(result.context_tier.as_deref(), Some("long_context"));
        assert_eq!(result.reasoning_effort.as_deref(), Some("xhigh"));
    }

    #[test]
    fn unknown_switch_state_waits_for_confirmation_without_claiming_success() {
        let result = model_switch_result(response(json!({})), "gpt-6-sol", None, None)
            .expect("request was acknowledged");
        assert!(result.pending);
        assert!(!result.queued);
        assert!(result.current_model.is_none());
        assert!(result
            .warning
            .as_deref()
            .is_some_and(|text| text.contains("not confirmed")));
    }

    #[test]
    fn compaction_and_explicit_rejection_are_reported_without_saving_a_choice() {
        let confirmation = model_switch_result(
            response(json!({
                "status": "confirmation_required",
                "confirmation": {
                    "currentTokens": 150000, "targetLimit": 100000,
                    "targetModelDisplayName": "GPT-6 Sol"
                }
            })),
            "gpt-6-sol",
            None,
            None,
        );
        assert!(confirmation.is_err());
        let rejected = model_switch_result(
            response(json!({
                "status": "unavailable", "message": "Not offered on this host"
            })),
            "gpt-6-sol",
            None,
            None,
        );
        assert!(rejected.is_err());
    }

    #[test]
    fn same_model_does_not_claim_effort_or_context_applied_if_they_remain_old() {
        let result = model_switch_result(
            response(json!({
                "modelId": "gpt-6-sol",
                "modelState": {
                    "modelId": "gpt-6-sol",
                    "contextTier": "default", "reasoningEffort": "medium"
                }
            })),
            "gpt-6-sol",
            Some("xhigh"),
            Some("long_context"),
        )
        .expect("request was acknowledged");
        assert!(result.pending);
        assert!(!result.queued);
        assert_eq!(result.context_tier.as_deref(), Some("default"));
        assert_eq!(result.reasoning_effort.as_deref(), Some("medium"));
    }
}
