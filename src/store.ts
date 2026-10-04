import { create } from "zustand";
import type { AgentRuntimeStatus, PermissionMode, SessionTab } from "./types";

type ConnectionState = "connecting" | "connected" | "disconnected" | "error";

interface UiState {
  activeSessionId: string | null;
  search: string;
  sidebarOpen: boolean;
  connections: Record<string, ConnectionState>;
  agentStatuses: Record<string, AgentRuntimeStatus>;
  permissionModes: Record<string, PermissionMode>;
  tabs: Record<string, SessionTab>;
  select: (id: string | null) => void;
  setSearch: (search: string) => void;
  toggleSidebar: () => void;
  setConnection: (id: string, status: ConnectionState) => void;
  setAgentStatus: (id: string, status: AgentRuntimeStatus) => void;
  setPermissionMode: (id: string, mode: PermissionMode) => void;
  setTab: (id: string, tab: SessionTab) => void;
}

export const useUi = create<UiState>((set) => ({
  activeSessionId: null,
  search: "",
  sidebarOpen: true,
  connections: {},
  agentStatuses: {},
  permissionModes: {},
  tabs: {},
  select: (activeSessionId) => set({ activeSessionId }),
  setSearch: (search) => set({ search }),
  toggleSidebar: () => set((state) => ({ sidebarOpen: !state.sidebarOpen })),
  setConnection: (id, status) =>
    set((state) => ({ connections: { ...state.connections, [id]: status } })),
  setAgentStatus: (id, status) =>
    set((state) => ({ agentStatuses: { ...state.agentStatuses, [id]: status } })),
  setPermissionMode: (id, mode) =>
    set((state) => ({ permissionModes: { ...state.permissionModes, [id]: mode } })),
  setTab: (id, tab) =>
    set((state) => ({ tabs: { ...state.tabs, [id]: tab } }))
}));
