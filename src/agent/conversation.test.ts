import { describe, expect, it } from "vitest";
import type { AgentEvent } from "../types";
import { finishConversationTurn, groupConversation, mergeAgentEvent, mergeAgentEvents } from "./conversation";

describe("Copilot conversation reducer", () => {
  it("replaces streaming fragments with the final message without duplicating history", () => {
    const events: AgentEvent[] = [
      { type: "userMessage", eventId: "u1", messageId: "user-1", content: "Fix it" },
      { type: "assistantDelta", eventId: "d1", messageId: "assistant-1", content: "I am " },
      { type: "assistantDelta", eventId: "d2", messageId: "assistant-1", content: "checking" },
      { type: "assistantMessage", eventId: "a1", messageId: "assistant-1", content: "I am checking." }
    ];
    const items = events.reduce(mergeAgentEvent, []);
    expect(items).toHaveLength(2);
    expect(items[1]).toEqual({
      kind: "message", id: "assistant-1", role: "assistant",
      content: "I am checking.", complete: true
    });
    expect(mergeAgentEvent(items, events[3])).toEqual(items);
  });

  it("updates tool activity by call id instead of exposing SDK payloads", () => {
    const started: AgentEvent = {
      type: "toolStarted", eventId: "start", toolCallId: "tool-1",
      toolName: "Read", description: "src/auth.ts", command: null
    };
    const completed: AgentEvent = {
      type: "toolCompleted", eventId: "end", toolCallId: "tool-1",
      success: true, result: "42 tests passed"
    };
    const items = mergeAgentEvent(mergeAgentEvent([], started), completed);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "tool", name: "Read", success: true });
  });

  it("streams a bounded shell transcript into one active tool row", () => {
    const events: AgentEvent[] = [
      { type: "toolStarted", eventId: "start", toolCallId: "shell-1",
        toolName: "Shell", description: "Building", command: "npm run build" },
      { type: "toolOutput", eventId: "out-1", toolCallId: "shell-1", output: "First line\n" },
      { type: "toolOutput", eventId: "out-2", toolCallId: "shell-1", output: "x".repeat(17_000) }
    ];
    const items = mergeAgentEvents([], events);
    expect(items).toHaveLength(1);
    expect(items[0]).toMatchObject({ kind: "tool", name: "Shell", command: "npm run build", success: null });
    if (items[0].kind !== "tool") throw new Error("Shell row missing");
    expect(items[0].output.length).toBe(16_000);
    expect(items[0].output.endsWith("x".repeat(16_000))).toBe(true);
  });

  it("reconciles streamed thinking summaries, tool progress, and sub-agent steps", () => {
    const events: AgentEvent[] = [
      { type: "intent", eventId: "i1", content: "Inspecting code" },
      { type: "reasoningDelta", eventId: "d1", reasoningId: "r1", content: "Check " },
      { type: "reasoningDelta", eventId: "d2", reasoningId: "r1", content: "the tests" },
      { type: "reasoning", eventId: "r1", reasoningId: "r1", content: "Checked the tests." },
      { type: "toolStarted", eventId: "t1", toolCallId: "t1", toolName: "Search", description: "src", command: null },
      { type: "toolProgress", eventId: "p1", toolCallId: "t1", message: "Reading files" },
      { type: "toolCompleted", eventId: "t2", toolCallId: "t1", success: true, result: null },
      {
        type: "subagentStarted", eventId: "s1", agentId: "agent-review",
        toolCallId: "task-1", agentName: "review", model: "gpt-6-sol",
        displayName: "Code review", description: "Checking changes"
      },
      { type: "subagentCompleted", eventId: "s2", agentId: "agent-review",
        toolCallId: "task-1", agentName: "review", displayName: "Code review", cancelled: false }
    ];
    const items = mergeAgentEvents([], events);
    expect(items).toHaveLength(3);
    expect(items[0]).toEqual({
      kind: "reasoning", id: "r1", content: "Checked the tests.", complete: true
    });
    expect(items[1]).toMatchObject({
      kind: "tool", name: "Search", progress: null, success: true
    });
    expect(items[2]).toMatchObject({
      kind: "subagent", displayName: "Code review", status: "done"
    });
    expect(mergeAgentEvents(mergeAgentEvents([], events.slice(0, 4)), events.slice(4))).toEqual(items);
  });

  it("keeps model messages outside activity and marks completion without inventing a reply", () => {
    const events: AgentEvent[] = [
      { type: "userMessage", eventId: "user", messageId: "user", content: "Fix it" },
      { type: "reasoning", eventId: "thought", reasoningId: "thought", content: "Inspect the tests" },
      {
        type: "toolStarted", eventId: "start", toolCallId: "build", toolName: "Shell",
        description: "Building", command: "npm test"
      },
      { type: "toolCompleted", eventId: "done", toolCallId: "build",
        success: true, result: "42 tests passed" },
      { type: "assistantMessage", eventId: "answer", messageId: "answer",
        content: "**Fixed it.**" }
    ];
    const finished = finishConversationTurn(mergeAgentEvents([], events), "idle", false);
    const rows = groupConversation(finished);
    expect(rows.map((row) => row.kind)).toEqual(["message", "activity", "message", "turn"]);
    expect(rows[1]).toMatchObject({ kind: "activity", steps: [
      { kind: "reasoning" }, { kind: "tool", result: "42 tests passed" }
    ] });
    expect(rows[2]).toMatchObject({ kind: "message", role: "assistant", content: "**Fixed it.**" });
    expect(rows[3]).toMatchObject({ kind: "turn", outcome: "finished", hasReply: true });
    expect(finishConversationTurn(finished, "duplicate-idle", false)).toBe(finished);

    const withoutAnswer = finishConversationTurn(mergeAgentEvents([], events.slice(0, -1)), "idle", false);
    expect(groupConversation(withoutAnswer).at(-1)).toMatchObject({
      kind: "turn", outcome: "finished", hasReply: false
    });
    expect(finishConversationTurn(mergeAgentEvents([], events.slice(0, 1)), "abort", true).at(-1))
      .toMatchObject({ kind: "turn", outcome: "stopped" });
    const unfinishedTool = finishConversationTurn(mergeAgentEvents([], events.slice(0, 3)), "idle", false);
    expect(unfinishedTool[2]).toMatchObject({
      kind: "tool", success: null, concluded: true
    });
    const unfinishedReply = finishConversationTurn(mergeAgentEvents([], [
      { type: "assistantDelta", eventId: "stream", messageId: "partial", content: "Partial" }
    ]), "idle", false);
    expect(unfinishedReply[0]).toMatchObject({
      kind: "message", role: "assistant", content: "Partial", complete: false, incomplete: true
    });
    const older = mergeAgentEvents([], [
      { type: "userMessage", eventId: "old-user", messageId: "old-user", content: "Old prompt" },
      { type: "assistantMessage", eventId: "old-answer", messageId: "old-answer", content: "Old reply" },
      { type: "toolStarted", eventId: "new-tool", toolCallId: "new-tool",
        toolName: "Shell", command: "npm test", description: null }
    ]);
    expect(finishConversationTurn(older, "new-idle", false, 2).at(-1)).toMatchObject({
      kind: "turn", hasReply: false
    });
  });

  it("renders only accepted task-complete summaries as replies and reconciles later identical messages", () => {
    const accepted: AgentEvent = {
      type: "taskComplete", eventId: "completed", summary: "**Finished.**",
      outcome: "completed", success: true, reason: null, truncated: false
    };
    const prelude: AgentEvent[] = [
      { type: "userMessage", eventId: "question", messageId: "question", content: "Fix it" },
      { type: "toolStarted", eventId: "tool", toolCallId: "task",
        toolName: "task_complete", description: null, command: null }
    ];
    const items = mergeAgentEvents([], [...prelude, accepted]);
    expect(groupConversation(items).map((row) => row.kind)).toEqual(["message", "activity", "message"]);
    expect(items.at(-1)).toMatchObject({
      kind: "message", role: "assistant", source: "taskComplete", content: "**Finished.**"
    });
    const echoed = mergeAgentEvent(items, {
      type: "assistantMessage", eventId: "answer", messageId: "answer", content: "**Finished.**"
    });
    expect(echoed).toHaveLength(items.length);
    expect(echoed.at(-1)).toMatchObject({ kind: "message", id: "answer" });
    expect(echoed.at(-1)).not.toHaveProperty("source");
    expect(finishConversationTurn(echoed, "idle", false).at(-1))
      .toMatchObject({ kind: "turn", hasReply: true });
    const streamedEcho = mergeAgentEvents(items, [
      { type: "assistantDelta", eventId: "delta", messageId: "stream", content: "**Finished" },
      { type: "assistantMessage", eventId: "echo", messageId: "stream", content: "**Finished.**" }
    ]);
    expect(streamedEcho.filter((item) => item.kind === "message" && item.role === "assistant"))
      .toHaveLength(1);
    expect(streamedEcho.at(-1)).toMatchObject({ kind: "message", id: "stream" });
    const alreadyAnswered = mergeAgentEvents([], [...prelude, {
      type: "assistantMessage", eventId: "answer", messageId: "answer", content: "**Finished.**"
    }, accepted] as AgentEvent[]);
    expect(alreadyAnswered).toHaveLength(3);
    const late = mergeAgentEvent(finishConversationTurn(items, "idle", false), accepted);
    expect(late).toHaveLength(4);
    expect(late.at(-1)).toMatchObject({ kind: "turn", hasReply: true });

    for (const outcome of ["continue", "blocked"]) {
      const rejected = mergeAgentEvents([], [...prelude, {
        ...accepted, eventId: outcome, success: false, outcome
      }]);
      expect(rejected.some((item) => item.kind === "message" && item.role === "assistant")).toBe(false);
      expect(rejected.at(-1)).toMatchObject({ kind: "completion", outcome });
    }
    expect(mergeAgentEvent([], { ...accepted, truncated: true, eventId: "cut" }))
      .toMatchObject([{ kind: "completion", reason: expect.stringContaining("display limit") }]);
  });

  it("keeps structured tool inputs alongside results and groups child launches by call ID", () => {
    const items = mergeAgentEvents([], [
      { type: "toolStarted", eventId: "started", toolCallId: "read",
        toolName: "mcp.search", description: "Querying", command: null,
        arguments: [{ label: "query", value: "{\n  \"term\": \"oauth\"\n}" }],
        argumentsWarning: "Sensitive argument values were redacted." },
      { type: "toolCompleted", eventId: "completed", toolCallId: "read",
        success: false, result: "Query failed" },
      ...["first", "second"].flatMap((toolCallId) => [
        { type: "subagentStarted", eventId: `start-${toolCallId}`, agentId: toolCallId,
          toolCallId, agentName: "review", displayName: "Code review",
          description: null, model: null },
        { type: "subagentCompleted", eventId: `done-${toolCallId}`, agentId: toolCallId,
          toolCallId, agentName: "review", displayName: "Code review", cancelled: false }
      ] as AgentEvent[])
    ]);
    expect(items[0]).toMatchObject({
      kind: "tool", arguments: [{ label: "query", value: expect.stringContaining("oauth") }],
      argumentsWarning: expect.stringContaining("redacted"), result: "Query failed"
    });
    expect(items.slice(1)).toMatchObject([
      { kind: "subagent", id: "first", status: "done" },
      { kind: "subagent", id: "second", status: "done" }
    ]);
  });

  it("restores ten thousand events in one indexed pass", () => {
    const history: AgentEvent[] = Array.from({ length: 10_000 }, (_, index) => ({
      type: "userMessage", eventId: `event-${index}`, messageId: `message-${index}`,
      content: `Message ${index}`
    }));
    const items = mergeAgentEvents([], history);
    expect(items).toHaveLength(10_000);
    expect(items[0]).toMatchObject({ id: "message-0", content: "Message 0" });
    expect(items[9_999]).toMatchObject({ id: "message-9999", content: "Message 9999" });
    expect(mergeAgentEvents(items, [history[9_999]])).toBe(items);
  });

  it("bounds each expandable Activity group when a chat runs thousands of steps", () => {
    const events: AgentEvent[] = Array.from({ length: 2_000 }, (_, index) => ({
      type: "toolStarted", eventId: `event-${index}`, toolCallId: `tool-${index}`,
      toolName: "Read", description: `File ${index}`, command: null
    }));
    const items = mergeAgentEvents([], events);
    const rows = groupConversation(items);
    expect(rows).toHaveLength(250);
    expect(rows.every((row) => row.kind === "activity" && row.steps.length <= 8)).toBe(true);
    expect(rows[0]).toMatchObject({ kind: "activity", id: "tool:tool-0" });
    expect(rows.at(-1)).toMatchObject({ kind: "activity", id: "tool:tool-1992" });
  });
});
