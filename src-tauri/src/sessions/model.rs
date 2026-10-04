use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub id: String,
    pub name: String,
    pub ssh_host: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub id: String,
    pub host_id: String,
    pub repo_path: String,
    pub display_name: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub tmux_session_name: String,
    pub copilot_session_id: Option<String>,
    pub copilot_has_messages: bool,
    pub command: String,
    pub pinned: bool,
    pub created_at: String,
    pub last_opened_at: Option<String>,
    pub host: Host,
    pub workspace: Workspace,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Running,
    Disconnected,
    Dead,
    HostUnavailable,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionWithStatus {
    #[serde(flatten)]
    pub session: Session,
    pub status: SessionStatus,
}
