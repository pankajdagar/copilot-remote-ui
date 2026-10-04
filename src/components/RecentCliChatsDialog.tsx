import { useCallback, useEffect, useMemo, useState } from "react";
import { RotateCcw } from "lucide-react";

import { api, errorMessage } from "../api";
import type { RecentCliChat } from "../types";

export function RecentCliChatsDialog({
  sessionId,
  workspacePath,
  onChoose,
  onClose
}: {
  sessionId: string;
  workspacePath: string;
  onChoose: (selectedId: string, allowDifferentRepo: boolean) => Promise<void>;
  onClose: () => void;
}) {
  const [sessions, setSessions] = useState<RecentCliChat[]>([]);
  const [filter, setFilter] = useState("");
  const [displayLimit, setDisplayLimit] = useState(50);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [allowDifferentRepo, setAllowDifferentRepo] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setSessions(await api.listRecentCliChats(sessionId));
    } catch (reason) {
      setError(`Could not list recent CLI chats on this remote host: ${errorMessage(reason)}`);
    } finally {
      setLoading(false);
    }
  }, [sessionId]);

  useEffect(() => { void load(); }, [load]);

  const visible = useMemo(() => sessions.filter((session) =>
    `${session.summary ?? ""} ${session.id}`.toLocaleLowerCase().includes(filter.toLocaleLowerCase())
  ), [sessions, filter]);
  const selected = sessions.find((session) => session.id === selectedId);
  const canAttach = !!selected && !selected.isRemote && !selected.linkedTo &&
    (selected.sameWorkspace || allowDifferentRepo) && !busy;

  async function choose() {
    if (!selected || !canAttach) return;
    setBusy(true);
    setError(null);
    try {
      await onChoose(selected.id, !selected.sameWorkspace);
      onClose();
    } catch (reason) {
      setError(`Could not attach selected CLI chat: ${errorMessage(reason)}. The previous app chat link was kept.`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dialog-backdrop" onKeyDown={(event) => {
      if (event.key === "Escape" && !busy) onClose();
    }}>
      <section className="dialog max-w-2xl" role="dialog" aria-modal="true" aria-label="Choose recent Copilot CLI chat">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2 className="text-lg font-semibold">Recent Copilot CLI chats</h2>
            <p className="mt-1 text-xs text-slate-400">Persisted conversations on this remote host. Choose one to link to this app session.</p>
          </div>
          <button className="toolbar-button flex items-center gap-1" type="button" disabled={loading || busy}
            onClick={() => void load()}><RotateCcw size={13} aria-hidden />Refresh</button>
        </div>
        <p className="mt-3 break-all rounded-lg bg-canvas px-3 py-2 text-xs text-slate-400">
          Selected repository: <span className="text-slate-200">{workspacePath || "Current workspace"}</span>
        </p>
        <label className="mt-4 block text-xs text-slate-300" htmlFor={`cli-chat-search-${sessionId}`}>
          Search summaries or IDs
        </label>
        <input id={`cli-chat-search-${sessionId}`} autoFocus className="field mt-1 w-full text-sm"
          value={filter} onChange={(event) => { setFilter(event.target.value); setDisplayLimit(50); }}
          placeholder="Search recent chats..." />
        {error && <p className="error-box mt-3" role="alert">{error}</p>}
        <div className="mt-3 max-h-72 space-y-2 overflow-y-auto pr-1" role="group" aria-label="Recent CLI chats">
          {visible.slice(0, displayLimit).map((session) => {
            const unavailable = session.isRemote || !!session.linkedTo;
            const modified = new Date(session.modifiedAt);
            return (
              <label key={session.id} className={`flex gap-3 rounded-lg border px-3 py-3 text-sm ${
                selectedId === session.id ? "border-accent/60 bg-accent/5" : "border-white/10 bg-canvas"
              } ${unavailable ? "opacity-60" : "cursor-pointer hover:border-white/25"}`}>
                <input type="radio" name={`cli-chat-${sessionId}`} value={session.id}
                  checked={selectedId === session.id} disabled={unavailable || busy}
                  onChange={() => { setSelectedId(session.id); setAllowDifferentRepo(false); }} />
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-medium text-slate-100">
                    {session.summary || "Untitled conversation"}
                  </span>
                  <span className="mt-1 block break-all font-mono text-[11px] text-slate-400">{session.id}</span>
                  <span className="mt-1 block text-xs text-slate-400">
                    {Number.isNaN(modified.getTime()) ? session.modifiedAt : modified.toLocaleString()}
                    {" · "}{session.sameWorkspace ? "This repository" : "Other or unknown repository"}
                    {session.isRemote ? " · Cloud session (not attachable here)" : ""}
                    {session.linkedTo ? ` · Already linked to ${session.linkedTo}` : ""}
                  </span>
                </span>
              </label>
            );
          })}
          {!loading && sessions.length === 0 && !error &&
            <p className="py-6 text-center text-sm text-slate-400">
              No persisted Copilot CLI chats found on this remote host. Live-only or cloud chats are not available to attach here.
            </p>}
          {!loading && sessions.length > 0 && visible.length === 0 &&
            <p className="py-6 text-center text-sm text-slate-400">No matching CLI chats.</p>}
          {visible.length > displayLimit &&
            <button type="button" className="secondary-button w-full"
              onClick={() => setDisplayLimit((limit) => limit + 50)}>
              Show more ({displayLimit} of {visible.length})
            </button>}
          {loading && <p className="py-6 text-center text-sm text-slate-400">Loading recent CLI chats...</p>}
        </div>
        {selected && !selected.sameWorkspace && !selected.isRemote && !selected.linkedTo && (
          <label className="mt-4 flex gap-2 rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-xs text-amber-100">
            <input type="checkbox" checked={allowDifferentRepo}
              onChange={(event) => setAllowDifferentRepo(event.target.checked)} />
            <span>This chat was not saved in the selected repository. I understand that continuing it in
              {" "}{workspacePath || "this workspace"} may use a different working directory than before.</span>
          </label>
        )}
        <div className="mt-5 flex justify-end gap-2">
          <button className="secondary-button" type="button" disabled={busy} onClick={onClose}>Cancel</button>
          <button className="primary-button" type="button" disabled={!canAttach} onClick={() => void choose()}>
            {busy ? "Attaching..." : "Attach selected chat"}
          </button>
        </div>
      </section>
    </div>
  );
}
