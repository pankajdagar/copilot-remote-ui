import { Channel } from "@tauri-apps/api/core";
import { useVirtualizer } from "@tanstack/react-virtual";
import * as Popover from "@radix-ui/react-popover";
import { ChevronDown, Send, SlidersHorizontal, Square } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";

import { api, errorMessage } from "../api";
import {
  mergeAgentEvents, decidePermission, finishConversationTurn, groupConversation,
  type ConversationItem
} from "../agent/conversation";
import { mergeSubagentTraces, type SubagentTraces } from "../agent/subagents";
import { useUi } from "../store";
import type { AgentConnectionStatus, AgentEvent, AgentSnapshot, ChatMode, ContextTier, CopilotModel, PermissionMode } from "../types";
import { ConversationRow } from "./ConversationRow";
import { RecentCliChatsDialog } from "./RecentCliChatsDialog";
import { SubagentInspector } from "./SubagentInspector";

type RequestedModel = {
  modelId: string; effort: string | null; tier: ContextTier | null; queued: boolean;
};

function matchesModel(
  modelId: string | null, effort: string | null, tier: ContextTier | null, expected: RequestedModel
): boolean {
  return modelId === expected.modelId &&
    (expected.effort === null || expected.effort === effort) &&
    (expected.tier === null || expected.tier === tier);
}

