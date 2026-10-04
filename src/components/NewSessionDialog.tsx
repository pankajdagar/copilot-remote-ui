import { useEffect, useState, type FormEvent } from "react";
import { api, errorMessage } from "../api";
import type { Host, Session, Workspace } from "../types";

export function NewSessionDialog({
  hosts,
  initialHostId,
  initialWorkspaceId,
  onHostAdded,
  onWorkspaceAdded,
  onCreated,
  onClose
}: {
  hosts: Host[];
  initialHostId?: string;
  initialWorkspaceId?: string;
  onHostAdded: () => Promise<void>;
  onWorkspaceAdded: () => Promise<void>;
  onCreated: (session: Session) => Promise<void>;
  onClose: () => void;
}) {
  const [hostId, setHostId] = useState(
    initialHostId ?? hosts.find((host) => host.sshHost)?.id ?? hosts[0]?.id ?? ""
  );
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [workspaceId, setWorkspaceId] = useState("");
  const [addingHost, setAddingHost] = useState(false);
  const [addingWorkspace, setAddingWorkspace] = useState(false);
  const [hostName, setHostName] = useState("");
  const [sshHost, setSshHost] = useState("");
  const [repoPath, setRepoPath] = useState("");
  const [repoName, setRepoName] = useState("");
  const [name, setName] = useState("");
  const [command, setCommand] = useState("bash");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!hostId && hosts.length) {
      setHostId(initialHostId ?? hosts.find((host) => host.sshHost)?.id ?? hosts[0].id);
    }
  }, [hosts, hostId, initialHostId]);

  useEffect(() => {
    if (!hostId) return;
    let cancelled = false;
    setWorkspaceId("");
    setWorkspaces([]);
    api.listWorkspaces(hostId)
      .then((items) => {
        if (cancelled) return;
        setWorkspaces(items);
        setWorkspaceId(
          items.find((item) => item.id === initialWorkspaceId)?.id ?? items[0]?.id ?? ""
        );
      })
      .catch((reason: unknown) => {
        if (!cancelled) setError(errorMessage(reason));
      });
    return () => { cancelled = true; };
  }, [hostId, initialWorkspaceId]);

  async function addHost(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const host = await api.addHost(hostName, sshHost);
      await onHostAdded();
      setHostId(host.id);
      setAddingHost(false);
      setHostName("");
      setSshHost("");
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  async function addWorkspace(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const workspace = await api.addWorkspace(
        hostId,
        repoPath,
        repoName.trim() || repoPath.trim().split("/").filter(Boolean).pop() || repoPath
      );
      setWorkspaces((items) => [...items, workspace]);
      setWorkspaceId(workspace.id);
      await onWorkspaceAdded();
      setAddingWorkspace(false);
      setRepoPath("");
      setRepoName("");
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  async function create(event: FormEvent) {
    event.preventDefault();
    setBusy(true);
    setError("");
    try {
      const session = await api.createSession(workspaceId, name, command, 100, 30);
      await onCreated(session);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dialog-backdrop" role="presentation">
      <section className="dialog" role="dialog" aria-modal="true" aria-labelledby="new-session-title">
        <div className="flex items-center justify-between">
          <h2 id="new-session-title" className="text-xl font-semibold">New Session</h2>
          <button type="button" className="icon-button" onClick={onClose} aria-label="Close">×</button>
        </div>
        <div className="mt-5 space-y-4">
          <div>
            <div className="flex items-center justify-between">
              <label htmlFor="host" className="field-label">Host</label>
              <button type="button" className="text-link" onClick={() => setAddingHost(!addingHost)}>
                {addingHost ? "Cancel" : "+ Add SSH host"}
              </button>
            </div>
            <select id="host" className="field w-full" value={hostId} onChange={(event) => setHostId(event.target.value)}>
              {hosts.map((host) => (
                <option key={host.id} value={host.id}>
                  {host.name}{host.sshHost ? ` (${host.sshHost})` : ""}
                </option>
              ))}
            </select>
          </div>
          {addingHost && (
            <form onSubmit={(event) => void addHost(event)} className="subform">
              <label className="field-label" htmlFor="host-name">Display name</label>
              <input id="host-name" className="field w-full" value={hostName} onChange={(event) => setHostName(event.target.value)} required placeholder="Development server" />
              <label className="field-label" htmlFor="ssh-host">SSH config alias</label>
              <input id="ssh-host" className="field w-full" value={sshHost} onChange={(event) => setSshHost(event.target.value)} required placeholder="dev-box" />
              <button className="secondary-button" type="submit" disabled={busy}>Save host</button>
            </form>
          )}
          <div>
            <div className="flex items-center justify-between">
              <label htmlFor="workspace" className="field-label">Repository</label>
              <button type="button" className="text-link" onClick={() => setAddingWorkspace(!addingWorkspace)}>
                {addingWorkspace ? "Cancel" : "+ Add repository"}
              </button>
            </div>
            <select id="workspace" className="field w-full" value={workspaceId} onChange={(event) => setWorkspaceId(event.target.value)}>
              {workspaces.length === 0 && <option value="">Add a repository first</option>}
              {workspaces.map((workspace) => (
                <option key={workspace.id} value={workspace.id}>
                  {workspace.displayName} · {workspace.repoPath}
                </option>
              ))}
            </select>
          </div>
          {addingWorkspace && (
            <form onSubmit={(event) => void addWorkspace(event)} className="subform">
              <label className="field-label" htmlFor="repo-path">Absolute path on selected host</label>
              <input id="repo-path" className="field w-full" value={repoPath} onChange={(event) => setRepoPath(event.target.value)} required placeholder="/home/developer/projects/my-app" />
              <label className="field-label" htmlFor="repo-name">Display name (optional)</label>
              <input id="repo-name" className="field w-full" value={repoName} onChange={(event) => setRepoName(event.target.value)} placeholder="my-app" />
              <button className="secondary-button" type="submit" disabled={busy || !hostId}>Save repository</button>
            </form>
          )}
          <form id="create-session" onSubmit={(event) => void create(event)} className="space-y-4">
            <div>
              <label className="field-label" htmlFor="session-name">Session name</label>
              <input id="session-name" className="field w-full" value={name} onChange={(event) => setName(event.target.value)} required placeholder="Fix OAuth callback" />
            </div>
            <div>
              <label className="field-label" htmlFor="command">Terminal command</label>
              <input id="command" className="field w-full font-mono text-sm" value={command} onChange={(event) => setCommand(event.target.value)} required />
              <p className="mt-1 text-xs text-slate-400">Persistent shell by default. Copilot chat connects separately. Shell expressions are not evaluated.</p>
            </div>
          </form>
          {error && <p role="alert" className="error-box">{error}</p>}
        </div>
        <footer className="mt-6 flex justify-end gap-2">
          <button className="secondary-button" type="button" onClick={onClose}>Cancel</button>
          <button className="primary-button" type="submit" form="create-session" disabled={busy || !workspaceId}>
            {busy ? "Working..." : "Create session"}
          </button>
        </footer>
      </section>
    </div>
  );
}
