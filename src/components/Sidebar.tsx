import type { Host, SessionWithStatus, Workspace } from "../types";
import { Folder, Plus, RefreshCw, Server } from "lucide-react";
import { useUi } from "../store";
import { SessionGroup } from "./SessionGroup";
import { SessionSearch } from "./SessionSearch";

export function Sidebar({
  sessions,
  hosts,
  workspaces,
  onNew,
  onRefresh,
  refreshing
}: {
  sessions: SessionWithStatus[];
  hosts: Host[];
  workspaces: Workspace[];
  onNew: (target?: { hostId?: string; workspaceId?: string }) => void;
  onRefresh: () => void;
  refreshing: boolean;
}) {
  const { activeSessionId, search, select } = useUi();
  const matches = sessions.filter((session) =>
    `${session.name} ${session.workspace.displayName} ${session.host.name}`
      .toLocaleLowerCase()
      .includes(search.toLocaleLowerCase())
  );
  const remoteHosts = hosts.filter((host) => host.sshHost &&
    `${host.name} ${host.sshHost} ${workspaces.filter((workspace) => workspace.hostId === host.id)
      .map((workspace) => `${workspace.displayName} ${workspace.repoPath}`).join(" ")}`
      .toLocaleLowerCase().includes(search.toLocaleLowerCase())
  );

  return (
    <aside className="flex w-[290px] min-w-[230px] shrink-0 flex-col border-r border-white/10 bg-panel">
      <header className="border-b border-white/10 bg-gradient-to-b from-accent/[0.06] to-transparent px-5 pb-5 pt-6">
        <div className="flex items-center justify-between">
          <div>
            <p className="text-xs uppercase tracking-[0.2em] text-accent">Copilot Remote UI</p>
            <h1 className="mt-2 text-xl font-semibold text-white">Sessions</h1>
          </div>
          <button
            type="button"
            onClick={onRefresh}
            className="icon-button"
            title="Refresh statuses"
            aria-label="Refresh statuses"
            disabled={refreshing}
          >
            <RefreshCw size={17} aria-hidden />
          </button>
        </div>
        <button type="button" className="primary-button mt-5 flex w-full items-center justify-center gap-2" onClick={() => onNew()}>
          <Plus size={17} aria-hidden />New Session
        </button>
        <div className="mt-4"><SessionSearch /></div>
      </header>
      <nav className="min-h-0 flex-1 overflow-y-auto px-3 pb-6 pt-1" aria-label="Sessions">
        <SessionGroup
          title="Pinned"
          sessions={matches.filter((session) => session.pinned)}
          activeSessionId={activeSessionId}
          onSelect={select}
        />
        <SessionGroup
          title="Recent"
          sessions={matches.filter((session) => !session.pinned)}
          activeSessionId={activeSessionId}
          onSelect={select}
        />
        {remoteHosts.length > 0 && (
          <section className="mt-6" aria-label="SSH hosts and repositories">
            <h2 className="px-3 text-xs font-semibold uppercase tracking-[0.15em] text-slate-500">
              SSH hosts
            </h2>
            <div className="mt-2 space-y-3">
              {remoteHosts.map((host) => {
                const repositories = workspaces.filter((workspace) => workspace.hostId === host.id);
                return (
                  <div key={host.id} className="rounded-xl border border-white/5 bg-raised/40 px-3 py-2.5">
                    <button type="button" className="flex w-full items-center gap-2 truncate text-left text-sm font-medium text-accent"
                      onClick={() => onNew({ hostId: host.id })}>
                      <Server size={14} className="shrink-0" aria-hidden /><span className="truncate">{host.name}</span>
                    </button>
                    {repositories.length === 0 && <p className="mt-1 pl-4 text-xs text-slate-500">Add a repository</p>}
                    {repositories.map((workspace) => (
                      <button type="button" key={workspace.id}
                        className="mt-1 flex w-full items-center gap-2 truncate pl-4 text-left text-xs text-slate-300 hover:text-white"
                        onClick={() => onNew({ hostId: host.id, workspaceId: workspace.id })}
                        title={workspace.repoPath}>
                        <Folder size={12} className="shrink-0" aria-hidden /><span className="truncate">{workspace.displayName}</span>
                      </button>
                    ))}
                  </div>
                );
              })}
            </div>
          </section>
        )}
        {matches.length === 0 && remoteHosts.length === 0 && (
          <p className="px-3 pt-8 text-sm text-slate-400">
            {search ? "No matching sessions or hosts." : "No sessions yet. Create one to get started."}
          </p>
        )}
      </nav>
    </aside>
  );
}
