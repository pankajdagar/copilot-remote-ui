import { Channel, invoke } from "@tauri-apps/api/core";
import type {
  AgentEvent, AgentSnapshot, ChatMode, ChangedFile, ContextTier, CopilotModel, Host, ModelSelection,
  Session, SessionWithStatus, McpAuthResult, McpInventory, PermissionMode, PortInventory,
  RecentCliChat,
  TerminalDiagnostics, TerminalEvent, Workspace
} from "./types";

export const api = {
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  listHosts: () => invoke<Host[]>("list_hosts"),
  listAllWorkspaces: () => invoke<Workspace[]>("list_all_workspaces"),
  addHost: (name: string, sshHost: string) =>
    invoke<Host>("add_host", { name, sshHost }),
  listWorkspaces: (hostId: string) =>
    invoke<Workspace[]>("list_workspaces", { hostId }),
  addWorkspace: (hostId: string, repoPath: string, displayName: string) =>
    invoke<Workspace>("add_workspace", { hostId, repoPath, displayName }),
  listSessions: () => invoke<SessionWithStatus[]>("list_sessions"),
  listChanges: (sessionId: string) => invoke<ChangedFile[]>("list_changes", { sessionId }),
  touchSession: (sessionId: string) =>
    invoke<Session>("touch_session", { sessionId }),
  createSession: (workspaceId: string, name: string, command: string, cols: number, rows: number) =>
    invoke<Session>("create_session", { workspaceId, name, command, cols, rows }),
  attachSession: (sessionId: string, cols: number, rows: number, onData: Channel<TerminalEvent>) =>
    invoke<string>("attach_session", { sessionId, cols, rows, onData }),
  renameSession: (sessionId: string, name: string) =>
    invoke<Session>("rename_session", { sessionId, name }),
  pinSession: (sessionId: string, pinned: boolean) =>
    invoke<Session>("pin_session", { sessionId, pinned }),
  deleteSession: (sessionId: string) =>
    invoke<void>("delete_session", { sessionId }),
  forgetSession: (sessionId: string) =>
    invoke<void>("forget_session", { sessionId }),
  connectChat: (sessionId: string, onEvent: Channel<AgentEvent>) =>
    invoke<AgentSnapshot>("connect_chat", { sessionId, onEvent }),
  listRecentCliChats: (sessionId: string) =>
    invoke<RecentCliChat[]>("list_recent_cli_chats", { sessionId }),
  attachCliChat: (
    sessionId: string, selectedId: string, allowDifferentRepo: boolean, onEvent: Channel<AgentEvent>
  ) => invoke<AgentSnapshot>("attach_cli_chat", { sessionId, selectedId, allowDifferentRepo, onEvent }),
  createChat: (sessionId: string, onEvent: Channel<AgentEvent>) =>
    invoke<AgentSnapshot>("create_chat", { sessionId, onEvent }),
  replaceChat: (sessionId: string, onEvent: Channel<AgentEvent>) =>
    invoke<AgentSnapshot>("replace_chat", { sessionId, onEvent }),
  sendChatMessage: (sessionId: string, text: string) =>
    invoke<void>("send_chat_message", { sessionId, text }),
  abortChat: (sessionId: string) =>
    invoke<void>("abort_chat", { sessionId }),
  chatHealth: (sessionId: string) =>
    invoke<boolean>("chat_health", { sessionId }),
  listPortForwards: (sessionId: string) =>
    invoke<PortInventory>("list_port_forwards", { sessionId }),
  pauseChatTunnel: (sessionId: string) =>
    invoke<void>("pause_chat_tunnel", { sessionId }),
  resumeChatTunnel: (sessionId: string) =>
    invoke<void>("resume_chat_tunnel", { sessionId }),
  listMcpServers: (sessionId: string) =>
    invoke<McpInventory>("list_mcp_servers", { sessionId }),
  addMcpServer: (sessionId: string, name: string, url: string, confirmed: boolean) =>
    invoke<void>("add_mcp_server", { sessionId, name, url, confirmed }),
  importCliMcpServer: (sessionId: string, name: string, confirmed: boolean) =>
    invoke<void>("import_cli_mcp_server", { sessionId, name, confirmed }),
  activateCliMcpServer: (sessionId: string, name: string) =>
    invoke<string>("activate_cli_mcp_server", { sessionId, name }),
  removeMcpServer: (sessionId: string, name: string) =>
    invoke<void>("remove_mcp_server", { sessionId, name }),
  authenticateMcpServer: (sessionId: string, name: string, forceReauth: boolean) =>
    invoke<McpAuthResult>("authenticate_mcp_server", { sessionId, name, forceReauth }),
  listCopilotModels: (sessionId: string) =>
    invoke<CopilotModel[]>("list_copilot_models", { sessionId }),
  getCopilotModelState: (sessionId: string) =>
    invoke<ModelSelection>("get_copilot_model_state", { sessionId }),
  setCopilotModel: (sessionId: string, modelId: string, reasoningEffort: string | null,
    contextTier: ContextTier | null) =>
    invoke<ModelSelection>("set_copilot_model", { sessionId, modelId, reasoningEffort, contextTier }),
  respondCopilotPermission: (sessionId: string, requestId: string, allow: boolean) =>
    invoke<void>("respond_copilot_permission", { sessionId, requestId, allow }),
  setCopilotPermissionMode: (sessionId: string, mode: PermissionMode, confirmed: boolean) =>
    invoke<PermissionMode>("set_copilot_permission_mode", { sessionId, mode, confirmed }),
  reviewedMcpApprovalState: (sessionId: string) =>
    invoke<{ available: string[]; enabled: boolean }>("reviewed_mcp_approval_state", { sessionId }),
  setReviewedMcpApproval: (sessionId: string, enabled: boolean, confirmed: boolean) =>
    invoke<{ available: string[]; enabled: boolean }>("set_reviewed_mcp_approval", {
      sessionId, enabled, confirmed
    }),
  setCopilotAutopilot: (sessionId: string, enabled: boolean) =>
    invoke<ChatMode>("set_copilot_autopilot", { sessionId, enabled }),
  disconnectChat: (sessionId: string) =>
    invoke<void>("disconnect_chat", { sessionId }),
  restartSession: (sessionId: string, cols: number, rows: number) =>
    invoke<Session>("restart_session", { sessionId, cols, rows }),
  writeTerminal: (sessionId: string, connectionId: string, bytes: Uint8Array) =>
    invoke<void>("write_terminal", { sessionId, connectionId, bytes: Array.from(bytes) }),
  resizeTerminal: (sessionId: string, connectionId: string, cols: number, rows: number) =>
    invoke<void>("resize_terminal", { sessionId, connectionId, cols, rows }),
  syncTerminalSize: (sessionId: string, connectionId: string, cols: number, rows: number) =>
    invoke<TerminalDiagnostics>("sync_terminal_size", { sessionId, connectionId, cols, rows }),
  enterScrollback: (sessionId: string, connectionId: string) =>
    invoke<void>("enter_scrollback", { sessionId, connectionId }),
  disconnectTerminal: (sessionId: string, connectionId: string) =>
    invoke<void>("disconnect_terminal", { sessionId, connectionId })
};

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
