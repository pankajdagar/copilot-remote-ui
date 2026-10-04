import { useCallback, useEffect, useState, type FormEvent } from "react";
import { PlugZap, RotateCcw } from "lucide-react";

import { api, errorMessage } from "../api";
import type { McpAuthResult, McpInventory } from "../types";

type CliCandidate = McpInventory["availableCli"][number];

export function IntegrationsPanel({ sessionId, host }: { sessionId: string; host: string }) {
  const [inventory, setInventory] = useState<McpInventory | null>(null);
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [reviewed, setReviewed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [confirmReauth, setConfirmReauth] = useState<string | null>(null);
  const [importing, setImporting] = useState<CliCandidate | null>(null);
  const [confirmedImport, setConfirmedImport] = useState(false);
  const [auth, setAuth] = useState<{ name: string; result: McpAuthResult } | null>(null);
  const refresh = useCallback(async () => {
    try {
      setInventory(await api.listMcpServers(sessionId));
      setMessage(null);
    } catch (reason) {
      setMessage(`Could not list MCP servers: ${errorMessage(reason)}`);
    }
  }, [sessionId]);

  useEffect(() => { void refresh(); }, [refresh]);

  async function add(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (!reviewed || busy) return;
    setBusy(true);
    try {
      await api.addMcpServer(sessionId, name.trim(), url.trim(), true);
      setName("");
      setUrl("");
      setReviewed(false);
      await refresh();
    } catch (reason) {
      setMessage(`MCP server needs attention: ${errorMessage(reason)}`);
      try {
        setInventory(await api.listMcpServers(sessionId));
      } catch (refreshError) {
        setMessage(`MCP add failed: ${errorMessage(reason)}. Could not refresh: ${errorMessage(refreshError)}`);
      }
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!removing || busy) return;
    setBusy(true);
    try {
      await api.removeMcpServer(sessionId, removing);
      setRemoving(null);
      setAuth(null);
      await refresh();
    } catch (reason) {
      setMessage(`Could not remove MCP server: ${errorMessage(reason)}`);
    } finally {
      setBusy(false);
    }
  }

  async function importExisting() {
    if (!importing || !confirmedImport || busy) return;
    const target = importing.name;
    setBusy(true);
    try {
      await api.importCliMcpServer(sessionId, target, true);
      setImporting(null);
      setConfirmedImport(false);
      await refresh();
      setMessage(`${target} approved for this remote host. Use Activate in Chat, or Chat → Settings → Reconnect Copilot.`);
    } catch (reason) {
      setMessage(`Could not enable ${target}: ${errorMessage(reason)}`);
      try {
        setInventory(await api.listMcpServers(sessionId));
      } catch (refreshError) {
        setMessage(`Could not enable ${target}: ${errorMessage(reason)}. Could not refresh: ${errorMessage(refreshError)}`);
      }
    } finally {
      setBusy(false);
    }
  }

  async function activateExisting(server: string) {
    if (busy) return;
    setBusy(true);
    try {
      const status = await api.activateCliMcpServer(sessionId, server);
      await refresh();
      setMessage(`${server} is ${status} in this Chat.`);
    } catch (reason) {
      setMessage(`Could not activate ${server}: ${errorMessage(reason)}`);
      try {
        setInventory(await api.listMcpServers(sessionId));
      } catch (refreshError) {
        setMessage(`Could not activate ${server}: ${errorMessage(reason)}. Could not refresh: ${errorMessage(refreshError)}`);
      }
    } finally {
      setBusy(false);
    }
  }

  async function authenticate(server: string, forceReauth: boolean) {
    setBusy(true);
    setAuth(null);
    try {
      const result = await api.authenticateMcpServer(sessionId, server, forceReauth);
      setAuth({ name: server, result });
      await refresh();
    } catch (reason) {
      setMessage(`Could not ${forceReauth ? "reauthenticate" : "authenticate"} MCP: ${errorMessage(reason)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="min-h-0 flex-1 overflow-y-auto p-6" aria-label="MCP integrations">
      <div className="mx-auto max-w-3xl space-y-5">
        <header className="flex items-center justify-between gap-3">
          <div>
            <h2 className="flex items-center gap-2 text-lg font-semibold"><PlugZap size={19} aria-hidden />MCP · {host}</h2>
            <p className="mt-1 text-xs text-slate-400">Reviewed HTTPS servers for Chat sessions on this remote host; no tokens stored by this app.</p>
          </div>
          <button type="button" className="secondary-button flex items-center gap-2" onClick={() => void refresh()}>
            <RotateCcw size={14} aria-hidden />Refresh status
          </button>
        </header>
        {message && <p className="error-box" role="alert">{message}</p>}
        {inventory?.warning && <p className="rounded-lg border border-amber-500/30 bg-amber-500/10 p-3 text-sm text-amber-200" role="alert">
          Live MCP status unavailable: {inventory.warning}
        </p>}
        {auth && (
          <article className="rounded-xl border border-accent/30 bg-accent/5 p-4 text-sm">
            <h3 className="font-medium">Sign in · {auth.name}</h3>
            <p className="mt-2 text-slate-300">{auth.result.note}</p>
            {auth.result.authorizationUrl && (
              <>
                <p className="mt-3 text-xs text-amber-200">The callback is on remote host. A Mac browser cannot finish this flow.</p>
                <p className="mt-3 break-all rounded bg-canvas p-2 font-mono text-xs select-text">
                  {auth.result.authorizationUrl}
                </p>
              </>
            )}
          </article>
        )}
        <article className="rounded-xl border border-white/10 bg-panel p-5">
          <h3 className="font-medium">Reviewed servers</h3>
          <div className="mt-3 space-y-3">
            {inventory?.imported.map((server) => {
              const transport = inventory.availableCli.find((item) => item.name === server.name)?.transport;
              const oauthCapable = transport === "http" || transport === "sse";
              return (
              <div key={`cli-${server.name}`} className="rounded-lg border border-white/10 bg-canvas p-3 text-sm">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="font-medium">{server.name} <span className="text-xs text-slate-400">· Copilot CLI config</span></span>
                  <span className="rounded-full bg-raised px-2 py-1 text-xs text-slate-300">{server.status}</span>
                </div>
                {server.needsReview && <p className="mt-2 text-xs text-amber-200" role="alert">
                  Remote configuration changed or disappeared. Review it again before reconnecting Chat.
                </p>}
                {server.error && <p className="mt-2 text-xs text-amber-200">{server.error}</p>}
                <div className="mt-3 flex flex-wrap gap-2">
                  <button type="button" className="toolbar-button"
                    disabled={busy || server.needsReview || ["connected", "pending", "not-attached", "disconnected"].includes(server.status)}
                    onClick={() => void activateExisting(server.name)}>Activate in Chat</button>
                  {oauthCapable && <>
                  <button type="button" className="toolbar-button"
                    disabled={busy || server.needsReview || server.status === "disabled" || server.status === "not-attached"}
                    onClick={() => void authenticate(server.name, false)}>Authenticate</button>
                  <button type="button" className="toolbar-button"
                    disabled={busy || server.needsReview || server.status === "disabled" || server.status === "not-attached"}
                    onClick={() => setConfirmReauth(server.name)}>Reauthenticate</button>
                  </>}
                  <button type="button" className="toolbar-button" disabled={busy}
                    onClick={() => setRemoving(server.name)}>Remove...</button>
                </div>
                {!oauthCapable && <p className="mt-2 text-xs text-slate-400">
                  This server's command and authentication are managed by Copilot CLI on remote host, not by this app.
                </p>}
              </div>
              );
            })}
            {inventory?.reviewed.map((server) => (
              <div key={server.name} className="rounded-lg border border-white/10 bg-canvas p-3 text-sm">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <span className="font-medium">{server.name}</span>
                  <span className="rounded-full bg-raised px-2 py-1 text-xs text-slate-300">{server.status}</span>
                </div>
                <p className="mt-1 break-all text-xs text-slate-400">{server.url}</p>
                {server.error && <p className="mt-2 text-xs text-amber-200">{server.error}</p>}
                <div className="mt-3 flex flex-wrap gap-2">
                  <button type="button" className="toolbar-button" disabled={busy}
                    onClick={() => void authenticate(server.name, false)}>Authenticate</button>
                  <button type="button" className="toolbar-button" disabled={busy}
                    onClick={() => setConfirmReauth(server.name)}>Reauthenticate</button>
                  <button type="button" className="toolbar-button" disabled={busy}
                    onClick={() => setRemoving(server.name)}>Remove...</button>
                </div>
                <p className="mt-2 text-xs text-slate-400">OAuth may need an approved browser on remote host; a Mac browser cannot reach its remote callback.</p>
              </div>
            ))}
            {inventory && inventory.reviewed.length === 0 && inventory.imported.length === 0 &&
              <p className="text-sm text-slate-400">No reviewed MCP servers on this remote host.</p>}
            {!inventory && !message && <p className="text-sm text-slate-400">Loading MCP servers...</p>}
          </div>
        </article>
        <article className="rounded-xl border border-white/10 bg-panel p-5">
          <h3 className="font-medium">Use an existing Copilot CLI MCP server</h3>
          <p className="mt-1 text-xs text-slate-400">
            Select a server configured in Copilot CLI <strong>on this remote host</strong>. This app reuses that
            remote configuration; it does not copy command arguments, environment variables or credentials.
          </p>
          <div className="mt-3 space-y-2">
            {inventory?.availableCli.map((candidate) => (
              <div key={candidate.name} className="flex flex-wrap items-center justify-between gap-3 rounded-lg bg-canvas px-3 py-2 text-xs">
                <div className="min-w-0">
                  <p className="font-medium text-slate-100">{candidate.name} · {candidate.transport}</p>
                  {(candidate.command || candidate.endpointHost) &&
                    <p className="mt-1 truncate text-slate-400">
                      {candidate.command ? `Program: ${candidate.command}` : `Host: ${candidate.endpointHost}`}
                    </p>}
                </div>
                {candidate.reviewed && !candidate.needsReview
                  ? <span className="text-accent">Reviewed</span>
                  : <button type="button" className="toolbar-button"
                      disabled={busy || !["stdio", "local", "http", "sse"].includes(candidate.transport)}
                      onClick={() => { setConfirmedImport(false); setImporting(candidate); }}>
                      {candidate.needsReview ? "Review changes..." : "Review & enable..."}
                    </button>}
              </div>
            ))}
            {inventory && inventory.availableCli.length === 0 && (
              <p className="text-xs text-slate-400">
                No user-configured MCP servers on this remote host. Configure the server with Copilot CLI
                on this host, then refresh. Plugin and managed servers cannot be imported here.
              </p>
            )}
          </div>
        </article>
        <form className="rounded-xl border border-white/10 bg-panel p-5" onSubmit={(event) => void add(event)}>
          <h3 className="font-medium">Add a new approved HTTPS MCP server</h3>
          <p className="mt-1 text-xs text-slate-400">Only remote HTTP MCP is supported. No local commands, headers, or API keys are installed.</p>
          <div className="mt-4 grid gap-3 sm:grid-cols-2">
            <label className="text-xs text-slate-300">Server name
              <input className="field mt-1 block w-full text-sm" value={name}
                onChange={(event) => setName(event.target.value)} required placeholder="approved-tools" />
            </label>
            <label className="text-xs text-slate-300">HTTPS endpoint
              <input className="field mt-1 block w-full text-sm" type="url" value={url}
                onChange={(event) => setUrl(event.target.value)} required placeholder="https://mcp.example.com/mcp" />
            </label>
          </div>
          <label className="mt-4 flex items-start gap-2 text-xs text-amber-200">
            <input type="checkbox" checked={reviewed} onChange={(event) => setReviewed(event.target.checked)} />
            I have reviewed this MCP server and am authorized to connect it from this remote host.
          </label>
          <button type="submit" className="primary-button mt-4" disabled={busy || !reviewed}>Add reviewed server</button>
        </form>
        {inventory?.external.length ? (
          <article className="rounded-xl border border-white/10 bg-panel p-5">
            <h3 className="font-medium">Other remote MCP servers</h3>
            <p className="mt-1 text-xs text-slate-400">Unreviewed user-configured servers are disabled for Chat. Workspace, plugin, managed or built-in servers may still be active; manage those outside this app.</p>
            <p className="mt-3 break-words text-xs text-slate-300">{inventory.external.join(" · ")}</p>
          </article>
        ) : null}
      </div>
      {removing && (
        <div className="dialog-backdrop">
          <section className="dialog max-w-md" role="dialog" aria-modal="true" aria-label="Remove reviewed MCP server?">
            <h3 className="font-medium">Remove {removing} from this remote host?</h3>
            <p className="mt-2 text-sm text-slate-300">Copilot Remote UI stops this server in active Chat sessions and removes its approval.
              {inventory?.imported.some((item) => item.name === removing)
                ? " Its Copilot CLI configuration remains available outside this app."
                : " Its saved HTTPS URL is removed."}
              {" "}Removing does not revoke OAuth access at the provider; revoke it separately if needed.</p>
            <div className="mt-4 flex justify-end gap-2">
              <button type="button" className="secondary-button" onClick={() => setRemoving(null)}>Cancel</button>
              <button type="button" className="secondary-button" disabled={busy}
                onClick={() => void remove()}>Remove server</button>
            </div>
          </section>
        </div>
      )}
      {confirmReauth && (
        <div className="dialog-backdrop">
          <section className="dialog max-w-md" role="dialog" aria-modal="true" aria-label="Reauthenticate MCP server?">
            <h3 className="font-medium">Reauthenticate {confirmReauth}?</h3>
            <p className="mt-2 text-sm text-slate-300">Copilot will clear this server's cached token.
              Have an approved browser on remote host available to complete the new OAuth callback.</p>
            <div className="mt-4 flex justify-end gap-2">
              <button type="button" className="secondary-button" onClick={() => setConfirmReauth(null)}>Cancel</button>
              <button type="button" className="secondary-button" disabled={busy}
                onClick={() => {
                  const server = confirmReauth;
                  setConfirmReauth(null);
                  void authenticate(server, true);
                }}>Start reauthentication</button>
            </div>
          </section>
        </div>
      )}
      {importing && (
        <div className="dialog-backdrop">
          <section className="dialog max-w-md" role="dialog" aria-modal="true" aria-label="Review existing MCP configuration">
            <h3 className="font-medium">Enable {importing.name} on this remote host?</h3>
            <p className="mt-2 text-sm text-slate-300">
              Copilot CLI reports a {importing.transport} server
              {importing.command ? ` using ${importing.command}` : importing.endpointHost ? ` at ${importing.endpointHost}` : ""}.
              Its command, arguments, environment and credentials stay in the remote CLI configuration.
            </p>
            <p className="mt-2 text-xs text-amber-200">
              Review the full remote CLI configuration and your enterprise policy first. A stdio MCP server can run code
              on remote host. Changes to that configuration require a new review. MCP tool calls remain subject to approval.
            </p>
            <label className="mt-4 flex items-start gap-2 text-xs text-slate-200">
              <input type="checkbox" checked={confirmedImport}
                onChange={(event) => setConfirmedImport(event.target.checked)} />
              I reviewed the complete MCP configuration on this remote host and am authorized to run it.
            </label>
            <div className="mt-4 flex justify-end gap-2">
              <button type="button" className="secondary-button"
                onClick={() => { setImporting(null); setConfirmedImport(false); }}>Cancel</button>
              <button type="button" className="secondary-button" disabled={!confirmedImport || busy}
                onClick={() => void importExisting()}>Enable for Chat</button>
            </div>
          </section>
        </div>
      )}
    </section>
  );
}
