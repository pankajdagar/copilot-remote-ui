import { useCallback, useEffect, useState } from "react";

import { api, errorMessage } from "../api";
import type { ChangedFile } from "../types";

export function ChangesPanel({
  sessionId,
  onCount
}: {
  sessionId: string;
  onCount: (count: number) => void;
}) {
  const [files, setFiles] = useState<ChangedFile[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const load = useCallback(async () => {
    setLoading(true);
    setError("");
    try {
      const changes = await api.listChanges(sessionId);
      setFiles(changes);
      onCount(changes.length);
    } catch (reason) {
      setError(`Cannot load remote Git changes: ${errorMessage(reason)}`);
    } finally {
      setLoading(false);
    }
  }, [sessionId, onCount]);

  useEffect(() => { void load(); }, [load]);

  return (
    <section className="min-h-0 flex-1 overflow-y-auto p-6" aria-label="Remote repository changes">
      <div className="mx-auto max-w-3xl">
        <div className="mb-5 flex items-center justify-between">
          <h3 className="text-lg font-semibold">Repository changes {files.length > 0 && `(${files.length})`}</h3>
          <button type="button" className="secondary-button" disabled={loading} onClick={() => void load()}>
            Refresh
          </button>
        </div>
        {error && <p className="error-box" role="alert">{error}</p>}
        {loading ? <p className="text-sm text-slate-400">Checking the remote repository...</p>
          : files.length === 0 && !error ? <p className="text-sm text-slate-400">Working tree is clean.</p>
          : <ul className="space-y-1">
            {files.map((file) => (
              <li key={file.path} className="flex items-center gap-4 rounded-md bg-panel px-4 py-3 text-sm">
                <span className="w-8 shrink-0 font-mono text-accent">{file.status.trim() || file.status}</span>
                <span className="min-w-0 truncate font-mono text-slate-200" title={file.path}>{file.path}</span>
              </li>
            ))}
          </ul>}
      </div>
    </section>
  );
}
