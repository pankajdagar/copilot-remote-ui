import { useCallback, useEffect, useState } from "react";
import { Cable, RotateCcw } from "lucide-react";

import { api, errorMessage } from "../api";
import type { PortInventory } from "../types";

export function PortsPanel({ sessionId, host }: { sessionId: string; host: string }) {
  const [inventory, setInventory] = useState<PortInventory | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const refresh = useCallback(async () => {
    try {
      setInventory(await api.listPortForwards(sessionId));
      setMessage(null);
    } catch (reason) {
      setInventory(null);
      setMessage(`Could not inspect SSH forwards: ${errorMessage(reason)}`);
    }
  }, [sessionId]);

  useEffect(() => { void refresh(); }, [refresh]);

  async function changeTunnel(paused: boolean) {
    setBusy(true);
    try {
      if (paused) await api.pauseChatTunnel(sessionId);
      else await api.resumeChatTunnel(sessionId);
      await refresh();
    } catch (reason) {
      setMessage(`Could not ${paused ? "release" : "resume"} Chat tunnel: ${errorMessage(reason)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="min-h-0 flex-1 overflow-y-auto p-6" aria-label="SSH port forwards">
      <div className="mx-auto max-w-3xl space-y-5">
        <header className="flex items-center justify-between gap-3">
          <div>
            <h2 className="flex items-center gap-2 text-lg font-semibold"><Cable size={19} aria-hidden />Ports · {host}</h2>
            <p className="mt-1 text-xs text-slate-400">Copilot Remote UI manages only its own Chat tunnel, not VS Code or other SSH clients.</p>
          </div>
          <button type="button" className="secondary-button flex items-center gap-2" onClick={() => void refresh()}>
            <RotateCcw size={14} aria-hidden />Refresh
          </button>
        </header>
        {message && <p className="error-box" role="alert">{message}</p>}
        <article className="rounded-xl border border-white/10 bg-panel p-5">
          <h3 className="font-medium">App-owned Chat tunnel</h3>
          {inventory?.chat.localPort
            ? <p className="mt-2 break-all font-mono text-xs text-accent">
                127.0.0.1:{inventory.chat.localPort} → {host}:127.0.0.1:{inventory.chat.remotePort}
              </p>
            : <p className="mt-2 text-sm text-slate-400">
                {inventory?.chat.paused ? "Paused — no Chat tunnel is listening." : "No Chat tunnel currently attached."}
              </p>}
          <div className="mt-4">
            {inventory?.chat.paused
              ? <button type="button" className="secondary-button" disabled={busy}
                  onClick={() => void changeTunnel(false)}>Resume Chat tunnel</button>
              : <button type="button" className="secondary-button" disabled={busy || !inventory?.chat.localPort}
                  onClick={() => void changeTunnel(true)}>Release Chat tunnel</button>}
          </div>
          <p className="mt-2 text-xs text-slate-400">
            Releasing disconnects Chat on this host until resumed, without stopping the remote tmux or Copilot process.
            Stop active work first. It cannot release ports held by other processes.
          </p>
        </article>
        <article className="rounded-xl border border-white/10 bg-panel p-5">
          <h3 className="font-medium">Configured OpenSSH forwards</h3>
          <p className="mt-1 text-xs text-slate-400">
            These come from the effective SSH alias configuration. They are not necessarily active, and may be owned
            by VS Code or another terminal; manage those forwards in the process that opened them.
          </p>
          <div className="mt-4 space-y-2">
            {inventory?.configured.map((forward, index) => (
              <div key={`${forward.direction}:${forward.spec}:${index}`}
                className="flex flex-wrap items-baseline gap-2 rounded-lg bg-canvas px-3 py-2 text-xs">
                <span className="text-slate-400">{forward.direction}</span>
                <code className="break-all text-slate-200">{forward.spec}</code>
              </div>
            ))}
            {inventory && inventory.configured.length === 0 &&
              <p className="text-sm text-slate-400">No configured forwards for this alias.</p>}
            {!inventory && !message && <p className="text-sm text-slate-400">Loading SSH configuration...</p>}
          </div>
        </article>
      </div>
    </section>
  );
}
