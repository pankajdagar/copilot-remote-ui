import type { SessionWithStatus } from "../types";
import { useUi } from "../store";

const statusLabel: Record<SessionWithStatus["status"], string> = {
  running: "Running",
  disconnected: "Disconnected",
  dead: "Missing",
  hostUnavailable: "Host unavailable"
};

export function SessionItem({
  session,
  active,
  onSelect
}: {
  session: SessionWithStatus;
  active: boolean;
  onSelect: () => void;
}) {
  const connection = useUi((state) => state.connections[session.id]);
  const agent = useUi((state) => state.agentStatuses[session.id]);
  const status = connection === "connected" ? "running"
    : (connection === "disconnected" || connection === "error") && session.status === "running"
      ? "disconnected" : session.status;
  const indicator = agent === "working" ? "status-running"
    : agent === "awaitingPermission" ? "status-hostUnavailable"
    : agent === "idle" ? "status-idle"
    : agent === "error" || agent === "unavailable" || agent === "hostOffline" || agent === "sessionMissing"
      ? "status-error" : `status-${status}`;
  const label = agent === "working" ? "Copilot working"
    : agent === "awaitingPermission" ? "Copilot approval required"
    : agent === "idle" ? "Copilot idle"
    : agent === "sessionMissing" ? "Copilot session missing"
    : agent === "hostOffline" ? "Host unavailable"
    : agent === "error" || agent === "unavailable" ? "Copilot unavailable"
    : statusLabel[status];
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={active ? "page" : undefined}
      className={`session-item ${active ? "session-item-active" : ""}`}
    >
      <span
        className={`status-dot ${indicator}`}
        title={label}
        aria-label={label}
      />
      <span className="min-w-0 flex-1 text-left">
        <span className="block truncate text-sm font-medium text-slate-100">{session.name}</span>
        <span className="mt-1 block truncate text-xs text-slate-400">
          {session.workspace.displayName} · {session.host.name}
        </span>
        <span className="mt-1 block text-[11px] text-slate-500">
          {session.lastOpenedAt
            ? `Opened ${new Date(session.lastOpenedAt).toLocaleString()}`
            : "Never opened"}
        </span>
        {agent && <span className="mt-1 block text-[11px] text-slate-400">{label}</span>}
      </span>
    </button>
  );
}