export function ChatPanel({
  sessionId,
  active,
  autoCreate,
  workspacePath,
  onLinked,
  onError
}: {
  sessionId: string;
  active: boolean;
  autoCreate: boolean;
  workspacePath?: string;
  onLinked: (id: string) => Promise<void>;
  onError: (message: string) => void;
}) {
  const [items, setItems] = useState<ConversationItem[]>([]);
  const [traces, setTraces] = useState<SubagentTraces>({});
  const [selectedTraceId, setSelectedTraceId] = useState<string | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [status, setStatus] = useState<AgentConnectionStatus | "connecting">("connecting");
  const [message, setMessage] = useState("");
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [stopping, setStopping] = useState(false);
  const [working, setWorking] = useState(false);
  const [keepingAwake, setKeepingAwake] = useState(false);
  const [sleepWarning, setSleepWarning] = useState<string | null>(null);
  const [intent, setIntent] = useState<string | null>(null);
  const [responding, setResponding] = useState<string | null>(null);
  const [stuckAboveBottom, setStuckAboveBottom] = useState(false);
  const [confirmAllowAll, setConfirmAllowAll] = useState(false);
  const [confirmAutopilot, setConfirmAutopilot] = useState(false);
  const [confirmReviewedMcp, setConfirmReviewedMcp] = useState(false);
  const [reviewedMcp, setReviewedMcp] = useState<{ available: string[]; enabled: boolean }>({
    available: [], enabled: false
  });
  const [reviewedMcpError, setReviewedMcpError] = useState<string | null>(null);
  const [recentOpen, setRecentOpen] = useState(false);
  const [switchBusy, setSwitchBusy] = useState(false);
  const [modeBusy, setModeBusy] = useState(false);
  const [agentMode, setAgentMode] = useState<ChatMode>("unsupported");
  const [models, setModels] = useState<CopilotModel[]>([]);
  const [selectedModel, setSelectedModel] = useState<string | null>(null);
  const [contextTier, setContextTier] = useState<ContextTier | null>(null);
  const [reasoningEffort, setReasoningEffort] = useState<string | null>(null);
  const [pendingChange, setPendingChange] = useState<string | null>(null);
  const [modelsLoading, setModelsLoading] = useState(false);
  const [modelBusy, setModelBusy] = useState(false);
  const [modelError, setModelError] = useState<string | null>(null);
  const [modelsError, setModelsError] = useState<string | null>(null);
  const [historyVersion, setHistoryVersion] = useState(0);
  const viewport = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const previousScrollTop = useRef(0);
  const wheelDirection = useRef<"up" | "down" | null>(null);
  const dragging = useRef(false);
  const followFrame = useRef<number | null>(null);
  const deltaFrame = useRef<number | null>(null);
  const pendingDeltas = useRef<AgentEvent[]>([]);
  const sendState = useRef<"sending" | "working" | "idle">("idle");
  const turnStarted = useRef(false);
  const turnStart = useRef<number | null>(null);
  const stopRequested = useRef(false);
  const modelEventVersion = useRef(0);
  const reviewedMcpVersion = useRef(0);
  const pendingSettings = useRef<RequestedModel | null>(null);
  const seen = useRef(new Set<string>());
  const generation = useRef(0);
  const retries = useRef(0);
  const lastConnectedAt = useRef(0);
  const retryTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const setAgentStatus = useUi((state) => state.setAgentStatus);
  const permissionMode = useUi((state) => state.permissionModes[sessionId] ?? "ask");
  const setPermissionMode = useUi((state) => state.setPermissionMode);
  const visibleItems = useMemo(() => groupConversation(items), [items]);
  const traceList = useMemo(() => Object.values(traces), [traces]);
  const selectedTrace = traceList.find((trace) =>
    trace.id === selectedTraceId || trace.toolCallId === selectedTraceId
  );
  const activeTool = useMemo(() => {
    for (let index = items.length - 1; index >= 0; index--) {
      const item = items[index];
      if (item.kind === "tool" && item.success === null && !item.concluded) return item;
    }
    return null;
  }, [items]);
  const latestTurn = items.at(-1);
  const rowCount = visibleItems.length;
  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => viewport.current,
    getItemKey: (index) => `${visibleItems[index].kind}:${visibleItems[index].id}`,
    estimateSize: (index) => visibleItems[index]?.kind === "message" ? 160 : 100,
    gap: 16,
    overscan: 5,
    anchorTo: following.current ? "end" : "start",
    followOnAppend: following.current,
    scrollEndThreshold: 8,
    initialRect: { width: 800, height: 560 },
    useFlushSync: false
  });

  const flushDeltas = useCallback(() => {
    deltaFrame.current = null;
    const batch = pendingDeltas.current.splice(0);
    if (batch.length > 0) {
      const root = batch.filter((event) => event.type !== "subagentEvent");
      if (root.length) setItems((current) => mergeAgentEvents(current, root));
      if (batch.some((event) => event.type === "subagentEvent")) {
        setTraces((current) => mergeSubagentTraces(current, batch));
      }
    }
  }, []);

  const refreshModel = useCallback(async () => {
    const request = generation.current;
    try {
      const current = await api.getCopilotModelState(sessionId);
      if (request !== generation.current) return;
      if (current.currentModel) {
        setSelectedModel(current.currentModel);
        setContextTier(current.contextTier);
        setReasoningEffort(current.reasoningEffort);
      }
      const expected = pendingSettings.current;
      if (expected && matchesModel(
        current.currentModel, current.reasoningEffort, current.contextTier, expected
      )) {
        pendingSettings.current = null;
        setPendingChange(null);
      } else if (expected && !expected.queued) {
        pendingSettings.current = null;
        setPendingChange(null);
        setModelError("Copilot has not confirmed the requested model settings. The active settings are shown; retry if needed.");
      }
    } catch (reason) {
      if (request === generation.current) setModelError(
        `Could not refresh Copilot model state: ${errorMessage(reason)}`
      );
    }
  }, [sessionId]);

  const receive = useCallback((event: AgentEvent) => {
    if (seen.current.has(event.eventId)) return;
    seen.current.add(event.eventId);
    if (event.type === "working") turnStarted.current = true;
    const shouldFinish = event.type === "idle" && turnStarted.current;
    const wasStopped = event.type === "idle" && (event.aborted || stopRequested.current);
    if (event.type === "idle") {
      turnStarted.current = false;
      stopRequested.current = false;
    }
    const streamed = event.type === "assistantDelta" || event.type === "reasoningDelta" ||
      event.type === "toolOutput" || event.type === "subagentEvent" && (
        event.event.type === "assistantDelta" || event.event.type === "reasoningDelta" ||
        event.event.type === "toolOutput"
      );
    if (streamed) {
      pendingDeltas.current.push(event);
      if (deltaFrame.current === null) deltaFrame.current = requestAnimationFrame(flushDeltas);
    } else {
      if (deltaFrame.current !== null) cancelAnimationFrame(deltaFrame.current);
      deltaFrame.current = null;
      const batch = pendingDeltas.current.splice(0);
      const all = [...batch, event];
      const root = all.filter((candidate) => candidate.type !== "subagentEvent");
      if (all.some((candidate) =>
        candidate.type === "subagentEvent" || candidate.type === "subagentStarted" ||
        candidate.type === "subagentCompleted" || candidate.type === "subagentFailed"
      )) setTraces((current) => mergeSubagentTraces(current, all));
      setItems((current) => {
        if (event.type === "working" && turnStart.current === null) {
          turnStart.current = current.length;
        }
        const next = mergeAgentEvents(current, root);
        if (shouldFinish) {
          const boundary = turnStart.current ?? current.length;
          turnStart.current = null;
          return finishConversationTurn(next, event.eventId, wasStopped, boundary);
        }
        if (event.type === "error" || event.type === "disconnected") {
          turnStart.current = null;
        }
        return next;
      });
    }
    if (event.type === "working") {
      sendState.current = "working";
      setIntent(null);
      setWorking(true);
      setAgentStatus(sessionId, "working");
    } else if (event.type === "intent") {
      setIntent(event.content.trim() || null);
    } else if (event.type === "modelChanged") {
      modelEventVersion.current++;
      setSelectedModel(event.modelId);
      setContextTier(event.contextTier);
      setReasoningEffort(event.reasoningEffort);
      const expected = pendingSettings.current;
      if (expected && matchesModel(event.modelId, event.reasoningEffort, event.contextTier, expected)) {
        pendingSettings.current = null;
        setPendingChange(null);
      }
      if (event.contextTier === null || event.reasoningEffort === null) void refreshModel();
    } else if (event.type === "idle") {
      sendState.current = "idle";
      setIntent(null);
      setWorking(false);
      setStopping(false);
      setAgentStatus(sessionId, "idle");
      if (pendingSettings.current) void refreshModel();
    } else if (event.type === "sleepStatus") {
      setKeepingAwake(event.active);
      setSleepWarning(event.error);
    } else if (event.type === "permissionRequested") {
      setAgentStatus(sessionId, "awaitingPermission");
    } else if (event.type === "error" || event.type === "disconnected") {
      sendState.current = "idle";
      turnStarted.current = false;
      turnStart.current = null;
      stopRequested.current = false;
      setIntent(null);
      setWorking(false);
      setKeepingAwake(false);
      setSleepWarning(null);
      setStopping(false);
      setAgentStatus(sessionId, "error");
      setMessage(event.message);
      if (event.type === "disconnected") setStatus("unavailable");
    }
  }, [sessionId, setAgentStatus, flushDeltas, refreshModel]);

  const loadModels = useCallback(async (request: number) => {
    setModelsLoading(true);
    setModelsError(null);
    try {
      const available = await api.listCopilotModels(sessionId);
      if (request !== generation.current) return;
      setModels(available);
      if (available.length === 0) setModelsError("Copilot returned no selectable models.");
    } catch (reason) {
      if (request === generation.current) setModelsError(`Could not load models: ${errorMessage(reason)}`);
    } finally {
      if (request === generation.current) setModelsLoading(false);
    }
  }, [sessionId]);

  const loadReviewedMcp = useCallback(async (request: number) => {
    const version = ++reviewedMcpVersion.current;
    try {
      const state = await api.reviewedMcpApprovalState(sessionId);
      if (request === generation.current && version === reviewedMcpVersion.current) {
        setReviewedMcp(state);
        setReviewedMcpError(null);
      }
    } catch (reason) {
      if (request === generation.current && version === reviewedMcpVersion.current) {
        setReviewedMcp({ available: [], enabled: false });
        setReviewedMcpError(`Could not check reviewed MCP approvals: ${errorMessage(reason)}`);
      }
    }
  }, [sessionId]);

  const restoreConnectedSnapshot = useCallback((
    snapshot: AgentSnapshot, buffered: AgentEvent[], request: number
  ): boolean => {
    seen.current.clear();
    if (deltaFrame.current !== null) cancelAnimationFrame(deltaFrame.current);
    deltaFrame.current = null;
    pendingDeltas.current = [];
    const unique: AgentEvent[] = [];
    for (const event of [...snapshot.events, ...buffered]) {
      if (seen.current.has(event.eventId)) continue;
      seen.current.add(event.eventId);
      unique.push(event);
    }
    const rootEvents = unique.filter((event) => event.type !== "subagentEvent");
    setTraces(mergeSubagentTraces({}, unique));
    let restoredItems = mergeAgentEvents([], rootEvents);
    const lastStart = buffered.reduce((index, event, current) =>
      event.type === "working" ? current : index, -1);
    const lastIdle = buffered.reduce((index, event, current) =>
      event.type === "idle" ? current : index, -1);
    const boundary = lastStart >= 0
      ? mergeAgentEvents([], [...snapshot.events, ...buffered.slice(0, lastStart)]
        .filter((event) => event.type !== "subagentEvent")).length
      : restoredItems.length;
    if (lastStart >= 0 && lastIdle > lastStart) {
      const idle = buffered[lastIdle];
      if (idle.type === "idle") {
        restoredItems = finishConversationTurn(restoredItems, idle.eventId, idle.aborted, boundary);
      }
    }
    setItems(restoredItems);
    virtualizer.measure();
    setHistoryVersion((current) => current + 1);
    const lost = buffered.some((event) => event.type === "disconnected");
    setStatus(lost ? "unavailable" : "connected");
    setAgentMode(snapshot.mode);
    setSelectedModel(snapshot.model);
    setContextTier(snapshot.contextTier);
    setReasoningEffort(snapshot.reasoningEffort);
    setModelError(snapshot.modelError);
    setMessage(snapshot.error ?? (snapshot.modeError
      ? `Copilot mode unavailable: ${snapshot.modeError}`
      : snapshot.mcpWarnings.length
        ? `MCP configuration changed or disappeared on remote host: ${snapshot.mcpWarnings.join(", ")}. Review it in Integrations before enabling.`
        : ""));
    if (lost) {
      setAgentStatus(sessionId, "error");
      return true;
    }
    const lastModelChange = buffered.filter((event) => event.type === "modelChanged").at(-1);
    if (lastModelChange?.type === "modelChanged") {
      setSelectedModel(lastModelChange.modelId);
      setContextTier(lastModelChange.contextTier);
      setReasoningEffort(lastModelChange.reasoningEffort);
      if (lastModelChange.contextTier === null || lastModelChange.reasoningEffort === null) {
        void refreshModel();
      }
    }
    void loadModels(request);
    void loadReviewedMcp(request);
    retries.current = 0;
    lastConnectedAt.current = Date.now();
    const lastState = buffered.filter((event) =>
      event.type === "working" || event.type === "idle" || event.type === "permissionRequested"
    ).at(-1);
    const isWorking = lastState?.type === "working";
    turnStarted.current = isWorking;
    turnStart.current = isWorking ? boundary : null;
    stopRequested.current = false;
    const latestSleep = buffered.filter((event) => event.type === "sleepStatus").at(-1);
    if (latestSleep?.type === "sleepStatus") {
      setKeepingAwake(latestSleep.active);
      setSleepWarning(latestSleep.error);
    }
    sendState.current = isWorking ? "working" : "idle";
    setWorking(isWorking);
    setAgentStatus(sessionId, lastState?.type === "permissionRequested"
      ? "awaitingPermission" : isWorking ? "working" : "idle");
    if (snapshot.copilotSessionId) void onLinked(sessionId);
    return false;
  }, [sessionId, setAgentStatus, onLinked, virtualizer, refreshModel, loadModels, loadReviewedMcp]);

  const connect = useCallback(async (mode: "resume" | "create" | "replace") => {
    setSettingsOpen(false);
    if (mode !== "resume") setSelectedTraceId(null);
    if (retryTimer.current) clearTimeout(retryTimer.current);
    retryTimer.current = null;
    const request = ++generation.current;
    const buffered: AgentEvent[] = [];
    let restored = false;
    setStatus("connecting");
    sendState.current = "idle";
    turnStarted.current = false;
    turnStart.current = null;
    stopRequested.current = false;
    setWorking(false);
    setStopping(false);
    setModels([]);
    setSelectedModel(null);
    setContextTier(null);
    setReasoningEffort(null);
    setPendingChange(null);
    pendingSettings.current = null;
    modelEventVersion.current = 0;
    setModelsLoading(false);
    setModelsError(null);
    setModelError(null);
    setReviewedMcp({ available: [], enabled: false });
    setReviewedMcpError(null);
    reviewedMcpVersion.current++;
    setIntent(null);
    setAgentStatus(sessionId, "connecting");
    setMessage("");
    function retryAfterDisconnect(retryMode: "resume" | "create" | "replace" = "resume") {
      if (request !== generation.current || retryTimer.current) return;
      const delay = Math.min(1000 * 2 ** Math.min(retries.current, 5), 30_000);
      retries.current++;
      retryTimer.current = setTimeout(() => void connect(retryMode), delay);
    }
    const channel = new Channel<AgentEvent>();
    channel.onmessage = (event) => {
      if (request !== generation.current) return;
      if (!restored) buffered.push(event);
      else {
        receive(event);
        if (event.type === "disconnected") retryAfterDisconnect();
      }
    };
    try {
      const snapshot = mode === "replace"
        ? await api.replaceChat(sessionId, channel)
        : mode === "create"
          ? await api.createChat(sessionId, channel)
          : await api.connectChat(sessionId, channel);
      if (request !== generation.current) return;
      if (snapshot.status === "connected") {
        const lost = restoreConnectedSnapshot(snapshot, buffered, request);
        restored = true;
        if (lost) retryAfterDisconnect();
      } else {
        restored = true;
        const lost = buffered.some((event) => event.type === "disconnected");
        setStatus(lost ? "unavailable" : snapshot.status);
        setAgentMode(snapshot.mode);
        setMessage(snapshot.error ?? "");
        setAgentStatus(sessionId, lost ? "error" : snapshot.status);
        if (lost || snapshot.status === "hostOffline" || snapshot.status === "unavailable") {
          retryAfterDisconnect(lost ? "resume" : mode);
        }
      }
    } catch (reason) {
      if (request !== generation.current) return;
      setStatus("unavailable");
      setAgentStatus(sessionId, "unavailable");
      setMessage(`Copilot connection failed: ${errorMessage(reason)}`);
      retryAfterDisconnect(mode);
    }
  }, [sessionId, receive, setAgentStatus, restoreConnectedSnapshot]);

  async function attachRecent(selectedId: string, allowDifferentRepo: boolean) {
    if (switchBusy) throw new Error("A Copilot conversation is already being attached");
    setSwitchBusy(true);
    const buffered: AgentEvent[] = [];
    let committed = false;
    let abandoned = false;
    let request = 0;
    const channel = new Channel<AgentEvent>();
    channel.onmessage = (event) => {
      if (abandoned) return;
      if (!committed) {
        buffered.push(event);
      } else if (request === generation.current) {
        receive(event);
        if (event.type === "disconnected" && !retryTimer.current) {
          retries.current = 1;
          retryTimer.current = setTimeout(() => void connect("resume"), 1000);
        }
      }
    };
    try {
      const snapshot = await api.attachCliChat(sessionId, selectedId, allowDifferentRepo, channel);
      if (snapshot.status !== "connected") {
        throw new Error(snapshot.error ?? `Copilot returned ${snapshot.status}`);
      }
      request = ++generation.current;
      if (retryTimer.current) clearTimeout(retryTimer.current);
      retryTimer.current = null;
      pendingSettings.current = null;
      setPendingChange(null);
      setModelError(null);
      setModelsError(null);
      setPermissionMode(sessionId, "ask");
      setReviewedMcp({ available: [], enabled: false });
      setSettingsOpen(false);
      setSelectedTraceId(null);
      const lost = restoreConnectedSnapshot(snapshot, buffered, request);
      committed = true;
      if (lost) {
        retries.current = 1;
        retryTimer.current = setTimeout(() => void connect("resume"), 1000);
      }
    } catch (reason) {
      abandoned = true;
      throw reason;
    } finally {
      setSwitchBusy(false);
    }
  }

  useEffect(() => {
    void connect(autoCreate ? "create" : "resume");
    return () => {
      generation.current++;
      if (retryTimer.current) clearTimeout(retryTimer.current);
      if (deltaFrame.current !== null) cancelAnimationFrame(deltaFrame.current);
      if (followFrame.current !== null) cancelAnimationFrame(followFrame.current);
      pendingDeltas.current = [];
      setAgentStatus(sessionId, "detached");
      void api.disconnectChat(sessionId).catch((reason: unknown) =>
        onError(`Could not detach Copilot chat: ${errorMessage(reason)}`)
      );
    };
  }, [sessionId, autoCreate, connect, onError]);

  useEffect(() => {
    if (status !== "connected") return;
    let disposed = false;
    let checking = false;
    async function check() {
      if (checking || disposed) return;
      checking = true;
      try {
        const healthy = await api.chatHealth(sessionId);
        if (!disposed && !healthy) {
          retries.current = 1;
          receive({
            type: "disconnected", eventId: `health-${Date.now()}`,
            message: "Copilot connection lost; reconnecting..."
          });
          void connect("resume");
        }
      } catch (reason) {
        if (!disposed) {
          retries.current = 1;
          receive({
            type: "disconnected", eventId: `health-${Date.now()}`,
            message: `Copilot health check failed: ${errorMessage(reason)}`
          });
          void connect("resume");
        }
      } finally {
        checking = false;
      }
    }
    if (Date.now() - lastConnectedAt.current > 10_000) void check();
    const interval = setInterval(() => { void check(); }, 15_000);
    return () => {
      disposed = true;
      clearInterval(interval);
    };
  }, [status, sessionId, receive, connect]);

  useEffect(() => {
    const stopDragging = () => { dragging.current = false; };
    window.addEventListener("pointerup", stopDragging);
    window.addEventListener("pointercancel", stopDragging);
    return () => {
      window.removeEventListener("pointerup", stopDragging);
      window.removeEventListener("pointercancel", stopDragging);
    };
  }, []);

  function followLatest() {
    wheelDirection.current = null;
    if (followFrame.current !== null) cancelAnimationFrame(followFrame.current);
    followFrame.current = requestAnimationFrame(() => {
      followFrame.current = null;
      if (!active || !following.current || rowCount === 0) return;
      virtualizer.scrollToOffset(virtualizer.getTotalSize());
      if (viewport.current) previousScrollTop.current = viewport.current.scrollTop;
    });
  }

  const totalHeight = virtualizer.getTotalSize();
  useLayoutEffect(() => {
    if (active && following.current) followLatest();
    return () => {
      if (followFrame.current !== null) cancelAnimationFrame(followFrame.current);
    };
  }, [active, historyVersion]);

  async function send() {
    if (status !== "connected" || sending || stopping || working || !draft.trim()) return;
    sendState.current = "sending";
    turnStarted.current = true;
    setItems((current) => {
      if (turnStart.current === null) turnStart.current = current.length;
      return current;
    });
    setSending(true);
    setWorking(true);
    setAgentStatus(sessionId, "working");
    setMessage("");
    try {
      await api.sendChatMessage(sessionId, draft.trim());
      setDraft("");
    } catch (reason) {
      setMessage(`Could not send message: ${errorMessage(reason)}`);
      if (sendState.current === "sending") {
        sendState.current = "idle";
        turnStarted.current = false;
        turnStart.current = null;
        setWorking(false);
        setAgentStatus(sessionId, "idle");
      }
    } finally {
      setSending(false);
    }
  }

  async function stop() {
    if (stopping) return;
    setStopping(true);
    stopRequested.current = true;
    try {
      await api.abortChat(sessionId);
      if (turnStarted.current) {
        turnStarted.current = false;
        if (deltaFrame.current !== null) cancelAnimationFrame(deltaFrame.current);
        deltaFrame.current = null;
        const buffered = pendingDeltas.current.splice(0);
        if (buffered.some((event) => event.type === "subagentEvent")) {
          setTraces((current) => mergeSubagentTraces(current, buffered));
        }
        setItems((current) => {
          const boundary = turnStart.current ?? current.length;
          turnStart.current = null;
          return finishConversationTurn(
            mergeAgentEvents(current, buffered.filter((event) => event.type !== "subagentEvent")),
            `stopped-${Date.now()}`, true, boundary
          );
        });
      }
      stopRequested.current = false;
      sendState.current = "idle";
      setWorking(false);
      setAgentStatus(sessionId, "idle");
    } catch (reason) {
      stopRequested.current = false;
      setMessage(`Could not stop Copilot: ${errorMessage(reason)}`);
    } finally {
      setStopping(false);
    }
  }

  const decide = useCallback(async (id: string, allow: boolean) => {
    setResponding(id);
    try {
      await api.respondCopilotPermission(sessionId, id, allow);
      setItems((current) => decidePermission(current, id, allow ? "submitted" : "denied"));
      setAgentStatus(sessionId, working ? "working" : "idle");
    } catch (reason) {
      setMessage(`Could not respond to permission request: ${errorMessage(reason)}`);
    } finally {
      setResponding(null);
    }
  }, [sessionId, setAgentStatus, working]);

  async function changePermissionMode(mode: PermissionMode, confirmed = false) {
    setModeBusy(true);
    try {
      const applied = await api.setCopilotPermissionMode(sessionId, mode, confirmed);
      setPermissionMode(sessionId, applied);
      if (applied === "ask") {
        reviewedMcpVersion.current++;
        setReviewedMcp((current) => ({ ...current, enabled: false }));
      }
      setConfirmAllowAll(false);
    } catch (reason) {
      setMessage(`Could not change permission mode: ${errorMessage(reason)}`);
    } finally {
      setModeBusy(false);
    }
  }

  async function changeAgentMode(enabled: boolean) {
    setModeBusy(true);
    try {
      const applied = await api.setCopilotAutopilot(sessionId, enabled);
      setAgentMode(applied);
      if (!enabled) {
        reviewedMcpVersion.current++;
        setReviewedMcp((current) => ({ ...current, enabled: false }));
      }
      setConfirmAutopilot(false);
    } catch (reason) {
      setMessage(`Could not change Copilot mode: ${errorMessage(reason)}`);
    } finally {
      setModeBusy(false);
    }
  }

  async function changeReviewedMcp(enabled: boolean) {
    setModeBusy(true);
    reviewedMcpVersion.current++;
    try {
      const applied = await api.setReviewedMcpApproval(sessionId, enabled, enabled);
      setReviewedMcp(applied);
      setReviewedMcpError(null);
      setConfirmReviewedMcp(false);
    } catch (reason) {
      setReviewedMcpError(`Could not change reviewed MCP approvals: ${errorMessage(reason)}`);
    } finally {
      setModeBusy(false);
    }
  }

  async function changeModel(modelId: string, effort: string | null, tier: ContextTier | null, label: string) {
    if (!modelId || modelBusy || status !== "connected" || working || sending || pendingChange) return;
    const eventVersion = modelEventVersion.current;
    setModelBusy(true);
    setModelError(null);
    try {
      const result = await api.setCopilotModel(sessionId, modelId, effort, tier);
      if (modelEventVersion.current === eventVersion) {
        if (result.currentModel) {
          setSelectedModel(result.currentModel);
          setContextTier(result.contextTier);
          setReasoningEffort(result.reasoningEffort);
        }
        pendingSettings.current = result.pending ? {
          modelId, effort, tier, queued: result.queued
        } : null;
        setPendingChange(result.pending
          ? result.queued ? `${label} queued; waiting for Copilot`
            : `${label} requested; awaiting Copilot confirmation`
          : null);
      }
      if (result.warning) setModelError(result.warning);
    } catch (reason) {
      setModelError(`Could not change model: ${errorMessage(reason)}`);
    } finally {
      setModelBusy(false);
    }
  }

  function keyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      void send();
    }
  }

  const recover = status === "sessionMissing" ? "replace" : status === "unattached" || autoCreate
    ? "create" : "resume";
  async function reconnect() {
    if (status === "paused") {
      try {
        await api.resumeChatTunnel(sessionId);
      } catch (reason) {
        setMessage(`Could not resume Chat tunnel: ${errorMessage(reason)}`);
        return;
      }
    }
    await connect(recover);
  }
  const activeModel = models.find((model) => model.id === selectedModel);
  const contextOptions: ContextTier[] = activeModel?.supportedContextTiers.length
    ? Array.from(new Set(["default" as const, ...activeModel.supportedContextTiers]))
    : ["default", "long_context"];
  const modelControlsDisabled = status !== "connected" || modelsLoading || modelBusy ||
    working || sending || pendingChange !== null;
  useEffect(() => {
    if (!active) {
      setSettingsOpen(false);
      setSelectedTraceId(null);
    }
  }, [active]);
  useEffect(() => {
    if (settingsOpen && status === "connected") void loadReviewedMcp(generation.current);
  }, [settingsOpen, status, loadReviewedMcp]);
  return (
    <>
    <section className="flex min-h-0 flex-1 flex-col bg-canvas" aria-label="Copilot chat">
      <div className="flex min-h-0 flex-1">
      <div className="flex min-w-0 flex-1 flex-col">
      {traceList.length > 0 && <div className="flex items-center justify-between border-b border-white/10 px-5 py-2 text-xs text-slate-400">
        <span>{traceList.length} subagent{traceList.length === 1 ? "" : "s"} · separate from main Chat</span>
        <button className="toolbar-button text-accent" type="button" aria-label="Inspect subagents"
          onClick={() => setSelectedTraceId(selectedTrace?.id ?? traceList[0].id)}>
          Inspect subagents
        </button>
      </div>}
      <div ref={viewport} tabIndex={0} className="chat-viewport min-h-0 flex-1 overflow-y-auto px-6 py-6"
        onKeyDown={(event) => {
          if (event.target !== event.currentTarget) return;
          if (["ArrowUp", "PageUp", "Home"].includes(event.key) || event.key === " " && event.shiftKey) {
            wheelDirection.current = "up";
            following.current = false;
            setStuckAboveBottom(true);
          } else if (["ArrowDown", "PageDown", "End"].includes(event.key) || event.key === " ") {
            wheelDirection.current = "down";
            if (event.currentTarget.scrollHeight - event.currentTarget.scrollTop -
              event.currentTarget.clientHeight <= 8) {
              wheelDirection.current = null;
              following.current = true;
              setStuckAboveBottom(false);
            }
          }
        }}
        onWheel={(event) => {
          wheelDirection.current = event.deltaY < 0 ? "up" : event.deltaY > 0 ? "down" : null;
          if (wheelDirection.current === "up" && event.currentTarget.scrollHeight > event.currentTarget.clientHeight) {
            if (followFrame.current !== null) cancelAnimationFrame(followFrame.current);
            followFrame.current = null;
            previousScrollTop.current = event.currentTarget.scrollTop;
            following.current = false;
            setStuckAboveBottom(true);
          } else if (wheelDirection.current === "down" &&
            event.currentTarget.scrollHeight - event.currentTarget.scrollTop - event.currentTarget.clientHeight <= 8) {
            wheelDirection.current = null;
            following.current = true;
            setStuckAboveBottom(false);
          }
        }}
        onPointerDown={() => { wheelDirection.current = null; dragging.current = true; }}
        onPointerUp={() => { dragging.current = false; }}
        onPointerCancel={() => { dragging.current = false; }}
        onScroll={(event) => {
          const element = event.currentTarget;
          const direction = wheelDirection.current;
          if (direction || dragging.current) {
            const nearBottom = element.scrollHeight - element.scrollTop - element.clientHeight <= 8;
            if (direction === "up" || dragging.current && element.scrollTop < previousScrollTop.current) {
              following.current = false;
            } else if (nearBottom) {
              following.current = true;
            } else if (dragging.current) {
              following.current = false;
            }
            setStuckAboveBottom(!following.current);
          }
          previousScrollTop.current = element.scrollTop;
          wheelDirection.current = null;
        }}>
        {rowCount === 0 && status === "connected" && (
          <p className="mx-auto max-w-3xl py-12 text-center text-sm text-slate-400">
            Ask Copilot about this remote repository.
          </p>
        )}
        <div className="relative mx-auto max-w-3xl" style={{ height: totalHeight }}>
          {virtualizer.getVirtualItems().map((row) => (
            <div key={row.key} data-index={row.index} ref={virtualizer.measureElement}
              className="absolute left-0 top-0 w-full"
              style={{ transform: `translateY(${row.start}px)` }}>
              <ConversationRow item={visibleItems[row.index]} responding={responding}
                onDecide={decide} onError={onError} onInspectSubagent={setSelectedTraceId} />
            </div>
          ))}
        </div>
      </div>
      {stuckAboveBottom && (
        <button className="self-center rounded-t bg-raised px-3 py-1 text-xs text-accent" type="button"
          aria-live="polite"
          onClick={() => {
            following.current = true;
            setStuckAboveBottom(false);
            followLatest();
          }}>
          {latestTurn?.kind === "turn"
            ? `${latestTurn.outcome === "stopped" ? "Copilot stopped" :
              latestTurn.outcome === "failed" ? "Copilot finished with a failed step" :
                latestTurn.hasReply ? "Copilot finished" : "Copilot finished without a reply"} · Jump to latest`
            : "Jump to latest"}
        </button>
      )}
      </div>
      {selectedTrace && <SubagentInspector trace={selectedTrace} traces={traceList}
        onSelect={setSelectedTraceId} onClose={() => setSelectedTraceId(null)}
        responding={responding} onDecide={decide} onError={onError} />}
      </div>
      <div className="border-t border-white/10 bg-panel/95 px-5 py-3 shadow-[0_-8px_32px_rgba(0,0,0,0.14)]">
        <div className="mx-auto max-w-3xl">
          {message && <p className="error-box mb-2" role="alert">{message}</p>}
          {working && <p className="mb-2 text-xs text-accent" role="status">
            Copilot is working{intent ? ` · ${intent}` : "..."}
            {activeTool && <span className="ml-2 text-slate-200">
              · {activeTool.name}{activeTool.command ? `: ${activeTool.command.slice(0, 90)}` : ""}
            </span>}
            {keepingAwake && <span className="ml-2 text-slate-400">· Keeping Mac awake</span>}
          </p>}
          {sleepWarning && <p className="mb-2 text-xs text-amber-200" role="alert">
            Could not prevent Mac idle sleep: {sleepWarning}
          </p>}
          <div className="mb-2 flex flex-wrap items-center gap-2 text-xs text-slate-400">
            <label className="shrink-0" htmlFor={`chat-model-${sessionId}`}>Model:</label>
            <select id={`chat-model-${sessionId}`} className="field min-w-0 max-w-[min(100%,24rem)] flex-1 text-xs"
              value={selectedModel ?? ""}
              disabled={modelControlsDisabled || models.length === 0}
              onChange={(event) => void changeModel(event.target.value, null, null, `Model ${event.target.value}`)}>
              {selectedModel === null && <option value="" disabled>
                {modelsLoading ? "Loading models..." : "Default (Copilot)"}
              </option>}
              {selectedModel !== null && !models.some((model) => model.id === selectedModel) &&
                <option value={selectedModel}>{selectedModel} (current)</option>}
              {models.map((model) => (
                <option key={model.id} value={model.id}>
                  {model.name === model.id ? model.id : `${model.name} · ${model.id}`}
                </option>
              ))}
            </select>
            {modelBusy && <span>Changing model...</span>}
            {pendingChange && <span role="status">{pendingChange}</span>}
            {pendingChange && <button type="button" className="toolbar-button"
              onClick={() => void refreshModel()}>Sync model</button>}
            {modelsError && status === "connected" && (
              <button type="button" className="toolbar-button" disabled={modelsLoading}
                onClick={() => void loadModels(generation.current)}>Retry models</button>
            )}
            {agentMode === "autopilot" && <span className="rounded-full bg-accent/10 px-2 py-1 text-accent">Autopilot</span>}
            <Popover.Root open={settingsOpen} onOpenChange={setSettingsOpen}>
              <Popover.Trigger asChild>
                <button type="button" className="secondary-button ml-auto flex items-center gap-2 px-3 py-1.5"
                  aria-label="Chat settings">
                  <SlidersHorizontal size={14} aria-hidden />
                  Settings
                  <ChevronDown size={12} aria-hidden />
                </button>
              </Popover.Trigger>
              <Popover.Portal>
                <Popover.Content side="top" align="end" sideOffset={10}
                  className="z-30 max-h-[min(70vh,34rem)] w-[min(92vw,28rem)] overflow-y-auto rounded-xl border border-white/15 bg-panel p-5 text-slate-200 shadow-2xl outline-none"
                  aria-label="Chat settings">
                  <h3 className="mb-1 text-sm font-semibold text-white">Conversation settings</h3>
                  <p className="mb-4 text-xs text-slate-400">Model controls and independent agent permissions.</p>
          <div className="mb-4 flex flex-wrap items-center gap-3 text-xs text-slate-400">
            <label htmlFor={`chat-context-${sessionId}`}>Context window:</label>
            <select id={`chat-context-${sessionId}`} className="field text-xs"
              value={contextTier ?? "default"}
              disabled={modelControlsDisabled || !selectedModel}
              onChange={(event) => {
                const tier = event.target.value;
                if (selectedModel && (tier === "default" || tier === "long_context")) {
                  void changeModel(selectedModel, reasoningEffort, tier, `Context window ${tier}`);
                } else setModelError(`Unsupported context tier: ${tier}`);
              }}>
              {contextTier !== null && !activeModel?.supportedContextTiers.includes(contextTier) &&
                !contextOptions.includes(contextTier) &&
                <option value={contextTier}>{contextTier} (current)</option>}
              {contextOptions.map((tier) => (
                <option key={tier} value={tier}>
                  {tier === "long_context" ? "Long context" : "Default"}
                  {!activeModel?.supportedContextTiers.length && tier === "long_context" ? " (if available)" : ""}
                </option>
              ))}
            </select>
            {activeModel?.maxContextWindowTokens != null &&
              <span title="Catalog maximum; Copilot does not report a separate token limit for each tier">
                Catalog max {activeModel.maxContextWindowTokens.toLocaleString()} tokens
              </span>}
            <label htmlFor={`chat-effort-${sessionId}`}>Reasoning effort:</label>
            <select id={`chat-effort-${sessionId}`} className="field text-xs"
              value={reasoningEffort ?? ""}
              disabled={modelControlsDisabled || !selectedModel || !activeModel?.supportedReasoningEfforts.length}
              onChange={(event) => {
                if (!selectedModel || !activeModel) return;
                const effort = event.target.value || activeModel.defaultReasoningEffort;
                if (effort && activeModel.supportedReasoningEfforts.includes(effort)) {
                  void changeModel(selectedModel, effort, contextTier, `Reasoning effort ${effort}`);
                } else setModelError(`Unsupported reasoning effort: ${event.target.value}`);
              }}>
              <option value="" disabled={!activeModel?.defaultReasoningEffort}>
                Model default{activeModel?.defaultReasoningEffort ? ` (${activeModel.defaultReasoningEffort})` : ""}
              </option>
              {reasoningEffort !== null && !activeModel?.supportedReasoningEfforts.includes(reasoningEffort) &&
                <option value={reasoningEffort}>{reasoningEffort} (current)</option>}
              {activeModel?.supportedReasoningEfforts.map((effort) => (
                <option key={effort} value={effort}>{effort.replaceAll("_", " ")}</option>
              ))}
            </select>
          </div>
          <div className="mb-4 border-t border-white/10 pt-4 text-xs text-slate-400">
            <p className="mb-2 font-medium text-slate-200">Permissions</p>
            <div className="flex flex-wrap items-center gap-2">
            <button type="button" className={`toolbar-button ${permissionMode === "ask" ? "text-accent" : ""}`}
              disabled={modeBusy || permissionMode === "ask"}
              onClick={() => void changePermissionMode("ask")}>Ask</button>
            <button type="button" className={`toolbar-button ${permissionMode === "allowAll" ? "text-amber-300" : ""}`}
              disabled={modeBusy || permissionMode === "allowAll" || status !== "connected"}
              onClick={() => { setSettingsOpen(false); setConfirmAllowAll(true); }}>Allow all...</button>
            {permissionMode === "allowAll" && <span role="status">This session · until app exit</span>}
            </div>
            <p className="mt-2">Allow all approves displayed Shell, Read, Write and URL requests for this session only. Reviewed MCP tools require a separate grant; managed policy never permits automatic approval.</p>
          </div>
          <div className="border-t border-white/10 pt-4 text-xs text-slate-400">
            <p className="mb-2 font-medium text-slate-200">Agent mode</p>
            <div className="flex flex-wrap items-center gap-2">
            <button type="button" className={`toolbar-button ${agentMode === "interactive" ? "text-accent" : ""}`}
              disabled={modeBusy || status !== "connected" || agentMode === "interactive" || agentMode === "unsupported"}
              onClick={() => void changeAgentMode(false)}>Interactive</button>
            <button type="button" className={`toolbar-button ${agentMode === "autopilot" ? "text-amber-300" : ""}`}
              disabled={modeBusy || status !== "connected" || agentMode === "autopilot" || agentMode === "unsupported"}
              onClick={() => { setSettingsOpen(false); setConfirmAutopilot(true); }}>Autopilot...</button>
            {agentMode === "autopilot" && <span role="status">Autopilot active · complete Read requests allowed; other permissions still apply</span>}
            {agentMode === "unsupported" && status === "connected" &&
              <span>Session mode unavailable on this Copilot headless runtime</span>}
            </div>
            <p className="mt-2">Autopilot continues multi-step work and permits complete unmanaged Reads. Shell, Write and URL still ask unless you separately enable Allow all.</p>
          </div>
          <div className="mt-4 border-t border-white/10 pt-4 text-xs text-slate-400">
            <p className="mb-2 font-medium text-slate-200">Reviewed MCP tool approvals</p>
            {reviewedMcpError && <p className="mb-2 text-amber-200" role="alert">{reviewedMcpError}</p>}
            <p className="mb-2 break-words">
              Available in this Chat: {reviewedMcp.available.length
                ? reviewedMcp.available.join(", ") : "none; review or activate a server in Integrations."}
            </p>
            {reviewedMcp.enabled ? (
              <button type="button" className="toolbar-button text-amber-200"
                disabled={modeBusy || status !== "connected"}
                onClick={() => void changeReviewedMcp(false)}>Disable reviewed MCP approvals</button>
            ) : (
              <button type="button" className="toolbar-button text-accent"
                disabled={modeBusy || status !== "connected" ||
                  agentMode !== "autopilot" || permissionMode !== "allowAll" ||
                  reviewedMcp.available.length === 0}
                onClick={() => { setSettingsOpen(false); setConfirmReviewedMcp(true); }}>
                Approve reviewed MCP automatically...
              </button>
            )}
            <p className="mt-2">
              Requires Autopilot, Allow all, and confirmation for this session. Only fully displayed
              calls to reviewed servers qualify; changed CLI configurations, managed policy,
              sandbox escalation, and missing or redacted arguments still ask.
            </p>
          </div>
          <div className="mt-4 border-t border-white/10 pt-4">
            <div className="flex flex-wrap gap-2">
              <button type="button" className="secondary-button text-xs"
                disabled={working || sending || switchBusy || status === "connecting"}
                onClick={() => { setSettingsOpen(false); void reconnect(); }}>
                Reconnect Copilot
              </button>
              <Popover.Close asChild>
                <button type="button" className="secondary-button text-xs"
                  disabled={working || sending || switchBusy || status === "connecting" || status === "paused"}
                  onClick={() => { setSettingsOpen(false); setRecentOpen(true); }}>
                  Browse recent CLI chats
                </button>
              </Popover.Close>
            </div>
            <p className="mt-2 text-xs text-slate-400">Reconnect after reviewing an existing MCP server to load its remote CLI configuration.</p>
          </div>
                </Popover.Content>
              </Popover.Portal>
            </Popover.Root>
          </div>
          {modelError && <p className="mb-2 text-xs text-amber-200" role="alert">{modelError}</p>}
          {modelsError && status === "connected" &&
            <p className="mb-2 text-xs text-amber-200" role="alert">{modelsError}</p>}
          {status !== "connected" && (
            <div className="mb-3 flex items-center justify-between gap-3 text-sm text-slate-300">
              <span>{status === "connecting" ? "Connecting to Copilot..." :
                status === "unattached" ? "No Copilot chat attached to this session." :
                status === "sessionMissing" ? "Copilot session could not be restored. The terminal is unaffected." :
                status === "paused" ? "Chat tunnel paused. Resume it here or from Ports to continue." :
                status === "hostOffline" ? "Chat SSH tunnel unavailable. The session is saved; the terminal may still work." :
                "Copilot unavailable. The terminal remains usable."}</span>
              {status !== "connecting" && (
                <div className="flex shrink-0 flex-wrap gap-2">
                  <button type="button" className="secondary-button"
                    onClick={() => void reconnect()}>
                    {status === "sessionMissing" ? "Start new Copilot conversation" :
                      status === "unattached" ? "Create Copilot chat" :
                        status === "paused" ? "Resume Chat tunnel" : "Reconnect Copilot"}
                  </button>
                  {status !== "paused" &&
                    <button type="button" className="secondary-button"
                      onClick={() => { setSettingsOpen(false); setRecentOpen(true); }}>Browse recent CLI chats</button>}
                </div>
              )}
            </div>
          )}
          <label className="sr-only" htmlFor={`chat-input-${sessionId}`}>Message to Copilot</label>
          <textarea id={`chat-input-${sessionId}`} className="field min-h-[76px] w-full resize-y rounded-xl text-sm"
            placeholder="Ask Copilot about this repository..."
            value={draft} onChange={(event) => setDraft(event.target.value)} onKeyDown={keyDown}
            disabled={status !== "connected"} />
          <div className="mt-2 flex items-center justify-between">
            <span className="text-xs text-slate-400">Cmd/Ctrl + Enter to send</span>
            {working
              ? <button type="button" className="secondary-button flex items-center gap-2" disabled={stopping}
                  onClick={() => void stop()}><Square size={13} aria-hidden />{stopping ? "Stopping..." : "Stop"}</button>
              : <button type="button" className="primary-button flex items-center gap-2" disabled={status !== "connected" || sending || !draft.trim()}
                  onClick={() => void send()}><Send size={14} aria-hidden />{sending ? "Sending..." : "Send"}</button>}
          </div>
        </div>
      </div>
    </section>
    {confirmAllowAll && (
      <div className="dialog-backdrop">
        <section className="dialog max-w-md" role="dialog" aria-modal="true" aria-labelledby={`allow-all-title-${sessionId}`}>
          <h2 id={`allow-all-title-${sessionId}`} className="text-lg font-semibold">Enable Allow all for this session?</h2>
          <p className="mt-3 text-sm text-slate-200">
            Shell commands, file writes, reads, and URLs with complete details will be approved automatically
            until this app exits or you switch back to Ask. Review the repository and follow your enterprise policy
            before enabling this mode. Requests already pending still need a manual choice.
          </p>
          <p className="mt-2 text-xs text-amber-200">
            Managed approvals, sandbox bypass, MCP integrations without a separate reviewed-server
            grant, hooks, custom tools, and actions with missing details are never auto-approved.
          </p>
          <div className="mt-5 flex justify-end gap-2">
            <button type="button" className="secondary-button" autoFocus disabled={modeBusy}
              onClick={() => setConfirmAllowAll(false)}>Cancel</button>
            <button type="button" className="secondary-button border-amber-500/40 text-amber-200" disabled={modeBusy}
              onClick={() => void changePermissionMode("allowAll", true)}>
              {modeBusy ? "Enabling..." : "Enable Allow all"}
            </button>
          </div>
        </section>
      </div>
    )}
    {confirmAutopilot && (
      <div className="dialog-backdrop">
        <section className="dialog max-w-md" role="dialog" aria-modal="true" aria-labelledby={`autopilot-title-${sessionId}`}>
          <h2 id={`autopilot-title-${sessionId}`} className="text-lg font-semibold">Enable Autopilot for this conversation?</h2>
          <p className="mt-3 text-sm text-slate-200">
            Copilot can pursue multi-step work in this remote repository. This uses the SDK session’s
            Autopilot mode; it does not automatically change your permission mode or set an AI-credit limit.
          </p>
          <p className="mt-2 text-xs text-amber-200">
            Autopilot permits complete Read requests unless managed policy requires review.
            With Ask, Shell/write/URL requests still need approval. With Allow all, other visible
            Shell/write/URL requests may be submitted automatically. New approvals cannot
            be handled while your Mac is asleep or the tunnel is disconnected. Stop ends an active generation.
          </p>
          <div className="mt-5 flex justify-end gap-2">
            <button type="button" className="secondary-button" autoFocus disabled={modeBusy}
              onClick={() => setConfirmAutopilot(false)}>Cancel</button>
            <button type="button" className="secondary-button border-amber-500/40 text-amber-200" disabled={modeBusy}
              onClick={() => void changeAgentMode(true)}>
              {modeBusy ? "Enabling..." : "Enable Autopilot"}
            </button>
          </div>
        </section>
      </div>
    )}
    {confirmReviewedMcp && (
      <div className="dialog-backdrop">
        <section className="dialog max-w-md" role="dialog" aria-modal="true"
          aria-labelledby={`reviewed-mcp-title-${sessionId}`}>
          <h2 id={`reviewed-mcp-title-${sessionId}`} className="text-lg font-semibold">
            Auto-approve reviewed MCP tools for this session?
          </h2>
          <p className="mt-3 break-words text-sm text-slate-200">
            Servers: {reviewedMcp.available.join(", ")}. This grant covers their complete, visible
            MCP tool requests while Autopilot and Allow all are active. It lasts until disabled or
            the app exits. Existing pending prompts still need a manual choice.
          </p>
          <p className="mt-2 text-xs text-amber-200">
            Tools can access or change remote data. Managed approvals, a server that disallows
            blanket grants, elevated sandbox requests, changed CLI configurations and incomplete
            or redacted arguments still require manual review. Only enable this after reviewing
            those servers and following your organization's policy.
          </p>
          {reviewedMcpError && <p className="mt-2 text-xs text-rose-200" role="alert">{reviewedMcpError}</p>}
          <div className="mt-5 flex justify-end gap-2">
            <button type="button" className="secondary-button" autoFocus disabled={modeBusy}
              onClick={() => setConfirmReviewedMcp(false)}>Cancel</button>
            <button type="button" className="secondary-button border-amber-500/40 text-amber-200"
              disabled={modeBusy} onClick={() => void changeReviewedMcp(true)}>
              {modeBusy ? "Enabling..." : "Enable reviewed MCP approvals"}
            </button>
          </div>
        </section>
      </div>
    )}
    {recentOpen && <RecentCliChatsDialog sessionId={sessionId}
      workspacePath={workspacePath ?? ""} onChoose={attachRecent}
      onClose={() => setRecentOpen(false)} />}
    </>
  );
}
