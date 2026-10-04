use std::sync::Arc;

use tauri::ipc::Channel;
use tauri::State;

use crate::copilot::events::AgentEvent;
use crate::copilot::mcp::{McpAuthResult, McpInventory};
use crate::copilot::ports::PortInventory;
use crate::copilot::recent::RecentCliChat;
use crate::copilot::runtime::RemoteCopilotRuntime;
use crate::copilot::session::{
    AgentSnapshot, ChatMode, CopilotAgentBackend, CopilotModel, ModelSelection, PermissionMode,
};
use crate::error::Result;

#[tauri::command]
pub async fn connect_copilot_host(
    state: State<'_, Arc<RemoteCopilotRuntime>>,
    session_id: String,
) -> Result<()> {
    state.connect_session_host(&session_id).await
}

#[tauri::command]
pub async fn disconnect_copilot_host(
    state: State<'_, Arc<RemoteCopilotRuntime>>,
    host_id: String,
) -> Result<()> {
    state.disconnect_host(&host_id).await
}

#[tauri::command]
pub async fn list_port_forwards(
    state: State<'_, Arc<RemoteCopilotRuntime>>,
    session_id: String,
) -> Result<PortInventory> {
    state.ports(&session_id).await
}

#[tauri::command]
pub async fn pause_chat_tunnel(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
) -> Result<()> {
    state.pause_chat_tunnel(&session_id).await
}

#[tauri::command]
pub async fn resume_chat_tunnel(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
) -> Result<()> {
    state.resume_chat_tunnel(&session_id).await
}

#[tauri::command]
pub async fn list_mcp_servers(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
) -> Result<McpInventory> {
    state.list_mcp(&session_id).await
}

#[tauri::command]
pub async fn add_mcp_server(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    name: String,
    url: String,
    confirmed: bool,
) -> Result<()> {
    state.add_mcp(&session_id, &name, &url, confirmed).await
}

#[tauri::command]
pub async fn import_cli_mcp_server(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    name: String,
    confirmed: bool,
) -> Result<()> {
    state.import_cli_mcp(&session_id, &name, confirmed).await
}

#[tauri::command]
pub async fn activate_cli_mcp_server(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    name: String,
) -> Result<String> {
    state.activate_cli_mcp(&session_id, &name).await
}

#[tauri::command]
pub async fn remove_mcp_server(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    name: String,
) -> Result<()> {
    state.remove_mcp(&session_id, &name).await
}

#[tauri::command]
pub async fn authenticate_mcp_server(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    name: String,
    force_reauth: bool,
) -> Result<McpAuthResult> {
    state.auth_mcp(&session_id, &name, force_reauth).await
}

#[tauri::command]
pub async fn connect_chat(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    on_event: Channel<AgentEvent>,
) -> Result<AgentSnapshot> {
    state.connect(&session_id, on_event, false, false).await
}

#[tauri::command]
pub async fn list_recent_cli_chats(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
) -> Result<Vec<RecentCliChat>> {
    state.recent_cli_chats(&session_id).await
}

#[tauri::command]
pub async fn attach_cli_chat(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    selected_id: String,
    allow_different_repo: bool,
    on_event: Channel<AgentEvent>,
) -> Result<AgentSnapshot> {
    state
        .attach_cli_chat(&session_id, &selected_id, allow_different_repo, on_event)
        .await
}

#[tauri::command]
pub async fn create_chat(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    on_event: Channel<AgentEvent>,
) -> Result<AgentSnapshot> {
    state.connect(&session_id, on_event, true, false).await
}

#[tauri::command]
pub async fn replace_chat(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    on_event: Channel<AgentEvent>,
) -> Result<AgentSnapshot> {
    state.connect(&session_id, on_event, false, true).await
}

#[tauri::command]
pub async fn send_chat_message(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    text: String,
) -> Result<()> {
    state.send(&session_id, &text).await
}

#[tauri::command]
pub async fn abort_chat(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
) -> Result<()> {
    state.abort(&session_id).await
}

#[tauri::command]
pub async fn chat_health(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
) -> Result<bool> {
    state.health(&session_id).await
}

#[tauri::command]
pub async fn list_copilot_models(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
) -> Result<Vec<CopilotModel>> {
    state.list_models(&session_id).await
}

#[tauri::command]
pub async fn get_copilot_model_state(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
) -> Result<ModelSelection> {
    state.model_state(&session_id).await
}

#[tauri::command]
pub async fn set_copilot_model(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    model_id: String,
    reasoning_effort: Option<String>,
    context_tier: Option<String>,
) -> Result<ModelSelection> {
    state
        .set_model(
            &session_id,
            &model_id,
            reasoning_effort.as_deref(),
            context_tier.as_deref(),
        )
        .await
}

#[tauri::command]
pub async fn respond_copilot_permission(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    request_id: String,
    allow: bool,
) -> Result<()> {
    state
        .respond_permission(&session_id, &request_id, allow)
        .await
}

#[tauri::command]
pub async fn disconnect_chat(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
) -> Result<()> {
    state.disconnect(&session_id).await;
    Ok(())
}

#[tauri::command]
pub fn set_copilot_permission_mode(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    mode: PermissionMode,
    confirmed: bool,
) -> Result<PermissionMode> {
    state.set_permission_mode(&session_id, mode, confirmed)
}

#[tauri::command]
pub async fn reviewed_mcp_approval_state(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
) -> Result<crate::copilot::session::ReviewedMcpApprovalState> {
    state.reviewed_mcp_approval_state(&session_id).await
}

#[tauri::command]
pub async fn set_reviewed_mcp_approval(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    enabled: bool,
    confirmed: bool,
) -> Result<crate::copilot::session::ReviewedMcpApprovalState> {
    state
        .set_reviewed_mcp_approval(&session_id, enabled, confirmed)
        .await
}

#[tauri::command]
pub async fn set_copilot_autopilot(
    state: State<'_, Arc<CopilotAgentBackend>>,
    session_id: String,
    enabled: bool,
) -> Result<ChatMode> {
    state.set_autopilot(&session_id, enabled).await
}
