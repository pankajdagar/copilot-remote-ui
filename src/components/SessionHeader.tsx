import type { SessionWithStatus } from "../types";
import * as Popover from "@radix-ui/react-popover";
import { Ellipsis, Pencil, Pin, RotateCcw, Trash2 } from "lucide-react";

export function SessionHeader({
  session,
  copilotStatus,
  terminalStatus,
  allowAll,
  onRename,
  onPin,
  onRestart,
  onDelete
}: {
  session: SessionWithStatus;
  copilotStatus: string;
  terminalStatus: string;
  allowAll: boolean;
  onRename: () => void;
  onPin: () => void;
  onRestart: () => void;
  onDelete: () => void;
}) {
  return (
    <header className="flex min-h-[82px] items-center justify-between gap-4 border-b border-white/10 bg-panel/95 px-6">
      <div className="min-w-0">
        <h2 className="truncate text-lg font-semibold text-white">{session.name}</h2>
        <p className="truncate text-xs text-slate-400">
          {session.workspace.displayName} · {session.host.name} · {session.workspace.repoPath}
        </p>
        <div className="mt-2 flex flex-wrap gap-2 text-[11px]">
          <span className="rounded-full border border-white/10 bg-raised px-2 py-0.5 text-slate-200">
            Copilot · {copilotStatus}
          </span>
          <span className="rounded-full border border-white/10 bg-raised px-2 py-0.5 text-slate-400">
            Terminal · {terminalStatus}
          </span>
          {allowAll && <span className="rounded-full border border-amber-500/30 bg-amber-500/10 px-2 py-0.5 text-amber-300" role="status">
            Allow all enabled
          </span>}
        </div>
      </div>
      <Popover.Root>
        <Popover.Trigger asChild>
          <button type="button" className="secondary-button flex shrink-0 items-center gap-2" aria-label="Session actions">
            <Ellipsis size={17} aria-hidden />Actions
          </button>
        </Popover.Trigger>
        <Popover.Portal>
          <Popover.Content align="end" sideOffset={8}
            className="z-30 min-w-44 rounded-xl border border-white/15 bg-panel p-1.5 shadow-2xl outline-none"
            aria-label="Session actions">
            <Popover.Close asChild><button type="button" className="session-action" onClick={onRename}>
              <Pencil size={14} aria-hidden />Rename
            </button></Popover.Close>
            <Popover.Close asChild><button type="button" className="session-action" onClick={onPin}>
              <Pin size={14} aria-hidden />{session.pinned ? "Unpin" : "Pin"}
            </button></Popover.Close>
            <Popover.Close asChild><button type="button" className="session-action" onClick={onRestart}>
              <RotateCcw size={14} aria-hidden />{session.status === "dead" ? "Create terminal" : "Restart terminal"}
            </button></Popover.Close>
            <div className="my-1 border-t border-white/10" />
            <Popover.Close asChild><button type="button" className="session-action text-rose-300" onClick={onDelete}>
              <Trash2 size={14} aria-hidden />Delete
            </button></Popover.Close>
          </Popover.Content>
        </Popover.Portal>
      </Popover.Root>
    </header>
  );
}
