import { useCallback, useEffect, useRef, useState, type FormEvent } from "react";
import { api, errorMessage } from "./api";
import { MainPane } from "./components/MainPane";
import { NewSessionDialog } from "./components/NewSessionDialog";
import { Sidebar } from "./components/Sidebar";
import { useUi } from "./store";
import type { Host, Session, SessionWithStatus, Workspace } from "./types";

export default function App() {
  const [sessions, setSessions] = useState<SessionWithStatus[]>([]);
  const [hosts, setHosts] = useState<Host[]>([]);
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [newTarget, setNewTarget] = useState<{ hostId?: string; workspaceId?: string } | null>(null);
  const [rename, setRename] = useState(false);
  const [renameValue, setRenameValue] = useState("");
  const [pendingAction, setPendingAction] = useState<{ kind: "delete" | "restart"; session: SessionWithStatus } | null>(null);
  const [actionError, setActionError] = useState("");
  const [actionBusy, setActionBusy] = useState(false);
  const [reconnectRequest, setReconnectRequest] = useState<{ sessionId: string; nonce: number } | null>(null);
  const [newCopilotSessionIds, setNewCopilotSessionIds] = useState<string[]>([]);
  const [notice, setNotice] = useState("");
  const [refreshing, setRefreshing] = useState(false);
  const refreshGeneration = useRef(0);
  const { activeSessionId, select, sidebarOpen, toggleSidebar } = useUi();
  const active = sessions.find((session) => session.id === activeSessionId);

  const reportError = useCallback((reason: string) => setNotice(reason), []);
  const refresh = useCallback(async () => {
    const generation = ++refreshGeneration.current;
    setRefreshing(true);
    try {
      const latest = await api.listSessions();
      if (generation === refreshGeneration.current) setSessions(latest);
    } catch (reason) {
      if (generation === refreshGeneration.current) {
        setNotice(`Could not load session statuses: ${errorMessage(reason)}`);
      }
    } finally {
      if (generation === refreshGeneration.current) setRefreshing(false);
    }
  }, []);

  const loadHosts = useCallback(async () => {
    try {
      setHosts(await api.listHosts());
      setWorkspaces(await api.listAllWorkspaces());
    } catch (reason) {
      setNotice(`Could not load hosts: ${errorMessage(reason)}`);
    }
  }, []);

  const onSessionOpened = useCallback(async (id: string) => {
    try {
      const updated = await api.touchSession(id);
      setSessions((current) => current
        .map((item) => item.id === id ? { ...item, ...updated } : item)
        .sort((a, b) => Number(b.pinned) - Number(a.pinned)
          || (b.lastOpenedAt ?? b.createdAt).localeCompare(a.lastOpenedAt ?? a.createdAt)));
    } catch (reason) {
      setNotice(`Could not update last opened time: ${errorMessage(reason)}`);
    }
  }, []);

  useEffect(() => {
    void loadHosts();
    void refresh();
  }, [loadHosts, refresh]);
  useEffect(() => {
    const onFocus = () => { void loadHosts(); void refresh(); };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [loadHosts, refresh]);

  async function created(session: Session) {
    await refresh();
    await loadHosts();
    setNewCopilotSessionIds((previous) => [...previous, session.id]);
    select(session.id);
    setNewTarget(null);
  }

  async function changeName(event: FormEvent) {
    event.preventDefault();
    if (!active) return;
    try {
      await api.renameSession(active.id, renameValue);
      setRename(false);
      await refresh();
    } catch (reason) {
      reportError(errorMessage(reason));
    }
  }

  async function togglePin() {
    if (!active) return;
    try {
      await api.pinSession(active.id, !active.pinned);
      await refresh();
    } catch (reason) {
      reportError(errorMessage(reason));
    }
  }

  function confirmAction(kind: "delete" | "restart") {
    if (!active) return;
    setActionError("");
    setPendingAction({ kind, session: active });
  }

  async function applyAction(forget = false) {
    if (!pendingAction || actionBusy) return;
    const { kind, session } = pendingAction;
    setActionBusy(true);
    setActionError("");
    try {
      if (kind === "restart") {
        await api.restartSession(session.id, 100, 30);
      } else if (forget) {
        await api.forgetSession(session.id);
      } else {
        await api.deleteSession(session.id);
      }
      if (kind === "delete") {
        setNewCopilotSessionIds((previous) => previous.filter((id) => id !== session.id));
        if (activeSessionId === session.id) select(null);
      }
      await refresh();
      setPendingAction(null);
      if (kind === "restart") {
        setReconnectRequest({ sessionId: session.id, nonce: Date.now() });
      }
    } catch (reason) {
      setActionError(errorMessage(reason));
    } finally {
      setActionBusy(false);
    }
  }

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-canvas text-slate-100">
      {sidebarOpen && <Sidebar sessions={sessions} hosts={hosts} workspaces={workspaces}
        onNew={(target) => setNewTarget(target ?? {})} onRefresh={() => { void loadHosts(); void refresh(); }} refreshing={refreshing} />}
      <div className="relative flex min-h-0 min-w-0 flex-1 flex-col">
        <button type="button" className="absolute left-1 top-6 z-10 rounded-r bg-raised px-1 text-xs text-slate-300" onClick={toggleSidebar} title="Toggle sidebar" aria-label="Toggle sidebar">
          {sidebarOpen ? "‹" : "›"}
        </button>
        {!sidebarOpen && (
          <button type="button" className="absolute right-4 top-5 z-10 primary-button" onClick={() => setNewTarget({})}>+ New Session</button>
        )}
        {notice && (
          <div className="flex items-center justify-between gap-4 border-b border-rose-500/30 bg-rose-950 px-5 py-2 text-sm text-rose-100" role="alert">
            <span>{notice}</span>
            <button type="button" onClick={() => setNotice("")} aria-label="Dismiss error">×</button>
          </div>
        )}
        <MainPane
          session={active}
          sessions={sessions}
          reconnectRequest={reconnectRequest}
          autoCreateChatIds={newCopilotSessionIds}
          onSessionOpened={onSessionOpened}
          onRefresh={refresh}
          onRename={() => { setRenameValue(active?.name ?? ""); setRename(true); }}
          onPin={() => void togglePin()}
          onRestart={() => confirmAction("restart")}
          onDelete={() => confirmAction("delete")}
          onError={reportError}
        />
      </div>
      {newTarget && (
        <NewSessionDialog hosts={hosts} initialHostId={newTarget.hostId}
          initialWorkspaceId={newTarget.workspaceId}
          onHostAdded={loadHosts} onWorkspaceAdded={loadHosts} onCreated={created}
          onClose={() => setNewTarget(null)} />
      )}
      {rename && active && (
        <div className="dialog-backdrop">
          <form className="dialog max-w-sm" onSubmit={(event) => void changeName(event)} role="dialog" aria-modal="true" aria-label="Rename session">
            <h2 className="text-lg font-semibold">Rename session</h2>
            <input autoFocus className="field mt-4 w-full" aria-label="New session name" value={renameValue} onChange={(event) => setRenameValue(event.target.value)} required />
            <div className="mt-5 flex justify-end gap-2">
              <button type="button" className="secondary-button" onClick={() => setRename(false)}>Cancel</button>
              <button type="submit" className="primary-button">Save</button>
            </div>
          </form>
        </div>
      )}
      {pendingAction && (
        <div className="dialog-backdrop">
          <section className="dialog max-w-md" role="dialog" aria-modal="true"
            aria-labelledby="session-action-title">
            <h2 id="session-action-title" className="text-lg font-semibold">
              {pendingAction.kind === "delete" ? "Delete session" : "Restart session"}
            </h2>
            <p className="mt-3 text-sm text-slate-300">
              {pendingAction.kind === "delete"
                ? `Stop the tmux process for "${pendingAction.session.name}" and remove its saved session?`
                : pendingAction.session.status === "dead"
                  ? `Create a terminal for "${pendingAction.session.name}" using ${pendingAction.session.command}?`
                : `Stop "${pendingAction.session.name}" and launch ${pendingAction.session.command} again?`}
            </p>
            {pendingAction.kind === "delete" && (
              <p className="mt-2 text-xs text-slate-400">
                If the host is offline, Delete cannot stop tmux. Forget only removes this app’s record;
                the remote process may keep running.
              </p>
            )}
            {pendingAction.kind === "delete" && pendingAction.session.copilotSessionId && (
              <p className="mt-2 text-xs text-slate-400">
                Copilot conversation data stays on the remote host, but removing this app session
                also removes its saved link to that conversation.
              </p>
            )}
            {actionError && <p role="alert" className="error-box mt-4">{actionError}</p>}
            <div className="mt-6 flex flex-wrap justify-end gap-2">
              <button type="button" className="secondary-button" disabled={actionBusy}
                onClick={() => setPendingAction(null)}>Cancel</button>
              {pendingAction.kind === "delete" && (
                <button type="button" className="secondary-button" disabled={actionBusy}
                  onClick={() => void applyAction(true)}>Forget only</button>
              )}
              <button type="button" className="secondary-button border-rose-500/40 text-rose-200"
                disabled={actionBusy} onClick={() => void applyAction()}>
                {actionBusy ? "Working..." : pendingAction.kind === "delete" ? "Delete and stop tmux"
                  : pendingAction.session.status === "dead" ? "Create terminal" : "Restart"}
              </button>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
