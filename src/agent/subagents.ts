import type { AgentEvent } from "../types";
import { finishConversationTurn, mergeAgentEvents, type ConversationItem } from "./conversation";

export interface SubagentTrace {
  id: string;
  toolCallId: string | null;
  agentId: string | null;
  displayName: string;
  description: string | null;
  model: string | null;
  status: "working" | "done" | "failed" | "cancelled" | "unidentified";
  error: string | null;
  items: ConversationItem[];
}

export type SubagentTraces = Record<string, SubagentTrace>;

function initialTrace(id: string, agentId: string | null, toolCallId: string | null): SubagentTrace {
  return {
    id, agentId, toolCallId, displayName: "Unidentified subagent",
    description: "Copilot did not provide enough information to link this trace to a named task.",
    model: null, status: "unidentified", error: null, items: []
  };
}

export function mergeSubagentTraces(traces: SubagentTraces, events: AgentEvent[]): SubagentTraces {
  if (!events.some((event) => event.type === "subagentEvent" ||
    event.type === "subagentStarted" || event.type === "subagentCompleted" ||
    event.type === "subagentFailed")) return traces;
  const next = { ...traces };
  const byAgent = new Map<string, string>();
  const byTool = new Map<string, string | null>();
  const queued = new Map<string, AgentEvent[]>();
  function indexTrace(trace: SubagentTrace) {
    if (trace.agentId) byAgent.set(trace.agentId, trace.id);
    if (trace.toolCallId) {
      const previous = byTool.get(trace.toolCallId);
      if (previous === undefined) byTool.set(trace.toolCallId, trace.id);
      else if (previous !== trace.id) byTool.set(trace.toolCallId, null);
    }
  }
  function matchTrace(agentId: string | null, toolCallId: string | null) {
    const exact = agentId ? byAgent.get(agentId) : undefined;
    if (exact) return next[exact];
    const candidate = toolCallId ? byTool.get(toolCallId) : undefined;
    const match = candidate ? next[candidate] : undefined;
    return match && (!agentId || !match.agentId) ? match : undefined;
  }
  function flush(id: string, idle?: Extract<AgentEvent, { type: "idle" }>) {
    const pending = queued.get(id);
    if (!pending?.length) return;
    const trace = next[id];
    const items = mergeAgentEvents(trace.items, pending);
    next[id] = {
      ...trace,
      items: idle ? finishConversationTurn(items, idle.eventId, idle.aborted) : items
    };
    queued.delete(id);
  }
  Object.values(traces).forEach(indexTrace);
  for (const event of events) {
    if (event.type === "subagentStarted" || event.type === "subagentCompleted" ||
      event.type === "subagentFailed") {
      const existing = matchTrace(event.agentId, event.toolCallId);
      const id = existing?.id ?? event.toolCallId;
      const trace = existing ?? initialTrace(id, event.agentId, event.toolCallId);
      next[id] = {
        ...trace,
        agentId: event.agentId ?? trace.agentId, toolCallId: event.toolCallId,
        displayName: event.displayName,
        description: event.type === "subagentStarted" ? event.description : trace.description,
        model: event.type === "subagentStarted" ? event.model : trace.model,
        status: event.type === "subagentStarted" ? "working" :
          event.type === "subagentFailed" ? "failed" :
            event.cancelled ? "cancelled" : "done",
        error: event.type === "subagentFailed" ? event.error : null
      };
      indexTrace(next[id]);
    } else if (event.type === "subagentEvent") {
      const existing = matchTrace(event.agentId, event.parentToolCallId);
      const id = existing?.id ??
        (event.parentToolCallId && !next[event.parentToolCallId] ? event.parentToolCallId :
          event.agentId ? `agent:${event.agentId}` : `unidentified:${event.eventId}`);
      const trace = existing ?? initialTrace(id, event.agentId, event.parentToolCallId);
      next[id] = {
        ...trace,
        agentId: event.agentId ?? trace.agentId,
        toolCallId: event.parentToolCallId ?? trace.toolCallId
      };
      indexTrace(next[id]);
      let pending = queued.get(id);
      if (!pending) {
        pending = [];
        queued.set(id, pending);
      }
      pending.push(event.event);
      if (event.event.type === "idle") flush(id, event.event);
    }
  }
  for (const id of queued.keys()) flush(id);
  return next;
}
