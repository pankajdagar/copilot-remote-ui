export interface Host {
  id: string;
  name: string;
  sshHost: string | null;
  createdAt: string;
}

export interface Workspace {
  id: string;
  hostId: string;
  repoPath: string;
  displayName: string;
  createdAt: string;
}

export type SessionStatus = "running" | "disconnected" | "dead" | "hostUnavailable";

export interface Session {
  id: string;
  workspaceId: string;
  name: string;
  tmuxSessionName: string;
  copilotSessionId: string | null;
  copilotHasMessages: boolean;
  command: string;
  pinned: boolean;
  createdAt: string;
  lastOpenedAt: string | null;
  host: Host;
  workspace: Workspace;
}

export interface SessionWithStatus extends Session {
  status: SessionStatus;
}

export type TerminalEvent =
  | { kind: "output"; bytes: number[] }
  | { kind: "closed"; message: string };

export interface TerminalDiagnostics {
  applicationMouse: boolean;
  alternateScreen: boolean;
  paneRows: number;
  clientRows: number | null;
  paneCount: number;
}

export type AgentConnectionStatus =
  | "connected" | "unattached" | "sessionMissing" | "paused" | "unavailable" | "hostOffline";

export interface PortInventory {
  chat: { localPort: number | null; remotePort: number | null; paused: boolean };
  configured: Array<{ direction: string; spec: string }>;
}

export interface McpInventory {
  reviewed: Array<{ name: string; url: string; status: string; error: string | null }>;
  imported: Array<{
    name: string; status: string; needsReview: boolean; error: string | null;
  }>;
  availableCli: Array<{
    name: string; transport: string; command: string | null;
    endpointHost: string | null; reviewed: boolean; needsReview: boolean;
  }>;
  external: string[];
  warning: string | null;
}

export interface McpAuthResult {
  authorizationUrl: string | null;
  note: string;
}

export type PermissionMode = "ask" | "allowAll";
export type ChatMode = "interactive" | "plan" | "autopilot" | "unsupported";

export type AgentRuntimeStatus =
  | "connecting" | "detached" | "idle" | "working" | "awaitingPermission" | "error" | "unavailable"
  | "hostOffline" | "unattached" | "sessionMissing" | "paused";

export type SessionTab = "chat" | "changes" | "terminal" | "ports" | "integrations";

export type AgentEvent =
  | { type: "userMessage"; eventId: string; messageId: string; content: string }
  | { type: "assistantDelta"; eventId: string; messageId: string; content: string }
  | { type: "assistantMessage"; eventId: string; messageId: string; content: string }
  | { type: "intent"; eventId: string; content: string }
  | { type: "reasoningDelta"; eventId: string; reasoningId: string; content: string }
  | { type: "reasoning"; eventId: string; reasoningId: string; content: string }
  | {
      type: "modelChanged"; eventId: string; modelId: string;
      contextTier: ContextTier | null; reasoningEffort: string | null;
    }
  | {
      type: "toolStarted"; eventId: string; toolCallId: string; toolName: string;
      description: string | null; command: string | null;
      arguments?: Array<{ label: string; value: string }> | null;
      argumentsWarning?: string | null;
    }
  | { type: "toolOutput"; eventId: string; toolCallId: string; output: string }
  | { type: "toolCompleted"; eventId: string; toolCallId: string; success: boolean; result: string | null }
  | { type: "toolProgress"; eventId: string; toolCallId: string; message: string }
  | {
      type: "subagentStarted"; eventId: string; agentId: string | null;
      toolCallId: string; agentName: string;
      displayName: string; description: string | null; model: string | null;
    }
  | {
      type: "subagentCompleted"; eventId: string; agentId: string | null;
      toolCallId: string; agentName: string; displayName: string; cancelled: boolean;
    }
  | {
      type: "subagentFailed"; eventId: string; agentId: string | null;
      toolCallId: string; agentName: string; displayName: string; error: string;
    }
  | {
      type: "taskComplete"; eventId: string; summary: string | null;
      outcome: string | null; success: boolean | null;
      reason: string | null; truncated: boolean;
    }
  | {
      type: "subagentEvent"; eventId: string; agentId: string | null;
      parentToolCallId: string | null; event: AgentEvent;
    }
  | {
      type: "permissionRequested"; eventId: string; requestId: string; kind: string;
      command: string | null; description: string | null; warning: string | null;
      details: Array<{ label: string; value: string }>; approvable: boolean;
      workingDirectory: string;
    }
  | {
      type: "permissionAutoApproved"; eventId: string; requestId: string;
      kind: string; command: string; workingDirectory: string; source: "autopilot" | "allowAll" | "reviewedMcp";
    }
  | { type: "working"; eventId: string }
  | { type: "idle"; eventId: string; aborted: boolean }
  | { type: "sleepStatus"; eventId: string; active: boolean; error: string | null }
  | { type: "error"; eventId: string; message: string }
  | { type: "disconnected"; eventId: string; message: string };

export interface AgentSnapshot {
  status: AgentConnectionStatus;
  copilotSessionId: string | null;
  mode: ChatMode;
  modeError: string | null;
  mcpWarnings: string[];
  model: string | null;
  contextTier: ContextTier | null;
  reasoningEffort: string | null;
  modelError: string | null;
  events: AgentEvent[];
  error: string | null;
}

export interface RecentCliChat {
  id: string;
  summary: string | null;
  startedAt: string;
  modifiedAt: string;
  sameWorkspace: boolean;
  isRemote: boolean;
  linkedTo: string | null;
}

export type ContextTier = "default" | "long_context" | "unknown";

export interface CopilotModel {
  id: string;
  name: string;
  supportedContextTiers: ContextTier[];
  maxContextWindowTokens: number | null;
  supportedReasoningEfforts: string[];
  defaultReasoningEffort: string | null;
}

export interface ModelSelection {
  currentModel: string | null;
  contextTier: ContextTier | null;
  reasoningEffort: string | null;
  pending: boolean;
  queued: boolean;
  warning: string | null;
}

export interface ChangedFile {
  path: string;
  status: string;
}
