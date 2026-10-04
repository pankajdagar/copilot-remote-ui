import { useState } from "react";
import { errorMessage } from "../api";
import type { TerminalControls } from "./TerminalView";

export function TerminalToolbar({
  status,
  mouseMode,
  mouseReports,
  terminal,
  onReconnect,
  onError
}: {
  status: string;
  mouseMode: string;
  mouseReports: number;
  terminal: () => TerminalControls | null;
  onReconnect: () => void;
  onError: (message: string) => void;
}) {
  const [query, setQuery] = useState("");
  const [syncing, setSyncing] = useState(false);
  const [syncResult, setSyncResult] = useState("");
  const [scrollbackInfo, setScrollbackInfo] = useState("");

  async function syncSize() {
    setSyncing(true);
    setSyncResult("");
    try {
      const controls = terminal();
      if (!controls) throw new Error("Terminal is not connected");
      const { cols, rows, diagnostics } = await controls.syncSize();
      if (diagnostics.paneCount === 1 && diagnostics.paneRows !== rows) {
        throw new Error(`PTY is ${cols}×${rows}, but the tmux pane is ${diagnostics.paneRows} rows`);
      }
      setSyncResult(`PTY ${cols}×${rows} · tmux ${diagnostics.paneRows} rows · Copilot mouse ${
        diagnostics.applicationMouse ? "requested" : "not requested"
      }`);
    } catch (reason) {
      onError(`Could not sync terminal size: ${errorMessage(reason)}`);
    } finally {
      setSyncing(false);
    }
  }

  async function enterScrollback() {
    try {
      const controls = terminal();
      if (!controls) throw new Error("Terminal is not connected");
      await controls.scrollback();
      setScrollbackInfo("tmux scrollback: wheel or PageUp/PageDown; Esc exits");
    } catch (reason) {
      onError(`Could not open scrollback: ${errorMessage(reason)}`);
    }
  }

  return (
    <div className="flex flex-wrap items-center gap-2 border-b border-white/10 bg-panel px-4 py-2">
      <span className="mr-1 text-xs text-slate-400">{status}</span>
      {status === "connected" && (
        <span className="text-xs text-slate-400" title="tmux requested mouse reporting from xterm; Sync Size checks Copilot's inner mouse mode">
          tmux mouse: {mouseMode === "none" ? "off" : "on"}
        </span>
      )}
      {status === "connected" && (
        <span className="text-xs text-slate-400" title="Mouse reports written to the local PTY">
          mouse reports: {mouseReports}
        </span>
      )}
      <button className="toolbar-button" type="button" onClick={() => void syncSize()}
        title="Refit xterm, resize the PTY, and reapply tmux sizing" disabled={syncing}>
        {syncing ? "Syncing..." : "Sync terminal size"}
      </button>
      {syncResult && <span className="text-xs text-accent" role="status">{syncResult}</span>}
      <button className="toolbar-button" type="button" onClick={() => void enterScrollback()}
        title="Scroll tmux output history; press Esc to return to Copilot">
        Scrollback
      </button>
      {scrollbackInfo && <span className="text-xs text-accent" role="status">{scrollbackInfo}</span>}
      <button className="toolbar-button" type="button" onClick={onReconnect}>Reconnect</button>
      <div className="flex flex-1 items-center justify-end gap-1">
        <input
          className="field w-32 text-xs"
          type="search"
          aria-label="Search terminal"
          placeholder="Find in terminal"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") terminal()?.findNext(query);
          }}
        />
        <button type="button" className="icon-button" onClick={() => terminal()?.findPrevious(query)} title="Previous match">↑</button>
        <button type="button" className="icon-button" onClick={() => terminal()?.findNext(query)} title="Next match">↓</button>
        <button
          type="button"
          className="toolbar-button"
          onClick={() => { void terminal()?.copy().catch((reason: unknown) => onError(String(reason))); }}
        >Copy</button>
        <button
          type="button"
          className="toolbar-button"
          onClick={() => { void terminal()?.paste().catch((reason: unknown) => onError(String(reason))); }}
        >Paste</button>
      </div>
    </div>
  );
}
