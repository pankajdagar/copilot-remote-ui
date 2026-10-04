import { describe, expect, it } from "vitest";
import type { AgentEvent } from "../types";
import { mergeAgentEvents } from "./conversation";
import { mergeSubagentTraces } from "./subagents";

const launch = (id: string, agentId: string | null): AgentEvent => ({
  type: "subagentStarted", eventId: `launch-${id}`, agentId,
  toolCallId: id, agentName: "review", displayName: "Code review",
  description: "Checking files", model: "gpt-6-sol"
});
const child = (event: AgentEvent, agentId: string | null,
  parentToolCallId: string | null = null): AgentEvent => ({
  type: "subagentEvent", eventId: `wrapped-${event.eventId}`, agentId, parentToolCallId, event
});

describe("subagent trace routing", () => {
  it("keeps two same-named agents out of the main transcript across history and live events", () => {
    const events: AgentEvent[] = [
      launch("task-a", "agent-a"), launch("task-b", "agent-b"),
      child({ type: "assistantMessage", eventId: "a", messageId: "reply",
        content: "Agent A findings" }, "agent-a"),
      child({ type: "assistantMessage", eventId: "b", messageId: "reply",
        content: "Agent B findings" }, "agent-b"),
      child({ type: "toolStarted", eventId: "tool-a", toolCallId: "read-a",
        toolName: "Read", description: "src/a.ts", command: null }, null, "task-a"),
      { type: "subagentCompleted", eventId: "done-a", agentId: "agent-a",
        toolCallId: "task-a", agentName: "review", displayName: "Code review", cancelled: false }
    ];
    const traces = mergeSubagentTraces(
      mergeSubagentTraces({}, events.slice(0, 3)), events.slice(3)
    );
    expect(mergeSubagentTraces({}, events)).toEqual(traces);
    expect(Object.values(traces)).toHaveLength(2);
    expect(traces["task-a"]).toMatchObject({
      displayName: "Code review", status: "done", model: "gpt-6-sol",
      items: [
        { kind: "message", content: "Agent A findings" },
        { kind: "tool", name: "Read" }
      ]
    });
    expect(traces["task-b"]).toMatchObject({
      status: "working", items: [{ kind: "message", content: "Agent B findings" }]
    });
    expect(mergeAgentEvents([], events).filter((item) => item.kind === "message")).toHaveLength(0);
  });

  it("retains unidentified child activity separately and correlates it when a lifecycle arrives", () => {
    const first = child({ type: "assistantMessage", eventId: "orphan", messageId: "orphan",
      content: "Working on the task" }, "agent-late");
    const pending = mergeSubagentTraces({}, [first]);
    expect(Object.values(pending)).toMatchObject([{
      status: "unidentified", items: [{ kind: "message", content: "Working on the task" }]
    }]);
    const joined = mergeSubagentTraces(pending, [launch("task-late", "agent-late")]);
    expect(Object.values(joined)).toHaveLength(1);
    expect(Object.values(joined)[0]).toMatchObject({
      toolCallId: "task-late", status: "working", items: [{ kind: "message" }]
    });
    expect(mergeSubagentTraces({}, [child({
      type: "toolStarted", eventId: "legacy", toolCallId: "read", toolName: "Read",
      description: null, command: null
    }, null, "task-legacy")])["task-legacy"]).toMatchObject({
      status: "unidentified", items: [{ kind: "tool", id: "read" }]
    });
  });

  it("records a child turn end without ending the parent", () => {
    const traces = mergeSubagentTraces({}, [
      launch("task-a", "agent-a"),
      child({ type: "working", eventId: "w" }, "agent-a"),
      child({ type: "assistantMessage", eventId: "m", messageId: "m",
        content: "Investigated" }, "agent-a"),
      child({ type: "idle", eventId: "idle", aborted: false }, "agent-a")
    ]);
    expect(traces["task-a"].items.at(-1)).toMatchObject({
      kind: "turn", hasReply: true, outcome: "finished"
    });
  });

  it("does not attribute another agent's activity to a task merely because its parent ID matches", () => {
    const traces = mergeSubagentTraces({}, [
      launch("task-a", "agent-a"),
      child({ type: "assistantMessage", eventId: "other", messageId: "other",
        content: "Other agent findings" }, "agent-b", "task-a")
    ]);
    expect(traces["task-a"].items).toHaveLength(0);
    expect(traces["agent:agent-b"].items).toMatchObject([
      { kind: "message", content: "Other agent findings" }
    ]);
  });

  it("replays thousands of child messages into one trace without mixing them into the parent", () => {
    const events: AgentEvent[] = [
      launch("large-task", "large-agent"),
      ...Array.from({ length: 8_000 }, (_, index) => child({
        type: "assistantMessage", eventId: `event-${index}`,
        messageId: `message-${index}`, content: `Child message ${index}`
      }, "large-agent"))
    ];
    const traces = mergeSubagentTraces({}, events);
    expect(traces["large-task"].items).toHaveLength(8_000);
    expect(traces["large-task"].items.at(-1)).toMatchObject({
      kind: "message", content: "Child message 7999"
    });
    expect(mergeAgentEvents([], events)).toMatchObject([{
      kind: "subagent", id: "large-task"
    }]);
  });
});
