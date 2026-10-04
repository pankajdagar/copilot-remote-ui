import { useVirtualizer } from "@tanstack/react-virtual";
import { X } from "lucide-react";
import { useLayoutEffect, useMemo, useRef } from "react";

import { groupConversation } from "../agent/conversation";
import type { SubagentTrace } from "../agent/subagents";
import { ConversationRow } from "./ConversationRow";

export function SubagentInspector({
  trace, traces, onSelect, onClose, onDecide, onError, responding
}: {
  trace: SubagentTrace;
  traces: SubagentTrace[];
  onSelect: (id: string) => void;
  onClose: () => void;
  onDecide: (id: string, allow: boolean) => Promise<void>;
  onError: (message: string) => void;
  responding: string | null;
}) {
  const viewport = useRef<HTMLDivElement>(null);
  const rows = useMemo(() => groupConversation(trace.items), [trace.items]);
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => viewport.current,
    getItemKey: (index) => `${trace.id}:${rows[index].kind}:${rows[index].id}`,
    estimateSize: (index) => rows[index]?.kind === "message" ? 160 : 100,
    gap: 12,
    overscan: 4,
    initialRect: { width: 380, height: 540 },
    useFlushSync: false
  });
  useLayoutEffect(() => {
    virtualizer.scrollToOffset(0);
  }, [trace.id]);

  return <aside className="flex min-h-0 w-[clamp(17rem,42%,34rem)] shrink-0 flex-col border-l border-white/10 bg-panel"
    aria-label="Subagent trace">
    <div className="border-b border-white/10 p-3">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-sm font-semibold text-white">Subagent activity</h2>
        <button className="toolbar-button" type="button" aria-label="Close subagent trace"
          onClick={onClose}><X size={15} aria-hidden /></button>
      </div>
      <label className="mt-2 block text-xs text-slate-400" htmlFor="subagent-inspector-select">
        Inspect a subagent
      </label>
      <select id="subagent-inspector-select" className="field mt-1 w-full text-xs"
        value={trace.id} onChange={(event) => onSelect(event.target.value)}>
        {traces.map((option) => <option key={option.id} value={option.id}>
          {option.displayName} · {option.status}
        </option>)}
      </select>
      {trace.model && <p className="mt-2 text-xs text-slate-400">Model: {trace.model}</p>}
      {trace.description && <p className="mt-1 text-xs text-slate-400">{trace.description}</p>}
      {trace.error && <p className="mt-1 text-xs text-rose-200">{trace.error}</p>}
    </div>
    <div ref={viewport} className="subagent-viewport min-h-0 flex-1 overflow-y-auto p-3" tabIndex={0}>
      {rows.length === 0 && <p className="py-8 text-center text-xs text-slate-400">
        No trace events reported yet.
      </p>}
      <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((row) => (
          <div key={row.key} data-index={row.index} ref={virtualizer.measureElement}
            className="absolute left-0 top-0 w-full"
            style={{ transform: `translateY(${row.start}px)` }}>
            <ConversationRow item={rows[row.index]} responding={responding}
              onDecide={onDecide} onError={onError} />
          </div>
        ))}
      </div>
    </div>
  </aside>;
}
