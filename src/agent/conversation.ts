import type { AgentEvent } from "../types";

export type ConversationItem =
  | {
      kind: "message"; id: string; role: "user" | "assistant";
      content: string; complete: boolean; incomplete?: boolean; source?: "taskComplete";
    }
  | { kind: "reasoning"; id: string; content: string; complete: boolean }
  | {
      kind: "tool"; id: string; name: string; description: string | null;
      command: string | null; arguments: Array<{ label: string; value: string }> | null;
      argumentsWarning: string | null; output: string; progress: string | null;
      success: boolean | null; concluded: boolean; result: string | null;
    }
  | {
      kind: "subagent"; id: string; name: string; displayName: string; description: string | null;
      agentId: string | null; model: string | null;
      status: "working" | "done" | "failed" | "cancelled"; error: string | null;
    }
  | {
      kind: "permission"; id: string; label: string; command: string | null;
      description: string | null; warning: string | null;
      details: Array<{ label: string; value: string }>; approvable: boolean;
      workingDirectory: string; decision: "pending" | "submitted" | "denied";
      source: "manual" | "autopilot" | "allowAll" | "reviewedMcp";
    }
  | { kind: "completion"; id: string; outcome: string; reason: string | null }
  | { kind: "error"; id: string; message: string }
  | {
      kind: "turn"; id: string; outcome: "finished" | "stopped" | "failed";
      hasReply: boolean;
    };

type ActivityItem = Extract<ConversationItem, { kind: "reasoning" | "tool" | "subagent" | "permission" }>;
export type ConversationDisplayItem =
  | ConversationItem
  | { kind: "activity"; id: string; steps: ActivityItem[] };

export function groupConversation(items: ConversationItem[]): ConversationDisplayItem[] {
  const rows: ConversationDisplayItem[] = [];
  let steps: ActivityItem[] = [];
  function flush() {
    if (steps.length) {
      rows.push({ kind: "activity", id: `${steps[0].kind}:${steps[0].id}`, steps });
      steps = [];
    }
  }
  for (const item of items) {
    if ((item.kind === "message" || item.kind === "reasoning") && !item.content.trim()) continue;
    if (item.kind === "reasoning" || item.kind === "tool" || item.kind === "subagent" ||
      item.kind === "permission" && item.source !== "manual") {
      if (steps.length === 8) flush();
      steps.push(item);
    } else {
      flush();
      rows.push(item);
    }
  }
  flush();
  return rows;
}

export function finishConversationTurn(
  items: ConversationItem[], id: string, aborted: boolean, startIndex = 0
): ConversationItem[] {
  const lastTurn = items.reduce((index, item, current) =>
    item.kind === "turn" ? current : index, -1);
  if (items.at(-1)?.kind === "turn" && startIndex <= lastTurn) return items;
  const boundary = Math.max(lastTurn + 1, Math.min(startIndex, items.length));
  const current = items.slice(boundary);
  const hasReply = current.some((item) => item.kind === "message" && item.role === "assistant" &&
    item.complete && item.content.trim().length > 0);
  const failed = current.some((item) => item.kind === "tool" && item.success === false ||
    item.kind === "subagent" && item.status === "failed");
  const concluded = items.map((item, index) => {
    if (index < boundary) return item;
    if (item.kind === "tool" && item.success === null) return { ...item, concluded: true };
    if (item.kind === "message" && item.role === "assistant" && !item.complete) {
      return { ...item, incomplete: true };
    }
    return item;
  });
  return [...concluded, {
    kind: "turn", id, outcome: aborted ? "stopped" : failed ? "failed" : "finished", hasReply
  }];
}

