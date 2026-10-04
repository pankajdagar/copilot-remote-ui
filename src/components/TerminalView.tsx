import { Channel } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebLinksAddon } from "@xterm/addon-web-links";
import { Terminal } from "@xterm/xterm";
import { forwardRef, useEffect, useImperativeHandle, useRef } from "react";
import "@xterm/xterm/css/xterm.css";

import { api, errorMessage } from "../api";
import type { TerminalDiagnostics, TerminalEvent } from "../types";

export interface TerminalControls {
  activate: () => void;
  syncSize: () => Promise<{ cols: number; rows: number; diagnostics: TerminalDiagnostics }>;
  scrollback: () => Promise<void>;
  findNext: (query: string) => void;
  findPrevious: (query: string) => void;
  copy: () => Promise<void>;
  paste: () => Promise<void>;
}

export const TerminalView = forwardRef<TerminalControls, {
  sessionId: string;
  active: boolean;
  reconnectKey: number;
  onConnected: () => void;
  onDisconnected: (message: string) => void;
  onMouseModeChange: (mode: string) => void;
  onMouseReports: (count: number) => void;
  onError: (message: string) => void;
}>(function TerminalView(
  { sessionId, active, reconnectKey, onConnected, onDisconnected, onMouseModeChange, onMouseReports, onError },
  ref
) {
  const container = useRef<HTMLDivElement>(null);
  const controls = useRef<TerminalControls | null>(null);

  useImperativeHandle(ref, () => ({
    activate: () => controls.current?.activate(),
    syncSize: () => controls.current?.syncSize() ??
      Promise.reject(new Error("Terminal is not connected")),
    scrollback: () => controls.current?.scrollback() ??
      Promise.reject(new Error("Terminal is not connected")),
    findNext: (query) => controls.current?.findNext(query),
    findPrevious: (query) => controls.current?.findPrevious(query),
    copy: async () => { await controls.current?.copy(); },
    paste: async () => { await controls.current?.paste(); }
  }), []);

  useEffect(() => {
    const element = container.current;
    if (!element) return;

    const terminal = new Terminal({
      cursorBlink: true,
      convertEol: false,
      scrollback: 10000,
      fontSize: 13,
      fontFamily: "SFMono-Regular, Menlo, Monaco, Consolas, monospace",
      theme: {
        background: "#10151d",
        foreground: "#e3eaf2",
        cursor: "#7cd4b0",
        selectionBackground: "#3c6b68"
      }
    });
    const fit = new FitAddon();
    const search = new SearchAddon();
    terminal.loadAddon(fit);
    terminal.loadAddon(search);
    terminal.loadAddon(new WebLinksAddon((_event, uri) => {
      void api.openUrl(uri).catch((reason: unknown) => onError(errorMessage(reason)));
    }));
    terminal.open(element);
    onMouseReports(0);
    let closed = false;
    let transportClosed = false;
    let connectionId: string | null = null;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let resizeChain = Promise.resolve();
    let writeChain = Promise.resolve();
    let lastMouseMode = terminal.modes.mouseTrackingMode;
    let mouseReports = 0;
    let mouseTimer: ReturnType<typeof setTimeout> | undefined;
    const encoder = new TextEncoder();

    function syncSize() {
      if (closed || !element || element.clientWidth === 0 || element.clientHeight === 0) return;
      fit.fit();
      const cols = terminal.cols;
      const rows = terminal.rows;
      const id = connectionId;
      if (!id || cols < 2 || rows < 2) return;
      resizeChain = resizeChain
        .then(() => api.resizeTerminal(sessionId, id, cols, rows))
        .catch((reason: unknown) => {
          if (!closed) onError(`Could not resize terminal: ${errorMessage(reason)}`);
        });
    }

    async function forceSyncSize(): Promise<{ cols: number; rows: number; diagnostics: TerminalDiagnostics }> {
      if (closed || !connectionId) throw new Error("Connect to the session before syncing its size");
      if (!element || element.clientWidth === 0 || element.clientHeight === 0) {
        throw new Error("Terminal pane is not visible");
      }
      fit.fit();
      const cols = terminal.cols;
      const rows = terminal.rows;
      if (cols < 2 || rows < 2) throw new Error("Terminal pane is too small to synchronize");
      const id = connectionId;
      await resizeChain;
      const diagnostics = await api.syncTerminalSize(sessionId, id, cols, rows);
      if (!closed) terminal.refresh(0, rows - 1);
      return { cols, rows, diagnostics };
    }

    function queueInput(bytes: Uint8Array, mouse: boolean) {
      const id = connectionId;
      if (!id || closed) return;
      for (let offset = 0; offset < bytes.length; offset += 64 * 1024) {
        const chunk = bytes.slice(offset, offset + 64 * 1024);
        writeChain = writeChain
          .then(async () => {
            await api.writeTerminal(sessionId, id, chunk);
            if (mouse) {
              mouseReports++;
              if (!mouseTimer) {
                mouseTimer = setTimeout(() => {
                  mouseTimer = undefined;
                  if (!closed) onMouseReports(mouseReports);
                }, 100);
              }
            }
          })
          .catch((reason: unknown) => {
            if (!closed) onError(`Could not send terminal input: ${errorMessage(reason)}`);
          });
      }
    }

    const dataSubscription = terminal.onData((data) =>
      queueInput(encoder.encode(data), data.startsWith("\x1b[<"))
    );
    const binarySubscription = terminal.onBinary((data) => {
      queueInput(Uint8Array.from(data, (character) => character.charCodeAt(0)), data.startsWith("\x1b[M"));
    });

    controls.current = {
      activate: () => {
        syncSize();
        terminal.focus();
      },
      syncSize: forceSyncSize,
      scrollback: async () => {
        if (closed || !connectionId) throw new Error("Connect to the session before opening scrollback");
        await api.enterScrollback(sessionId, connectionId);
        if (!closed) terminal.focus();
      },
      findNext: (query) => { if (query) search.findNext(query); },
      findPrevious: (query) => { if (query) search.findPrevious(query); },
      copy: async () => {
        if (terminal.hasSelection()) await navigator.clipboard.writeText(terminal.getSelection());
      },
      paste: async () => { terminal.paste(await navigator.clipboard.readText()); }
    };

    const observer = new ResizeObserver(() => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(syncSize, 75);
    });
    observer.observe(element);
    window.visualViewport?.addEventListener("resize", syncSize);
    window.addEventListener("resize", syncSize);

    const channel = new Channel<TerminalEvent>();
    channel.onmessage = (event) => {
      if (closed) return;
      if (event.kind === "output") {
        terminal.write(new Uint8Array(event.bytes), () => {
          if (closed) return;
          const mode = terminal.modes.mouseTrackingMode;
          if (mode !== lastMouseMode) {
            lastMouseMode = mode;
            onMouseModeChange(mode);
          }
        });
      } else {
        transportClosed = true;
        connectionId = null;
        onDisconnected(event.message);
      }
    };
    requestAnimationFrame(() => {
      if (closed) return;
      fit.fit();
      void api.attachSession(
        sessionId,
        Math.max(2, terminal.cols),
        Math.max(2, terminal.rows),
        channel
      )
        .then((id) => {
          if (closed || transportClosed) {
            void api.disconnectTerminal(sessionId, id).catch((reason: unknown) =>
              onError(errorMessage(reason))
            );
            return;
          }
          connectionId = id;
          syncSize();
          terminal.focus();
          onConnected();
        })
        .catch((reason: unknown) => {
          if (!closed) onError(errorMessage(reason));
        });
    });

    return () => {
      closed = true;
      observer.disconnect();
      if (timer) clearTimeout(timer);
      if (mouseTimer) clearTimeout(mouseTimer);
      window.visualViewport?.removeEventListener("resize", syncSize);
      window.removeEventListener("resize", syncSize);
      dataSubscription.dispose();
      binarySubscription.dispose();
      controls.current = null;
      terminal.dispose();
      if (connectionId) {
        void api.disconnectTerminal(sessionId, connectionId).catch((reason: unknown) =>
          onError(`Could not detach terminal: ${errorMessage(reason)}`)
        );
      }
    };
  }, [sessionId, reconnectKey, onConnected, onDisconnected, onMouseModeChange, onMouseReports, onError]);

  useEffect(() => {
    if (!active) return;
    const frame = requestAnimationFrame(() => controls.current?.activate());
    return () => cancelAnimationFrame(frame);
  }, [active]);

  return <div ref={container} className="terminal-container h-full w-full" aria-label="Interactive terminal" />;
});
