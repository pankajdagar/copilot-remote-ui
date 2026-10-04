import { useCallback, useEffect, type MutableRefObject } from "react";
import { TerminalView, type TerminalControls } from "./TerminalView";

export function CachedTerminal({
  sessionId,
  active,
  reconnectKey,
  controls,
  onConnected,
  onDisconnected,
  onMouseModeChange,
  onMouseReports,
  onError,
  onEvicted
}: {
  sessionId: string;
  active: boolean;
  reconnectKey: number;
  controls: MutableRefObject<Map<string, TerminalControls>>;
  onConnected: (id: string) => void;
  onDisconnected: (id: string, message: string) => void;
  onMouseModeChange: (id: string, mode: string) => void;
  onMouseReports: (id: string, count: number) => void;
  onError: (id: string, message: string) => void;
  onEvicted: (id: string) => void;
}) {
  const setRef = useCallback((value: TerminalControls | null) => {
    if (value) controls.current.set(sessionId, value);
    else controls.current.delete(sessionId);
  }, [controls, sessionId]);
  const connected = useCallback(() => onConnected(sessionId), [onConnected, sessionId]);
  const disconnected = useCallback((message: string) => onDisconnected(sessionId, message), [onDisconnected, sessionId]);
  const mouseMode = useCallback((mode: string) => onMouseModeChange(sessionId, mode), [onMouseModeChange, sessionId]);
  const mouseReports = useCallback((count: number) => onMouseReports(sessionId, count), [onMouseReports, sessionId]);
  const error = useCallback((message: string) => onError(sessionId, message), [onError, sessionId]);

  useEffect(() => () => onEvicted(sessionId), [onEvicted, sessionId]);

  return (
    <div className={`min-h-0 flex-1 p-3 ${active ? "" : "hidden"}`} aria-hidden={!active}>
      <TerminalView
        key={reconnectKey}
        ref={setRef}
        sessionId={sessionId}
        active={active}
        reconnectKey={reconnectKey}
        onConnected={connected}
        onDisconnected={disconnected}
        onMouseModeChange={mouseMode}
        onMouseReports={mouseReports}
        onError={error}
      />
    </div>
  );
}
