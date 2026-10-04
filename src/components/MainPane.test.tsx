// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useEffect } from "react";

import { useUi } from "../store";
import type { SessionWithStatus } from "../types";
import { MainPane } from "./MainPane";

const lifecycle = vi.hoisted(() => ({ mounts: [] as string[], evictions: [] as string[] }));
const chatLifecycle = vi.hoisted(() => ({ mounts: [] as string[], evictions: [] as string[] }));

vi.mock("./CachedTerminal", () => ({
  CachedTerminal: ({
    sessionId, active, reconnectKey, onEvicted
  }: {
    sessionId: string;
    active: boolean;
    reconnectKey: number;
    onEvicted: (id: string) => void;
  }) => {
    useEffect(() => {
      lifecycle.mounts.push(sessionId);
      return () => {
        lifecycle.evictions.push(sessionId);
        onEvicted(sessionId);
      };
    }, [sessionId, onEvicted]);
    return <div data-testid={`cached-${sessionId}`} data-active={active} data-reconnect={reconnectKey} />;
  }
}));
vi.mock("./ChatPanel", () => ({
  ChatPanel: ({ sessionId, active }: { sessionId: string; active: boolean }) => {
    useEffect(() => {
      chatLifecycle.mounts.push(sessionId);
      return () => { chatLifecycle.evictions.push(sessionId); };
    }, [sessionId]);
    return <div data-testid={`chat-${sessionId}`} data-active={active}>Chat</div>;
  }
}));
vi.mock("./ChangesPanel", () => ({
  ChangesPanel: () => <div data-testid="changes-panel">Changes</div>
}));
vi.mock("./PortsPanel", () => ({
  PortsPanel: () => <div data-testid="ports-panel">Ports</div>
}));
vi.mock("./IntegrationsPanel", () => ({
  IntegrationsPanel: () => <div data-testid="integrations-panel">MCP</div>
}));

function session(id: string): SessionWithStatus {
  return {
    id,
    workspaceId: "workspace",
    name: id,
    tmuxSessionName: `crui_${id}`,
    copilotSessionId: null,
    copilotHasMessages: false,
    command: "copilot",
    pinned: false,
    createdAt: "2026-09-25T00:00:00Z",
    lastOpenedAt: null,
    status: "disconnected",
    host: { id: "host", name: "Remote", sshHost: "remote", createdAt: "2026-09-25T00:00:00Z" },
    workspace: {
      id: "workspace", hostId: "host", repoPath: "/home/coder/repo",
      displayName: "repo", createdAt: "2026-09-25T00:00:00Z"
    }
  };
}