export function mergeAgentEvents(items: ConversationItem[], events: AgentEvent[]): ConversationItem[] {
  if (events.length === 0) return items;
  const next = [...items];
  const messages = new Map<string, number>();
  const reasoning = new Map<string, number>();
  const tools = new Map<string, number>();
  const subagents = new Map<string, number>();
  const permissions = new Map<string, number>();
  const errors = new Set<string>();
  const completedContent = new Set<string>();
  const summaries = new Map<string, number>();
  next.forEach((item, index) => {
    if (item.kind === "message") {
      messages.set(item.id, index);
      if (item.role === "user") {
        completedContent.clear();
        summaries.clear();
      } else if (item.complete) {
        completedContent.add(item.content.trim());
        if (item.source === "taskComplete") summaries.set(item.content.trim(), index);
      }
    }
    else if (item.kind === "reasoning") reasoning.set(item.id, index);
    else if (item.kind === "tool") tools.set(item.id, index);
    else if (item.kind === "subagent" && item.status === "working") subagents.set(item.id, index);
    else if (item.kind === "permission") permissions.set(item.id, index);
    else if (item.kind === "error") errors.add(item.id);
  });
  let changed = false;
  for (const event of events) {
    switch (event.type) {
      case "userMessage": {
        if (messages.has(event.messageId)) break;
        messages.set(event.messageId, next.length);
        next.push({ kind: "message", id: event.messageId, role: "user", content: event.content, complete: true });
        completedContent.clear();
        summaries.clear();
        changed = true;
        break;
      }
      case "assistantDelta": {
        const index = messages.get(event.messageId);
        if (index === undefined) {
          messages.set(event.messageId, next.length);
          next.push({ kind: "message", id: event.messageId, role: "assistant", content: event.content, complete: false });
        } else {
          const item = next[index];
          if (item.kind === "message" && !item.complete) next[index] = { ...item, content: item.content + event.content };
          else break;
        }
        changed = true;
        break;
      }
      case "assistantMessage": {
        const text = event.content.trim();
        const summaryIndex = summaries.get(text);
        if (summaryIndex !== undefined) {
          const summary = next[summaryIndex];
          if (summary.kind !== "message") break;
          const streamIndex = messages.get(event.messageId);
          if (streamIndex !== undefined && streamIndex !== summaryIndex) {
            next.splice(streamIndex, 1);
            for (const indexMap of [messages, reasoning, tools, subagents, permissions, summaries]) {
              for (const [id, index] of indexMap) {
                if (index > streamIndex) indexMap.set(id, index - 1);
              }
            }
          }
          const target = streamIndex !== undefined && streamIndex < summaryIndex
            ? summaryIndex - 1 : summaryIndex;
          messages.delete(summary.id);
          messages.set(event.messageId, target);
          next[target] = {
            kind: "message", id: event.messageId, role: "assistant",
            content: event.content, complete: true
          };
          summaries.delete(text);
          completedContent.add(text);
          changed = true;
          break;
        }
        const full: ConversationItem = {
          kind: "message", id: event.messageId, role: "assistant", content: event.content, complete: true
        };
        const index = messages.get(event.messageId);
        if (index === undefined) {
          messages.set(event.messageId, next.length);
          next.push(full);
        } else next[index] = full;
        completedContent.add(text);
        changed = true;
        break;
      }
      case "reasoningDelta": {
        const index = reasoning.get(event.reasoningId);
        if (index === undefined) {
          reasoning.set(event.reasoningId, next.length);
          next.push({ kind: "reasoning", id: event.reasoningId, content: event.content, complete: false });
        } else {
          const item = next[index];
          if (item.kind === "reasoning" && !item.complete) next[index] = { ...item, content: item.content + event.content };
          else break;
        }
        changed = true;
        break;
      }
      case "reasoning": {
        const full: ConversationItem = {
          kind: "reasoning", id: event.reasoningId, content: event.content, complete: true
        };
        const index = reasoning.get(event.reasoningId);
        if (index === undefined) {
          reasoning.set(event.reasoningId, next.length);
          next.push(full);
        } else next[index] = full;
        changed = true;
        break;
      }
      case "toolStarted": {
        const existing = tools.get(event.toolCallId);
        if (existing !== undefined) {
          const item = next[existing];
          if (item.kind === "tool" && item.name === "Tool") {
            next[existing] = {
              ...item, name: event.toolName, description: event.description, command: event.command,
              arguments: event.arguments ?? null, argumentsWarning: event.argumentsWarning ?? null
            };
            changed = true;
          }
          break;
        }
        tools.set(event.toolCallId, next.length);
        next.push({
          kind: "tool", id: event.toolCallId, name: event.toolName,
          description: event.description, command: event.command,
          arguments: event.arguments ?? null, argumentsWarning: event.argumentsWarning ?? null, output: "",
          progress: null, success: null, concluded: false, result: null
        });
        changed = true;
        break;
      }
      case "toolOutput": {
        const index = tools.get(event.toolCallId);
        if (index === undefined) {
          tools.set(event.toolCallId, next.length);
          next.push({
            kind: "tool", id: event.toolCallId, name: "Tool",
            description: null, command: null, arguments: null, argumentsWarning: null,
            output: event.output.slice(-16_000),
            progress: null, success: null, concluded: false, result: null
          });
        } else {
          const item = next[index];
          if (item.kind !== "tool" || item.success !== null) break;
          next[index] = { ...item, output: (item.output + event.output).slice(-16_000) };
        }
        changed = true;
        break;
      }
      case "toolProgress": {
        const index = tools.get(event.toolCallId);
        if (index === undefined) {
          tools.set(event.toolCallId, next.length);
          next.push({
            kind: "tool", id: event.toolCallId, name: "Tool", description: null,
            command: null, arguments: null, argumentsWarning: null, output: "",
            progress: event.message, success: null, concluded: false, result: null
          });
        } else {
          const item = next[index];
          if (item.kind !== "tool" || item.success !== null) break;
          next[index] = { ...item, progress: event.message };
        }
        changed = true;
        break;
      }
      case "toolCompleted": {
        const index = tools.get(event.toolCallId);
        if (index === undefined) {
          tools.set(event.toolCallId, next.length);
          next.push({
            kind: "tool", id: event.toolCallId, name: "Tool",
            description: null, command: null, arguments: null, argumentsWarning: null, output: "",
            progress: null, success: event.success, concluded: true, result: event.result
          });
        } else {
          const item = next[index];
          if (item.kind === "tool") next[index] = {
            ...item, progress: null, success: event.success, concluded: true, result: event.result
          };
        }
        changed = true;
        break;
      }
      case "subagentStarted": {
        const key = event.toolCallId;
        if (subagents.has(key)) break;
        subagents.set(key, next.length);
        next.push({
          kind: "subagent", id: key, name: event.agentName,
          displayName: event.displayName, description: event.description,
          agentId: event.agentId ?? null, model: event.model ?? null, status: "working", error: null
        });
        changed = true;
        break;
      }
      case "subagentCompleted":
      case "subagentFailed": {
        const key = event.toolCallId;
        const index = subagents.get(key);
        subagents.delete(key);
        const status = event.type === "subagentFailed" ? "failed" :
          event.cancelled ? "cancelled" : "done";
        const error = event.type === "subagentFailed" ? event.error : null;
        if (index === undefined) {
          next.push({
            kind: "subagent", id: key, name: event.agentName,
            displayName: event.displayName, description: null,
            agentId: event.agentId ?? null, model: null, status, error
          });
        } else {
          const item = next[index];
          if (item.kind === "subagent") next[index] = { ...item, status, error };
        }
        changed = true;
        break;
      }
      case "taskComplete": {
        const accepted = event.outcome === "completed" && event.success !== false ||
          event.outcome === null && event.success === true;
        if (accepted && event.summary?.trim() && !event.truncated) {
          const text = event.summary.trim();
          if (completedContent.has(text)) break;
          const insertAt = next.at(-1)?.kind === "turn" ? next.length - 1 : next.length;
          const summaryId = `task-complete:${event.eventId}`;
          next.splice(insertAt, 0, {
            kind: "message", id: summaryId, role: "assistant",
            content: event.summary, complete: true, source: "taskComplete"
          });
          messages.set(summaryId, insertAt);
          summaries.set(text, insertAt);
          completedContent.add(text);
          if (insertAt < next.length - 1) {
            const turn = next.at(-1);
            if (turn?.kind === "turn") next[next.length - 1] = { ...turn, hasReply: true };
          }
          changed = true;
        } else {
          const reason = event.truncated
            ? "Completion summary exceeded the display limit; inspect it in Copilot CLI."
            : event.reason ?? (accepted ? "Copilot provided no completion summary." : null);
          const outcome = event.outcome ?? (event.success === false ? "rejected" : "unknown");
          if (next.some((item) => item.kind === "completion" && item.id === event.eventId)) break;
          next.push({ kind: "completion", id: event.eventId, outcome, reason });
          changed = true;
        }
        break;
      }
      case "permissionRequested": {
        if (permissions.has(event.requestId)) break;
        permissions.set(event.requestId, next.length);
        next.push({
          kind: "permission", id: event.requestId, label: event.kind,
          command: event.command, description: event.description, warning: event.warning,
          details: event.details, approvable: event.approvable,
          workingDirectory: event.workingDirectory, decision: "pending", source: "manual"
        });
        changed = true;
        break;
      }
      case "permissionAutoApproved": {
        if (permissions.has(event.requestId)) break;
        permissions.set(event.requestId, next.length);
        next.push({
          kind: "permission", id: event.requestId, label: event.kind,
          command: event.command, description: null, warning: null, details: [], approvable: false,
          workingDirectory: event.workingDirectory, decision: "submitted", source: event.source
        });
        changed = true;
        break;
      }
      case "error":
      case "disconnected": {
        if (errors.has(event.eventId)) break;
        errors.add(event.eventId);
        next.push({ kind: "error", id: event.eventId, message: event.message });
        changed = true;
        break;
      }
      case "working":
      case "idle":
      case "sleepStatus":
      case "intent":
      case "modelChanged":
      case "subagentEvent":
        break;
    }
  }
  return changed ? next : items;
}

export function mergeAgentEvent(items: ConversationItem[], event: AgentEvent): ConversationItem[] {
  return mergeAgentEvents(items, [event]);
}

export function decidePermission(
  items: ConversationItem[], requestId: string, decision: "submitted" | "denied"
): ConversationItem[] {
  return items.map((item) => item.kind === "permission" && item.id === requestId
    ? { ...item, decision } : item);
}
