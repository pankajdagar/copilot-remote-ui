import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { Cable, GitBranch, MessageSquare, PlugZap, SquareTerminal } from "lucide-react";
import type { SessionWithStatus, SessionTab } from "../types";
import { touchSessionCache } from "../sessionCache";
import { useUi } from "../store";
import { ChangesPanel } from "./ChangesPanel";
import { PortsPanel } from "./PortsPanel";
import { IntegrationsPanel } from "./IntegrationsPanel";
import { SessionHeader } from "./SessionHeader";
import { TerminalToolbar } from "./TerminalToolbar";
import type { TerminalControls } from "./TerminalView";

const CachedTerminal = lazy(() => import("./CachedTerminal")
  .then((module) => ({ default: module.CachedTerminal })));
const CachedChat = lazy(() => import("./ChatPanel")
  .then((module) => ({ default: module.ChatPanel })));

export function MainPane({
  session,
  sessions,
  reconnectRequest,
  autoCreateChatIds,
  onSessionOpened,
  onRefresh,
  onRename,
  onPin,
  onRestart,
  onDelete,
  onError
}: {
  session: SessionWithStatus | undefined;
  sessions: SessionWithStatus[];
  reconnectRequest: { sessionId: string; nonce: number } | null;
  autoCreateChatIds: string[];
  onSessionOpened: (id: string) => Promise<void>;
  onRefresh: () => Promise<void>;
  onRename: () => void;
  onPin: () => void;
  onRestart: () => void;
  onDelete: () => void;
  onError: (message: string) => void;
}) {
  const controls = useRef(new Map<string, TerminalControls>());
  const [cachedIds, setCachedIds] = useState<string[]>([]);
  const [cachedChatIds, setCachedChatIds] = useState<string[]>([]);
  const [reconnectKeys, setReconnectKeys] = useState<Record<string, number>>({});
  const [messages, setMessages] = useState<Record<string, string>>({});
  const [mouseModes, setMouseModes] = useState<Record<string, string>>({});
  const [mouseReports, setMouseReports] = useState<Record<string, number>>({});
  const [changesCount, setChangesCount] = useState<number | null>(null);
  const connections = useUi((state) => state.connections);
  const agentStatuses = useUi((state) => state.agentStatuses);
  const permissionModes = useUi((state) => state.permissionModes);
  const tabs = useUi((state) => state.tabs);
  const setConnection = useUi((state) => state.setConnection);
  const setTab = useUi((state) => state.setTab);
  const id = session?.id;
  const tab: SessionTab = id ? tabs[id] ?? "chat" : "chat";
  const unavailable = session?.status === "hostUnavailable" || session?.status === "dead";

  useEffect(() => {
    if (!id || unavailable || tab !== "terminal") return;
    if (!cachedIds.includes(id)) setConnection(id, "connecting");
    setCachedIds((previous) => touchSessionCache(previous, id));
  }, [id, unavailable, tab, cachedIds, setConnection]);

  useEffect(() => {
    if (id && tab === "chat") {
      setCachedChatIds((previous) => touchSessionCache(previous, id));
    }
  }, [id, tab]);

  useEffect(() => {
    const known = new Set(sessions.map((item) => item.id));
    setCachedIds((previous) => {
      const remaining = previous.filter((cached) => known.has(cached));
      return remaining.length === previous.length ? previous : remaining;
    });
    setCachedChatIds((previous) => {
      const remaining = previous.filter((cached) => known.has(cached));
      return remaining.length === previous.length ? previous : remaining;
    });
  }, [sessions]);

  useEffect(() => {
    if (!reconnectRequest) return;
    const target = reconnectRequest.sessionId;
    setConnection(target, "connecting");
    setReconnectKeys((previous) => ({ ...previous, [target]: (previous[target] ?? 0) + 1 }));
  }, [reconnectRequest, setConnection]);

  useEffect(() => {
    if (id) void onSessionOpened(id);
    setChangesCount(null);
  }, [id, onSessionOpened]);

  const connected = useCallback((sessionId: string) => {
    setConnection(sessionId, "connected");
    setMessages((previous) => ({ ...previous, [sessionId]: "" }));
  }, [setConnection]);
  const disconnected = useCallback((sessionId: string, reason: string) => {
    setConnection(sessionId, "disconnected");
    setMessages((previous) => ({ ...previous, [sessionId]: reason }));
    void onRefresh();
  }, [setConnection, onRefresh]);
  const terminalError = useCallback((sessionId: string, reason: string) => {
    setConnection(sessionId, "error");
    setMessages((previous) => ({ ...previous, [sessionId]: reason }));
    onError(reason);
  }, [setConnection, onError]);
  const mouseMode = useCallback((sessionId: string, mode: string) => {
    setMouseModes((previous) => ({ ...previous, [sessionId]: mode }));
  }, []);
  const mouseReport = useCallback((sessionId: string, count: number) => {
    setMouseReports((previous) => ({ ...previous, [sessionId]: count }));
  }, []);
  const evicted = useCallback((sessionId: string) => setConnection(sessionId, "disconnected"), [setConnection]);

  const status = unavailable
    ? session?.status === "dead" ? "tmux session missing" : "Host unavailable"
    : (id && connections[id]) || "disconnected";
  const terminalStatus = status === "connected" ? "Running"
    : status === "connecting" ? "Connecting"
    : status === "disconnected" ? "Detached" : status;
  const copilotStatus = id ? ({
    connecting: "Connecting", detached: "Detached", working: "Working", idle: "Idle",
    awaitingPermission: "Approval needed", error: "Error", unavailable: "Unavailable",
    hostOffline: "Chat SSH unavailable", sessionMissing: "Session missing", unattached: "No chat attached",
    paused: "Chat paused"
  } as const)[agentStatuses[id]] ?? (session?.copilotSessionId ? "Not connected" : "No chat attached")
    : "No chat attached";

  return (
    <main className="flex min-h-0 min-w-0 flex-1 flex-col bg-canvas">
      {session && (
        <>
          <SessionHeader
            session={session}
            copilotStatus={copilotStatus}
            terminalStatus={terminalStatus}
            allowAll={permissionModes[session.id] === "allowAll"}
            onRename={onRename}
            onPin={onPin}
            onRestart={onRestart}
            onDelete={onDelete}
          />
          <nav className="flex gap-1 overflow-x-auto border-b border-white/10 bg-panel px-5 py-2" aria-label="Session tabs">
            {(["chat", "changes", "terminal", "ports", "integrations"] as const).map((item) => (
              <button key={item} type="button" aria-current={tab === item ? "page" : undefined}
                className={`flex shrink-0 items-center gap-2 rounded-lg px-3 py-2 text-xs font-medium capitalize transition-colors ${
                  tab === item ? "bg-raised text-white shadow-sm ring-1 ring-white/10" : "text-slate-400 hover:bg-raised/70 hover:text-white"
                }`} onClick={() => setTab(session.id, item)}>
                {item === "chat" ? <MessageSquare size={14} aria-hidden /> :
                  item === "changes" ? <GitBranch size={14} aria-hidden /> :
                    item === "terminal" ? <SquareTerminal size={14} aria-hidden /> :
                      item === "ports" ? <Cable size={14} aria-hidden /> : <PlugZap size={14} aria-hidden />}
                {item}{item === "changes" && changesCount !== null ? ` ${changesCount}` : ""}
              </button>
            ))}
          </nav>
          {tab === "terminal" && (
            <>
              <TerminalToolbar
                key={session.id}
                status={status}
                mouseMode={mouseModes[session.id] ?? "none"}
                mouseReports={mouseReports[session.id] ?? 0}
                terminal={() => controls.current.get(session.id) ?? null}
                onReconnect={() => {
                  setConnection(session.id, "connecting");
                  setReconnectKeys((previous) => ({
                    ...previous,
                    [session.id]: (previous[session.id] ?? 0) + 1
                  }));
                }}
                onError={onError}
              />
              {messages[session.id] && (
                <p className="border-b border-amber-500/20 bg-amber-500/10 px-5 py-2 text-xs text-amber-200" role="status">
                  {messages[session.id]}
                </p>
              )}
            </>
          )}
        </>
      )}
      <div className="flex min-h-0 flex-1 flex-col">
        {cachedIds.filter((cached) => sessions.some((item) => item.id === cached)).map((cached) => (
          <Suspense key={cached} fallback={
            cached === id && tab === "terminal" ? <p className="m-auto text-sm text-slate-400">Opening terminal...</p> : null
          }>
            <CachedTerminal
              sessionId={cached}
              active={cached === id && tab === "terminal" && !unavailable}
              reconnectKey={reconnectKeys[cached] ?? 0}
              controls={controls}
              onConnected={connected}
              onDisconnected={disconnected}
              onMouseModeChange={mouseMode}
              onMouseReports={mouseReport}
              onError={terminalError}
              onEvicted={evicted}
            />
          </Suspense>
        ))}
        {!session && (
          <div className="flex flex-1 items-center justify-center p-8 text-center">
            <div>
              <div className="mb-4 text-4xl text-accent">⌘</div>
              <h2 className="text-xl font-semibold">Choose a session</h2>
              <p className="mt-2 text-sm text-slate-400">Select a session or create a new one to start working.</p>
            </div>
          </div>
        )}
        {cachedChatIds.filter((cached) => sessions.some((item) => item.id === cached)).map((cached) => (
          <div key={cached} className={cached === id && tab === "chat" ? "flex min-h-0 flex-1" : "hidden"}
            aria-hidden={cached !== id || tab !== "chat"}>
            <Suspense fallback={cached === id && tab === "chat" ?
              <p className="m-auto text-sm text-slate-400">Opening Copilot chat...</p> : null}>
              <CachedChat sessionId={cached} active={cached === id && tab === "chat"}
                autoCreate={autoCreateChatIds.includes(cached)}
                workspacePath={sessions.find((item) => item.id === cached)?.workspace.repoPath}
                onLinked={onSessionOpened} onError={onError} />
            </Suspense>
          </div>
        ))}
        {session && tab === "chat" && !cachedChatIds.includes(session.id) && (
          <p className="m-auto text-sm text-slate-400">Connecting Copilot chat...</p>
        )}
        {session && tab === "changes" && (
          <ChangesPanel key={session.id} sessionId={session.id} onCount={setChangesCount} />
        )}
        {session && tab === "ports" && (
          <PortsPanel key={session.id} sessionId={session.id} host={session.host.name} />
        )}
        {session && tab === "integrations" && (
          <IntegrationsPanel key={session.id} sessionId={session.id} host={session.host.name} />
        )}
        {session && tab === "terminal" && unavailable && (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
            <p className="text-lg font-medium">{status}</p>
            <p className="max-w-md text-sm text-slate-400">
              {session.status === "dead"
                ? "The tmux session no longer exists. Restart to launch the saved command again."
                : "SSH or tmux is unreachable. Check your host connection and refresh statuses; the saved session will not be deleted."}
            </p>
            <button className="secondary-button" type="button" onClick={() => void onRefresh()}>Check again</button>
          </div>
        )}
        {session && tab === "terminal" && !unavailable && !cachedIds.includes(session.id) && (
          <p className="m-auto text-sm text-slate-400">Connecting terminal...</p>
        )}
      </div>
    </main>
  );
}