describe("MainPane terminal cache", () => {
  afterEach(cleanup);

  beforeEach(() => {
    lifecycle.mounts.length = 0;
    lifecycle.evictions.length = 0;
    chatLifecycle.mounts.length = 0;
    chatLifecycle.evictions.length = 0;
    useUi.setState({
      connections: {}, agentStatuses: {}, permissionModes: {}, activeSessionId: null,
      tabs: { a: "terminal", b: "terminal", c: "terminal", d: "terminal", e: "terminal" }
    });
  });

  it("reuses warmed terminals and evicts only the least recently used", async () => {
    const sessions = ["a", "b", "c", "d", "e"].map(session);
    const onRefresh = vi.fn(async () => {});
    const props = {
      sessions, reconnectRequest: null, autoCreateChatIds: [],
      onRefresh, onSessionOpened: vi.fn(async () => {}),
      onRename: vi.fn(), onPin: vi.fn(), onRestart: vi.fn(), onDelete: vi.fn(), onError: vi.fn()
    };
    const { rerender } = render(<MainPane {...props} session={sessions[0]} />);
    await waitFor(() => expect(screen.getByTestId("cached-a")).toBeTruthy());

    rerender(<MainPane {...props} session={sessions[1]} />);
    await waitFor(() => expect(screen.getByTestId("cached-b")).toBeTruthy());
    expect(screen.getByTestId("cached-a").getAttribute("data-active")).toBe("false");
    rerender(<MainPane {...props} session={sessions[0]} />);
    expect(lifecycle.mounts).toEqual(["a", "b"]);
    expect(screen.getByTestId("cached-a").getAttribute("data-active")).toBe("true");
    expect(onRefresh).not.toHaveBeenCalled();

    rerender(<MainPane {...props} session={sessions[2]} />);
    rerender(<MainPane {...props} session={sessions[3]} />);
    rerender(<MainPane {...props} session={sessions[4]} />);
    await waitFor(() => expect(screen.getByTestId("cached-e")).toBeTruthy());
    expect(screen.queryByTestId("cached-b")).toBeNull();
    expect(lifecycle.evictions).toContain("b");
    expect(screen.getByTestId("cached-a")).toBeTruthy();
  });

  it("reconnects only the selected terminal and removes deleted views", async () => {
    const sessions = [session("a"), session("b")];
    const props = {
      sessions, reconnectRequest: null, autoCreateChatIds: [],
      onRefresh: vi.fn(async () => {}),
      onSessionOpened: vi.fn(async () => {}),
      onRename: vi.fn(), onPin: vi.fn(), onRestart: vi.fn(), onDelete: vi.fn(), onError: vi.fn()
    };
    const { rerender } = render(<MainPane {...props} session={sessions[0]} />);
    await waitFor(() => expect(screen.getByTestId("cached-a")).toBeTruthy());
    rerender(<MainPane {...props} session={sessions[1]} />);
    await waitFor(() => expect(screen.getByTestId("cached-b")).toBeTruthy());
    fireEvent.click(screen.getByText("Reconnect"));
    expect(screen.getByTestId("cached-b").getAttribute("data-reconnect")).toBe("1");
    expect(screen.getByTestId("cached-a").getAttribute("data-reconnect")).toBe("0");

    rerender(<MainPane {...props} session={sessions[0]} sessions={[sessions[0]]} />);
    await waitFor(() => expect(screen.queryByTestId("cached-b")).toBeNull());
    expect(screen.getByTestId("cached-a")).toBeTruthy();
  });

  it("defaults to Chat and keeps a warmed terminal alive across tabs", async () => {
    useUi.setState({ tabs: {} });
    const entry = session("chat-default");
    const props = {
      session: entry, sessions: [entry], autoCreateChatIds: [],
      reconnectRequest: null, onRefresh: vi.fn(async () => {}),
      onSessionOpened: vi.fn(async () => {}), onRename: vi.fn(),
      onPin: vi.fn(), onRestart: vi.fn(), onDelete: vi.fn(), onError: vi.fn()
    };
    render(<MainPane {...props} />);
    expect(screen.getByRole("main").classList.contains("min-h-0")).toBe(true);
    expect(await screen.findByTestId("chat-chat-default")).toBeTruthy();
    expect(screen.queryByTestId("cached-chat-default")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: /^terminal$/i }));
    await waitFor(() => expect(screen.getByTestId("cached-chat-default")).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: /^changes$/i }));
    expect(screen.getByTestId("changes-panel")).toBeTruthy();
    expect(screen.getByTestId("cached-chat-default").getAttribute("data-active")).toBe("false");
    expect(screen.getByTestId("chat-chat-default").getAttribute("data-active")).toBe("false");
    fireEvent.click(screen.getByRole("button", { name: /^chat$/i }));
    expect(screen.getByTestId("chat-chat-default").getAttribute("data-active")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: /^ports$/i }));
    expect(screen.getByTestId("ports-panel")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /^integrations$/i }));
    expect(screen.getByTestId("integrations-panel")).toBeTruthy();
    expect(chatLifecycle.mounts).toEqual(["chat-default"]);
    expect(chatLifecycle.evictions).toEqual([]);
    expect(lifecycle.evictions).not.toContain("chat-default");
  });

  it("keeps SDK chat views subscribed when switching logical sessions", async () => {
    useUi.setState({ tabs: {} });
    const sessions = [session("a"), session("b")];
    const props = {
      sessions, autoCreateChatIds: [], reconnectRequest: null,
      onRefresh: vi.fn(async () => {}), onSessionOpened: vi.fn(async () => {}),
      onRename: vi.fn(), onPin: vi.fn(), onRestart: vi.fn(), onDelete: vi.fn(), onError: vi.fn()
    };
    const { rerender } = render(<MainPane {...props} session={sessions[0]} />);
    await waitFor(() => expect(screen.getByTestId("chat-a")).toBeTruthy());
    rerender(<MainPane {...props} session={sessions[1]} />);
    await waitFor(() => expect(screen.getByTestId("chat-b")).toBeTruthy());
    expect(screen.getByTestId("chat-a").getAttribute("data-active")).toBe("false");
    rerender(<MainPane {...props} session={sessions[0]} />);
    expect(screen.getByTestId("chat-a").getAttribute("data-active")).toBe("true");
    expect(chatLifecycle.mounts).toEqual(["a", "b"]);
    expect(chatLifecycle.evictions).toEqual([]);
  });

  it("keeps Chat available when only the tmux terminal has disappeared", async () => {
    useUi.setState({ tabs: {} });
    const missing = { ...session("missing-terminal"), status: "dead" as const, copilotSessionId: "sdk-1" };
    const props = {
      session: missing, sessions: [missing], autoCreateChatIds: [], reconnectRequest: null,
      onRefresh: vi.fn(async () => {}), onSessionOpened: vi.fn(async () => {}),
      onRename: vi.fn(), onPin: vi.fn(), onRestart: vi.fn(), onDelete: vi.fn(), onError: vi.fn()
    };
    render(<MainPane {...props} />);
    expect(screen.getByTestId("chat-missing-terminal")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /^terminal$/i }));
    expect(screen.getByText(/The tmux session no longer exists/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Session actions" }));
    expect(screen.getByRole("button", { name: "Create terminal" })).toBeTruthy();
  });
});
