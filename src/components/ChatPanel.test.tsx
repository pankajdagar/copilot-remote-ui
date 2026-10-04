// @vitest-environment jsdom
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import { api } from "../api";
import { useUi } from "../store";
import type { AgentEvent, AgentSnapshot, CopilotModel, PermissionMode } from "../types";
import { ChatPanel } from "./ChatPanel";

const channels = vi.hoisted(() => [] as Array<{ onmessage?: (event: AgentEvent) => void }>);
const catalog = vi.hoisted(() => [
  {
    id: "gpt-5.4", name: "GPT 5.4",
    supportedContextTiers: ["default"], maxContextWindowTokens: 128_000,
    supportedReasoningEfforts: ["low", "medium", "high"], defaultReasoningEffort: "medium"
  },
  {
    id: "claude-sonnet-5", name: "Claude Sonnet 5",
    supportedContextTiers: ["default", "long_context"], maxContextWindowTokens: 200_000,
    supportedReasoningEfforts: ["low", "high"], defaultReasoningEffort: "high"
  },
  {
    id: "gpt-6-sol", name: "GPT-6 Sol",
    supportedContextTiers: ["default", "long_context"], maxContextWindowTokens: 250_000,
    supportedReasoningEfforts: ["low", "medium", "high", "xhigh"], defaultReasoningEffort: "medium"
  }
] as CopilotModel[]);
vi.mock("@tauri-apps/api/core", () => ({
  Channel: class {
    onmessage?: (event: AgentEvent) => void;
    constructor() { channels.push(this); }
  }
}));
vi.mock("../api", () => ({
  errorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
  api: {
    connectChat: vi.fn(), createChat: vi.fn(), replaceChat: vi.fn(),
    listRecentCliChats: vi.fn(async () => [
      {
        id: "cli-recent", summary: "Existing CLI conversation",
        startedAt: "2026-09-25T00:00:00Z", modifiedAt: "2026-09-28T01:00:00Z",
        sameWorkspace: true, isRemote: false, linkedTo: null
      }
    ]),
    attachCliChat: vi.fn(),
    disconnectChat: vi.fn(async () => {}), sendChatMessage: vi.fn(async () => {}),
    abortChat: vi.fn(async () => {}), chatHealth: vi.fn(async () => true),
    resumeChatTunnel: vi.fn(async () => {}),
    listCopilotModels: vi.fn(async () => catalog),
    getCopilotModelState: vi.fn(async () => ({
      currentModel: "gpt-5.4", contextTier: "default", reasoningEffort: "medium",
      pending: false, queued: false, warning: null
    })),
    setCopilotModel: vi.fn(async (_sessionId: string, modelId: string,
      effort: string | null, tier: string | null) =>
      ({ currentModel: modelId, contextTier: tier ?? "default",
        reasoningEffort: effort ?? "medium", pending: false, queued: false, warning: null })),
    respondCopilotPermission: vi.fn(async () => {}),
    setCopilotPermissionMode: vi.fn(async (_sessionId: string, mode: PermissionMode) => mode),
    reviewedMcpApprovalState: vi.fn(async () => ({ available: ["example-tools"], enabled: false })),
    setReviewedMcpApproval: vi.fn(async (_sessionId: string, enabled: boolean) =>
      ({ available: ["example-tools"], enabled })),
    setCopilotAutopilot: vi.fn(async (_sessionId: string, enabled: boolean) =>
      enabled ? "autopilot" : "interactive")
  }
}));

const connected: AgentSnapshot = {
  status: "connected", copilotSessionId: "sdk-session-1",
  mode: "interactive", modeError: null, mcpWarnings: [],
  model: "gpt-5.4", contextTier: "default",
  reasoningEffort: "medium", modelError: null, events: [], error: null
};

function openSettings() {
  if (!screen.queryByRole("dialog", { name: "Chat settings" })) {
    fireEvent.click(screen.getByRole("button", { name: "Chat settings" }));
  }
}

describe("Copilot chat", () => {
  let heightDescriptor: PropertyDescriptor | undefined;
  let widthDescriptor: PropertyDescriptor | undefined;
  let scrollToDescriptor: PropertyDescriptor | undefined;
  beforeAll(() => {
    heightDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetHeight");
    widthDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "offsetWidth");
    scrollToDescriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "scrollTo");
    Object.defineProperty(HTMLElement.prototype, "offsetHeight", {
      configurable: true,
      get(this: HTMLElement) {
        return this.classList.contains("chat-viewport") || this.classList.contains("subagent-viewport") ? 560
          : this.hasAttribute("data-index") ? 160 : heightDescriptor?.get?.call(this) ?? 0;
      }
    });
    Object.defineProperty(HTMLElement.prototype, "offsetWidth", {
      configurable: true,
      get(this: HTMLElement) {
        return this.classList.contains("chat-viewport") || this.classList.contains("subagent-viewport") ? 800
          : this.hasAttribute("data-index") ? 760 : widthDescriptor?.get?.call(this) ?? 0;
      }
    });
    Object.defineProperty(HTMLElement.prototype, "scrollTo", {
      configurable: true,
      value(this: HTMLElement, options: ScrollToOptions | number) {
        this.scrollTop = typeof options === "number" ? options : options.top ?? this.scrollTop;
        this.dispatchEvent(new Event("scroll", { bubbles: true }));
      }
    });
    const original = HTMLElement.prototype.getBoundingClientRect;
    vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
      if (this.classList.contains("chat-viewport") || this.classList.contains("subagent-viewport")) {
        return { x: 0, y: 0, top: 0, left: 0, right: 800, bottom: 560, width: 800, height: 560, toJSON: () => ({}) };
      }
      if (this.hasAttribute("data-index")) {
        return { x: 0, y: 0, top: 0, left: 0, right: 760, bottom: 160, width: 760, height: 160, toJSON: () => ({}) };
      }
      return original.call(this);
    });
  });
  afterAll(() => {
    vi.restoreAllMocks();
    if (heightDescriptor) Object.defineProperty(HTMLElement.prototype, "offsetHeight", heightDescriptor);
    if (widthDescriptor) Object.defineProperty(HTMLElement.prototype, "offsetWidth", widthDescriptor);
    if (scrollToDescriptor) Object.defineProperty(HTMLElement.prototype, "scrollTo", scrollToDescriptor);
    else Reflect.deleteProperty(HTMLElement.prototype, "scrollTo");
  });

  beforeEach(() => {
    channels.length = 0;
    useUi.setState({ agentStatuses: {}, permissionModes: {}, tabs: {} });
    vi.mocked(api.connectChat).mockReset().mockResolvedValue(connected);
    vi.mocked(api.createChat).mockReset().mockResolvedValue(connected);
    vi.mocked(api.replaceChat).mockReset().mockResolvedValue(connected);
    vi.mocked(api.listRecentCliChats).mockReset().mockResolvedValue([{
      id: "cli-recent", summary: "Existing CLI conversation",
      startedAt: "2026-09-25T00:00:00Z", modifiedAt: "2026-09-28T01:00:00Z",
      sameWorkspace: true, isRemote: false, linkedTo: null
    }]);
    vi.mocked(api.attachCliChat).mockReset().mockResolvedValue({
      ...connected, copilotSessionId: "cli-recent", events: [
        { type: "assistantMessage", eventId: "cli-answer", messageId: "cli-answer", content: "From Copilot CLI." }
      ]
    });
    vi.mocked(api.respondCopilotPermission).mockClear();
    vi.mocked(api.chatHealth).mockReset().mockResolvedValue(true);
    vi.mocked(api.sendChatMessage).mockClear();
    vi.mocked(api.abortChat).mockClear();
    vi.mocked(api.resumeChatTunnel).mockClear();
    vi.mocked(api.listCopilotModels).mockReset().mockResolvedValue(catalog);
    vi.mocked(api.getCopilotModelState).mockReset().mockResolvedValue({
      currentModel: "gpt-5.4", contextTier: "default", reasoningEffort: "medium",
      pending: false, queued: false, warning: null
    });
    vi.mocked(api.setCopilotModel).mockReset().mockImplementation(async (_sessionId, modelId, effort, tier) =>
      ({ currentModel: modelId, contextTier: tier ?? "default",
        reasoningEffort: effort ?? "medium", pending: false, queued: false, warning: null }));
    vi.mocked(api.setCopilotPermissionMode).mockClear();
    vi.mocked(api.reviewedMcpApprovalState).mockReset().mockResolvedValue({
      available: ["example-tools"], enabled: false
    });
    vi.mocked(api.setReviewedMcpApproval).mockReset()
      .mockImplementation(async (_sessionId, enabled) => ({ available: ["example-tools"], enabled }));
    vi.mocked(api.setCopilotAutopilot).mockClear();
  });
  afterEach(cleanup);

  it("restores history and streams assistant messages and activity", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, events: [
        { type: "userMessage", eventId: "u1", messageId: "u1", content: "Fix it" },
        { type: "assistantMessage", eventId: "a1", messageId: "a1", content: "Checking." }
      ]
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    expect(await screen.findByText("Checking.")).toBeTruthy();
    act(() => channels[0].onmessage?.({
      type: "assistantDelta", eventId: "d1", messageId: "a2", content: "Reading files"
    }));
    await waitFor(() => expect(screen.getByText("Reading files")).toBeTruthy());
    act(() => channels[0].onmessage?.({
      type: "assistantMessage", eventId: "a2", messageId: "a2", content: "Read the files."
    }));
    expect(screen.queryByText("Reading files")).toBeNull();
    expect(screen.getByText("Read the files.")).toBeTruthy();
    act(() => channels[0].onmessage?.({
      type: "toolStarted", eventId: "t1", toolCallId: "tool-1", toolName: "Read",
      description: "src/auth.ts", command: null
    }));
    expect(screen.getAllByText(/Read · src\/auth.ts/).length).toBeGreaterThan(0);
  });

  it("shows the active Shell command and streamed output before it completes", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    act(() => channels[0].onmessage?.({
      type: "toolStarted", eventId: "shell-start", toolCallId: "shell-1",
      toolName: "Shell", description: "Building UI", command: "npm run build"
    }));
    expect(screen.getByText(/Running · Shell · Building UI/)).toBeTruthy();
    expect(screen.getByText("npm run build")).toBeTruthy();
    act(() => channels[0].onmessage?.({ type: "working", eventId: "shell-work" }));
    expect(screen.getByRole("status").textContent).toContain("Shell: npm run build");
    act(() => channels[0].onmessage?.({
      type: "toolOutput", eventId: "shell-output", toolCallId: "shell-1", output: "Compiled 52 modules"
    }));
    expect(await screen.findByText("Compiled 52 modules")).toBeTruthy();
    act(() => channels[0].onmessage?.({
      type: "toolCompleted", eventId: "shell-done", toolCallId: "shell-1",
      success: true, result: "Build completed"
    }));
    expect(screen.getByText(/Done · Shell · Building UI/)).toBeTruthy();
  });

  it("keeps the final assistant answer in Chat, outside the expandable activity", async () => {
    const { container } = render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    act(() => {
      channels[0].onmessage?.({ type: "working", eventId: "turn-1" });
      channels[0].onmessage?.({
        type: "toolStarted", eventId: "tool-start", toolCallId: "tool",
        toolName: "Shell", description: "Building UI", command: "npm test"
      });
    });
    const activity = container.querySelector(".chat-viewport details");
    if (!(activity instanceof HTMLDetailsElement)) throw new Error("Activity group is missing");
    expect(activity.open).toBe(true);
    act(() => {
      channels[0].onmessage?.({
        type: "toolCompleted", eventId: "tool-done", toolCallId: "tool",
        success: true, result: "42 tests passed"
      });
      channels[0].onmessage?.({
        type: "assistantMessage", eventId: "final-answer", messageId: "final-answer",
        content: "**I fixed the UI and verified it.**"
      });
      channels[0].onmessage?.({ type: "idle", eventId: "turn-finished", aborted: false });
    });
    const answer = await screen.findByText("I fixed the UI and verified it.");
    expect(answer.closest("details")).toBeNull();
    expect(answer.closest("article")?.textContent).toContain("Copilot");
    expect(screen.getByText("Copilot finished this turn.").closest("details")).toBeNull();
    expect(activity.open).toBe(true);
    fireEvent.click(activity.querySelector("summary") as HTMLElement);
    expect(activity.open).toBe(false);
    act(() => channels[0].onmessage?.({
      type: "userMessage", eventId: "next-prompt", messageId: "next-prompt",
      content: "What changed?"
    }));
    expect(activity.open).toBe(false);
    expect(screen.getByText("I fixed the UI and verified it.")).toBeTruthy();
  });

  it("opens a separate virtualized subagent trace without replacing or finishing the parent chat", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, events: [
        { type: "userMessage", eventId: "root-question", messageId: "root-question",
          content: "Review the change" },
        { type: "subagentStarted", eventId: "start-a", agentId: "agent-a",
          toolCallId: "task-a", agentName: "review", displayName: "Code review",
          description: "Inspecting files", model: "gpt-6-sol" },
        { type: "subagentStarted", eventId: "start-b", agentId: "agent-b",
          toolCallId: "task-b", agentName: "review", displayName: "Code review",
          description: "Checking tests", model: null },
        { type: "subagentEvent", eventId: "child-a", agentId: "agent-a", parentToolCallId: null,
          event: { type: "assistantMessage", eventId: "answer-a", messageId: "answer-a",
            content: "Found a bug in file A" } },
        { type: "subagentEvent", eventId: "child-b", agentId: "agent-b", parentToolCallId: null,
          event: { type: "assistantMessage", eventId: "answer-b", messageId: "answer-b",
            content: "Tests are green" } },
        { type: "assistantMessage", eventId: "root-answer", messageId: "root-answer",
          content: "I am reviewing the results." }
      ]
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    expect(await screen.findByText("I am reviewing the results.")).toBeTruthy();
    expect(screen.queryByText("Found a bug in file A")).toBeNull();
    fireEvent.click(screen.getAllByRole("button", { name: "Inspect subagent trace" })[0]);
    const aside = screen.getByRole("complementary", { name: "Subagent trace" });
    expect(aside.textContent).toContain("Found a bug in file A");
    expect(aside.textContent).not.toContain("Tests are green");
    expect(screen.getByText("I am reviewing the results.")).toBeTruthy();
    fireEvent.change(screen.getByRole("combobox", { name: "Inspect a subagent" }),
      { target: { value: "task-b" } });
    expect(aside.textContent).toContain("Tests are green");
    expect(aside.textContent).not.toContain("Found a bug in file A");
    act(() => {
      channels[0].onmessage?.({ type: "working", eventId: "parent-working" });
      channels[0].onmessage?.({
        type: "subagentEvent", eventId: "child-idle", agentId: "agent-b",
        parentToolCallId: null,
        event: { type: "idle", eventId: "child-idle-inner", aborted: false }
      });
    });
    expect(screen.getByRole("button", { name: "Stop" })).toBeTruthy();
    expect(aside.textContent).toContain("Copilot finished this turn.");
    fireEvent.click(screen.getByRole("button", { name: "Close subagent trace" }));
    expect(screen.queryByRole("complementary", { name: "Subagent trace" })).toBeNull();
    expect(screen.getByText("I am reviewing the results.")).toBeTruthy();
  });

  it("mounts only nearby rows in a long subagent trace", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, events: [
        { type: "subagentStarted", eventId: "launch", agentId: "long-agent",
          toolCallId: "long-task", agentName: "research", displayName: "Research",
          description: null, model: null },
        ...Array.from({ length: 2_000 }, (_, index): AgentEvent => ({
          type: "subagentEvent", eventId: `wrapped-${index}`, agentId: "long-agent",
          parentToolCallId: null, event: {
            type: "assistantMessage", eventId: `msg-${index}`,
            messageId: `msg-${index}`, content: `Child answer ${index}`
          }
        }))
      ]
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    const inspect = await screen.findByRole("button", { name: "Inspect subagents" });
    fireEvent.click(inspect);
    const aside = screen.getByRole("complementary", { name: "Subagent trace" });
    expect(aside.querySelectorAll("article").length).toBeLessThan(20);
    expect(aside.textContent).not.toContain("Child answer 1999");
  });

  it("shows an accepted task completion as a final chat bubble, but not a rejected one", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    act(() => {
      channels[0].onmessage?.({ type: "working", eventId: "working" });
      channels[0].onmessage?.({
        type: "taskComplete", eventId: "not-yet", summary: "Incomplete",
        outcome: "continue", success: false, reason: "More checks needed", truncated: false
      });
      channels[0].onmessage?.({
        type: "taskComplete", eventId: "accepted", summary: "**All checks passed.**",
        outcome: "completed", success: true, reason: null, truncated: false
      });
      channels[0].onmessage?.({
        type: "assistantMessage", eventId: "echo", messageId: "echo",
        content: "**All checks passed.**"
      });
      channels[0].onmessage?.({ type: "idle", eventId: "idle", aborted: false });
    });
    expect(screen.getAllByText("All checks passed.")).toHaveLength(1);
    expect(screen.getByText("All checks passed.").closest("details")).toBeNull();
    expect(screen.getByText(/completion was not accepted/)).toBeTruthy();
    expect(screen.getByText("Copilot finished this turn.")).toBeTruthy();
    expect(screen.queryByText("Incomplete")).toBeNull();
  });

  it("makes a missing final reply visible instead of calling tool output a Copilot answer", async () => {
    const { container } = render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    act(() => {
      channels[0].onmessage?.({ type: "working", eventId: "work" });
      channels[0].onmessage?.({
        type: "toolStarted", eventId: "tool-start", toolCallId: "tool",
        toolName: "Shell", description: "Running tests", command: "npm test"
      });
      channels[0].onmessage?.({
        type: "toolCompleted", eventId: "tool-done", toolCallId: "tool",
        success: true, result: "42 tests passed"
      });
      channels[0].onmessage?.({ type: "idle", eventId: "turn-finished", aborted: false });
    });
    expect(await screen.findByText(/Copilot finished without a final reply/)).toBeTruthy();
    expect(container.querySelector(".chat-viewport article")).toBeNull();
    const toolResult = screen.getByText("42 tests passed");
    expect(toolResult.closest("details")).not.toBeNull();
  });

  it("does not count an old restored answer as the reply to a new task", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, events: [
        { type: "userMessage", eventId: "old-user", messageId: "old-user", content: "Old prompt" },
        { type: "assistantMessage", eventId: "old-answer", messageId: "old-answer",
          content: "Previous response." }
      ]
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    expect(await screen.findByText("Previous response.")).toBeTruthy();
    act(() => {
      channels[0].onmessage?.({ type: "working", eventId: "new-work" });
      channels[0].onmessage?.({
        type: "toolStarted", eventId: "tool-start", toolCallId: "tool",
        toolName: "Shell", description: "Running tests", command: "npm test"
      });
      channels[0].onmessage?.({
        type: "toolCompleted", eventId: "tool-done", toolCallId: "tool",
        success: true, result: "42 passed"
      });
      channels[0].onmessage?.({ type: "idle", eventId: "new-idle", aborted: false });
    });
    expect(screen.getByText(/Copilot finished without a final reply/)).toBeTruthy();
    expect(screen.queryByText("Copilot finished this turn.")).toBeNull();
    expect(screen.getByText("Previous response.")).toBeTruthy();
  });

  it("does not show a tool as still running after a turn ends without a tool completion event", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    act(() => {
      channels[0].onmessage?.({ type: "working", eventId: "work" });
      channels[0].onmessage?.({
        type: "toolStarted", eventId: "tool-start", toolCallId: "tool",
        toolName: "Shell", description: "Running tests", command: "npm test"
      });
      channels[0].onmessage?.({ type: "idle", eventId: "idle", aborted: false });
    });
    expect(await screen.findByText(/Copilot finished without a final reply/)).toBeTruthy();
    expect(screen.getByText(/No completion event · Shell/)).toBeTruthy();
    expect(screen.queryByText(/Running · Shell/)).toBeNull();
  });

  it("labels streamed text as incomplete if a turn ends without a final assistant message", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    act(() => {
      channels[0].onmessage?.({ type: "working", eventId: "work" });
      channels[0].onmessage?.({
        type: "assistantDelta", eventId: "partial", messageId: "answer", content: "Partial result"
      });
      channels[0].onmessage?.({ type: "idle", eventId: "idle", aborted: false });
    });
    expect(await screen.findByText("Partial result")).toBeTruthy();
    expect(screen.getByText("Copilot · incomplete")).toBeTruthy();
    expect(screen.getByText(/Copilot finished without a final reply/)).toBeTruthy();
  });

  it("hides empty messages and leaves a measured gap between visible turns", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, events: [
        { type: "userMessage", eventId: "u0", messageId: "empty-user", content: "" },
        { type: "userMessage", eventId: "u1", messageId: "real-user", content: "What changed?" },
        { type: "assistantDelta", eventId: "a0", messageId: "empty-answer", content: " " },
        { type: "assistantMessage", eventId: "a1", messageId: "empty-answer", content: " \n " },
        { type: "assistantMessage", eventId: "a2", messageId: "real-answer", content: "Two files." }
      ]
    });
    const { container } = render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    expect(await screen.findByText("Two files.")).toBeTruthy();
    expect(screen.getAllByText("You")).toHaveLength(1);
    expect(screen.getAllByText("Copilot")).toHaveLength(1);
    expect(container.querySelectorAll("[data-index]")).toHaveLength(2);
    expect((container.querySelector('[data-index="1"]') as HTMLElement).style.transform)
      .toBe("translateY(176px)");
  });

  it("does not show a streaming assistant card until nonblank text arrives", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    act(() => channels[0].onmessage?.({
      type: "assistantDelta", eventId: "leading-whitespace",
      messageId: "streaming", content: "  "
    }));
    expect(screen.queryByText("Copilot")).toBeNull();
    act(() => channels[0].onmessage?.({
      type: "assistantDelta", eventId: "answer-text",
      messageId: "streaming", content: "Done"
    }));
    await screen.findByText("Done");
    expect(screen.getAllByText("Copilot · responding")).toHaveLength(1);
    act(() => channels[0].onmessage?.({
      type: "assistantMessage", eventId: "empty-final",
      messageId: "streaming", content: " "
    }));
    expect(screen.queryByText("Copilot")).toBeNull();
  });

  it("coalesces a burst of streamed Markdown deltas without losing text", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    const input = await screen.findByRole("textbox", { name: "Message to Copilot" }) as HTMLTextAreaElement;
    await waitFor(() => expect(input.disabled).toBe(false));
    act(() => {
      for (let index = 0; index < 30; index++) {
        channels[0].onmessage?.({
          type: "assistantDelta", eventId: `delta-${index}`, messageId: "answer", content: "a"
        });
      }
    });
    await waitFor(() => expect(screen.getByText("a".repeat(30))).toBeTruthy());
    act(() => channels[0].onmessage?.({
      type: "assistantMessage", eventId: "complete", messageId: "answer", content: "Done."
    }));
    expect(screen.queryByText("a".repeat(30))).toBeNull();
    expect(screen.getByText("Done.")).toBeTruthy();
  });

  it("shows current intent and collapsible thinking summaries without replaying old status", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, events: [
        { type: "reasoning", eventId: "r1", reasoningId: "r1", content: "Check **the tests**." },
        {
          type: "subagentCompleted", eventId: "s1", agentId: null, toolCallId: "task-1",
          agentName: "review", displayName: "Code review", cancelled: false
        }
      ]
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    expect(await screen.findByText(/Thinking summary/)).toBeTruthy();
    expect(screen.getByText("Done · Code review")).toBeTruthy();
    act(() => channels[0].onmessage?.({ type: "working", eventId: "w1" }));
    act(() => channels[0].onmessage?.({
      type: "intent", eventId: "i1", content: "Inspecting the repository"
    }));
    expect(screen.getByRole("status").textContent).toContain("Inspecting the repository");
    act(() => channels[0].onmessage?.({
      type: "toolStarted", eventId: "t1", toolCallId: "t1", toolName: "Read",
      description: "src/auth.ts", command: null
    }));
    act(() => channels[0].onmessage?.({
      type: "toolProgress", eventId: "p1", toolCallId: "t1", message: "Reading tests"
    }));
    expect(screen.getByText("Reading tests")).toBeTruthy();
    act(() => channels[0].onmessage?.({ type: "idle", eventId: "done", aborted: false }));
    expect(screen.queryByText(/Inspecting the repository/)).toBeNull();
  });

  it("shows when the Mac is kept awake during work and exposes inhibition failures", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    act(() => {
      channels[0].onmessage?.({ type: "working", eventId: "work-1" });
      channels[0].onmessage?.({ type: "sleepStatus", eventId: "awake-1", active: true, error: null });
    });
    expect(screen.getByRole("status").textContent).toContain("Keeping Mac awake");
    act(() => channels[0].onmessage?.({
      type: "sleepStatus", eventId: "failed-1", active: false, error: "caffeinate unavailable"
    }));
    expect(screen.getByText(/Could not prevent Mac idle sleep: caffeinate unavailable/)).toBeTruthy();
    act(() => channels[0].onmessage?.({
      type: "sleepStatus", eventId: "awake-2", active: false, error: null
    }));
    expect(screen.queryByText(/Could not prevent Mac idle sleep/)).toBeNull();
  });

  it("leaves old terminal-only sessions unattached until chat is requested", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      status: "unattached", copilotSessionId: null, mode: "unsupported",
      modeError: null, mcpWarnings: [], model: null, contextTier: null, reasoningEffort: null,
      modelError: null, events: [], error: null
    });
    render(<ChatPanel sessionId="old-session" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    fireEvent.click(await screen.findByRole("button", { name: "Create Copilot chat" }));
    await waitFor(() => expect(api.createChat).toHaveBeenCalledWith("old-session", expect.anything()));
    expect(api.connectChat).toHaveBeenCalledTimes(1);
  });

  it("does not silently replace a missing stored Copilot conversation", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      status: "sessionMissing", copilotSessionId: "saved-sdk-id",
      mode: "unsupported", modeError: null, mcpWarnings: [], model: null,
      contextTier: null, reasoningEffort: null, modelError: null,
      events: [], error: "Stored conversation could not be restored"
    });
    vi.mocked(api.replaceChat).mockResolvedValue(connected);
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    expect(await screen.findByText(/Stored conversation could not be restored/)).toBeTruthy();
    expect(api.createChat).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Start new Copilot conversation" }));
    await waitFor(() => expect(api.replaceChat).toHaveBeenCalledWith("app-1", expect.anything()));
  });

  it("offers recent CLI chats when the stored conversation cannot be restored", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, status: "sessionMissing", copilotSessionId: "lost-id",
      error: "The stored conversation is missing", events: []
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} workspacePath="/remote/repo" />);
    fireEvent.click(await screen.findByRole("button", { name: "Browse recent CLI chats" }));
    expect(await screen.findByText("Existing CLI conversation")).toBeTruthy();
    fireEvent.click(screen.getByRole("radio", { name: /Existing CLI conversation/ }));
    fireEvent.click(screen.getByRole("button", { name: "Attach selected chat" }));
    await waitFor(() => expect(api.attachCliChat).toHaveBeenCalledWith(
      "app-1", "cli-recent", false, expect.anything()
    ));
    expect(await screen.findByText("From Copilot CLI.")).toBeTruthy();
    expect(api.replaceChat).not.toHaveBeenCalled();
  });

  it("keeps an active Chat and its stream after an unsuccessful CLI attach", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, events: [
        { type: "assistantMessage", eventId: "old-answer", messageId: "old-answer", content: "Old chat remains." }
      ]
    });
    vi.mocked(api.attachCliChat).mockRejectedValueOnce(new Error("Selected CLI session could not be resumed"));
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} workspacePath="/remote/repo" />);
    expect(await screen.findByText("Old chat remains.")).toBeTruthy();
    openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Browse recent CLI chats" }));
    fireEvent.click(await screen.findByRole("radio", { name: /Existing CLI conversation/ }));
    fireEvent.click(screen.getByRole("button", { name: "Attach selected chat" }));
    expect(await screen.findByText(/Selected CLI session could not be resumed/)).toBeTruthy();
    expect(screen.getByText("Old chat remains.")).toBeTruthy();
    act(() => channels[0].onmessage?.({
      type: "assistantMessage", eventId: "later", messageId: "later", content: "Old stream still works."
    }));
    expect(screen.getByText("Old stream still works.")).toBeTruthy();
  });

  it("replaces only the explicit Chat link on successful CLI selection", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, events: [
        { type: "assistantMessage", eventId: "old-answer", messageId: "old-answer", content: "Previous answer." }
      ]
    });
    useUi.getState().setPermissionMode("app-1", "allowAll");
    const onLinked = vi.fn(async () => {});
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={onLinked} onError={vi.fn()} workspacePath="/remote/repo" />);
    expect(await screen.findByText("Previous answer.")).toBeTruthy();
    openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Browse recent CLI chats" }));
    fireEvent.click(await screen.findByRole("radio", { name: /Existing CLI conversation/ }));
    fireEvent.click(screen.getByRole("button", { name: "Attach selected chat" }));
    expect(await screen.findByText("From Copilot CLI.")).toBeTruthy();
    expect(screen.queryByText("Previous answer.")).toBeNull();
    expect(useUi.getState().permissionModes["app-1"]).toBe("ask");
    expect(api.connectChat).toHaveBeenCalledTimes(1);
    expect(api.replaceChat).not.toHaveBeenCalled();
    expect(onLinked).toHaveBeenCalledWith("app-1");
    act(() => channels[0].onmessage?.({
      type: "assistantMessage", eventId: "stale", messageId: "stale", content: "Stale old channel."
    }));
    expect(screen.queryByText("Stale old channel.")).toBeNull();
  });

  it("does not reconnect a deliberately paused host until resumed", async () => {
    vi.mocked(api.connectChat)
      .mockResolvedValueOnce({ ...connected, status: "paused", error: "Chat tunnel paused" })
      .mockResolvedValueOnce(connected);
    vi.useFakeTimers();
    try {
      render(<ChatPanel sessionId="app-1" active autoCreate={false}
        onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
      await act(async () => { await Promise.resolve(); });
      await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
      expect(api.connectChat).toHaveBeenCalledTimes(1);
      fireEvent.click(screen.getByRole("button", { name: "Resume Chat tunnel" }));
      await act(async () => { await Promise.resolve(); });
      expect(api.resumeChatTunnel).toHaveBeenCalledWith("app-1");
      expect(api.connectChat).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("surfaces tunnel diagnostics without implying that terminal SSH is unavailable", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, status: "hostOffline", model: null, events: [],
      error: "SSH tunnel to test-host failed: SSH exited with exit status: 255: " +
        "channel open failed: administratively prohibited"
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    expect(await screen.findByText(/administratively prohibited/)).toBeTruthy();
    expect(screen.getByText(/Chat SSH tunnel unavailable.*terminal may still work/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Reconnect Copilot" })).toBeTruthy();
  });

  it("requires explicit approval and submits using Cmd/Ctrl+Enter", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    const permission: AgentEvent = {
      type: "permissionRequested", eventId: "p1", requestId: "req-1",
      kind: "Shell", command: "npm test", workingDirectory: "/home/coder/repo",
      description: "Run tests", warning: null, details: [], approvable: true
    };
    act(() => channels[0].onmessage?.(permission));
    expect(screen.getByText("npm test")).toBeTruthy();
    expect(useUi.getState().agentStatuses["app-1"]).toBe("awaitingPermission");
    fireEvent.click(screen.getByRole("button", { name: "Allow once" }));
    await waitFor(() => expect(api.respondCopilotPermission).toHaveBeenCalledWith("app-1", "req-1", true));

    const input = screen.getByRole("textbox", { name: "Message to Copilot" });
    fireEvent.change(input, { target: { value: "Line one\nLine two" } });
    fireEvent.keyDown(input, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(api.sendChatMessage).toHaveBeenCalledWith("app-1", "Line one\nLine two"));
    act(() => channels[0].onmessage?.({ type: "working", eventId: "work-1" }));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() => expect(api.abortChat).toHaveBeenCalledWith("app-1"));
  });

  it("does not resurrect an old working state when restoring a completed conversation", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, events: [
        { type: "working", eventId: "old-turn" },
        { type: "assistantMessage", eventId: "old-answer", messageId: "old-answer", content: "Finished." }
      ]
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByText("Finished.");
    expect(screen.getByRole("button", { name: "Send" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  });

  it("does not get stuck on Stop when idle arrives before send acknowledgement", async () => {
    let acknowledge!: () => void;
    vi.mocked(api.sendChatMessage).mockReturnValueOnce(new Promise<void>((resolve) => {
      acknowledge = resolve;
    }));
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    const input = await screen.findByRole("textbox", { name: "Message to Copilot" });
    await waitFor(() => expect((input as HTMLTextAreaElement).disabled).toBe(false));
    fireEvent.change(input, { target: { value: "Quick task" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));
    act(() => {
      channels[0].onmessage?.({ type: "working", eventId: "turn-start" });
      channels[0].onmessage?.({ type: "idle", eventId: "turn-done", aborted: false });
    });
    await act(async () => { acknowledge(); });
    expect(screen.getByRole("button", { name: "Send" })).toBeTruthy();
    expect(screen.getByText(/Copilot finished without a final reply/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Stop" })).toBeNull();
  });

  it("shows the actual selected model on restore and switches only after SDK confirmation", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    const select = await screen.findByRole("combobox", { name: "Model:" }) as HTMLSelectElement;
    await waitFor(() => expect(select.disabled).toBe(false));
    expect(select.value).toBe("gpt-5.4");
    expect(select.options[select.selectedIndex].text).toContain("GPT 5.4");
    let confirm!: (result: {
      currentModel: string; contextTier: "default"; reasoningEffort: "high";
      pending: false; queued: false; warning: null
    }) => void;
    vi.mocked(api.setCopilotModel).mockReturnValueOnce(new Promise((resolve) => { confirm = resolve; }));
    fireEvent.change(select, { target: { value: "claude-sonnet-5" } });
    expect(api.setCopilotModel).toHaveBeenCalledWith("app-1", "claude-sonnet-5", null, null);
    expect(select.value).toBe("gpt-5.4");
    await act(async () => { confirm({
      currentModel: "claude-sonnet-5", contextTier: "default",
      reasoningEffort: "high", pending: false, queued: false, warning: null
    }); });
    expect(select.value).toBe("claude-sonnet-5");
    act(() => channels[0].onmessage?.({
      type: "modelChanged", eventId: "automatic-switch", modelId: "gpt-5.4",
      contextTier: "default", reasoningEffort: "medium"
    }));
    expect(select.value).toBe("gpt-5.4");
  });

  it("switches to GPT-6 Sol and applies its reported context and reasoning options", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    const model = await screen.findByRole("combobox", { name: "Model:" }) as HTMLSelectElement;
    openSettings();
    const context = screen.getByRole("combobox", { name: "Context window:" }) as HTMLSelectElement;
    const effort = screen.getByRole("combobox", { name: "Reasoning effort:" }) as HTMLSelectElement;
    await waitFor(() => expect(model.disabled).toBe(false));
    fireEvent.change(model, { target: { value: "gpt-6-sol" } });
    await waitFor(() => expect(model.value).toBe("gpt-6-sol"));
    expect(api.setCopilotModel).toHaveBeenCalledWith("app-1", "gpt-6-sol", null, null);
    expect(screen.getByTitle(/Catalog maximum/).textContent?.replace(/\D/g, "")).toBe("250000");
    expect(Array.from(context.options).map((option) => option.value)).toContain("long_context");
    expect(Array.from(effort.options).map((option) => option.value)).toContain("xhigh");
    fireEvent.change(context, { target: { value: "long_context" } });
    await waitFor(() => expect(context.value).toBe("long_context"));
    expect(api.setCopilotModel).toHaveBeenLastCalledWith(
      "app-1", "gpt-6-sol", "medium", "long_context"
    );
    fireEvent.change(effort, { target: { value: "xhigh" } });
    await waitFor(() => expect(effort.value).toBe("xhigh"));
    expect(api.setCopilotModel).toHaveBeenLastCalledWith(
      "app-1", "gpt-6-sol", "xhigh", "long_context"
    );
    expect(screen.queryByText(/restricted by host policy/)).toBeNull();
  });

  it("offers server-validated context tiers when the catalog omits its tier list", async () => {
    vi.mocked(api.listCopilotModels).mockResolvedValueOnce(
      catalog.map((model) => model.id === "gpt-5.4"
        ? { ...model, supportedContextTiers: [] } : model)
    );
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    openSettings();
    const context = await screen.findByRole("combobox", { name: "Context window:" }) as HTMLSelectElement;
    await waitFor(() => expect(context.disabled).toBe(false));
    expect(Array.from(context.options).map((option) => option.text)).toContain("Long context (if available)");
    vi.mocked(api.setCopilotModel).mockRejectedValueOnce(new Error("Long context unavailable"));
    fireEvent.change(context, { target: { value: "long_context" } });
    expect(await screen.findByText(/Could not change model: Long context unavailable/)).toBeTruthy();
    expect(context.value).toBe("default");
  });

  it("keeps the active model on switch failure and reports model-list errors", async () => {
    vi.mocked(api.listCopilotModels).mockRejectedValueOnce(new Error("Model list unavailable"));
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    expect(await screen.findByText(/Could not load models: Model list unavailable/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry models" }));
    const select = await screen.findByRole("combobox", { name: "Model:" }) as HTMLSelectElement;
    await waitFor(() => expect(select.disabled).toBe(false));
    vi.mocked(api.setCopilotModel).mockRejectedValueOnce(new Error("Model restricted"));
    fireEvent.change(select, { target: { value: "claude-sonnet-5" } });
    expect(await screen.findByText(/Could not change model: Model restricted/)).toBeTruthy();
    expect(select.value).toBe("gpt-5.4");
  });

  it("shows a deferred model switch as pending until the runtime reports its activation", async () => {
    vi.mocked(api.setCopilotModel).mockResolvedValueOnce({
      currentModel: "gpt-5.4", contextTier: "default",
      reasoningEffort: "medium", pending: true, queued: true, warning: null
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    const select = await screen.findByRole("combobox", { name: "Model:" }) as HTMLSelectElement;
    await waitFor(() => expect(select.disabled).toBe(false));
    fireEvent.change(select, { target: { value: "claude-sonnet-5" } });
    expect(await screen.findByText("Model claude-sonnet-5 queued; waiting for Copilot")).toBeTruthy();
    expect(select.value).toBe("gpt-5.4");
    act(() => channels[0].onmessage?.({
      type: "modelChanged", eventId: "deferred-change", modelId: "claude-sonnet-5",
      contextTier: "default", reasoningEffort: "high"
    }));
    expect(screen.queryByText(/queued; waiting for Copilot/)).toBeNull();
    expect(select.value).toBe("claude-sonnet-5");
    act(() => channels[0].onmessage?.({ type: "working", eventId: "model-turn" }));
    expect(select.disabled).toBe(true);
  });

  it("reconciles a queued change when its event arrives before the switch reply", async () => {
    let respond!: (result: {
      currentModel: string; contextTier: "default"; reasoningEffort: "medium";
      pending: true; queued: true; warning: null;
    }) => void;
    vi.mocked(api.setCopilotModel).mockReturnValueOnce(new Promise((resolve) => { respond = resolve; }));
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    const select = await screen.findByRole("combobox", { name: "Model:" }) as HTMLSelectElement;
    await waitFor(() => expect(select.disabled).toBe(false));
    openSettings();
    fireEvent.change(select, { target: { value: "gpt-6-sol" } });
    act(() => channels[0].onmessage?.({
      type: "modelChanged", eventId: "switched", modelId: "gpt-6-sol",
      contextTier: "default", reasoningEffort: "medium"
    }));
    await act(async () => { respond({
      currentModel: "gpt-5.4", contextTier: "default", reasoningEffort: "medium",
      pending: true, queued: true, warning: null
    }); });
    expect(select.value).toBe("gpt-6-sol");
    expect(screen.queryByRole("button", { name: "Sync model" })).toBeNull();
  });

  it("can sync a queued switch after an event was missed", async () => {
    vi.mocked(api.setCopilotModel).mockResolvedValueOnce({
      currentModel: "gpt-5.4", contextTier: "default",
      reasoningEffort: "medium", pending: true, queued: true, warning: null
    });
    vi.mocked(api.getCopilotModelState).mockResolvedValueOnce({
      currentModel: "gpt-6-sol", contextTier: "long_context",
      reasoningEffort: "xhigh", pending: false, queued: false, warning: null
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    const select = await screen.findByRole("combobox", { name: "Model:" }) as HTMLSelectElement;
    await waitFor(() => expect(select.disabled).toBe(false));
    fireEvent.change(select, { target: { value: "gpt-6-sol" } });
    fireEvent.click(await screen.findByRole("button", { name: "Sync model" }));
    await waitFor(() => expect(select.value).toBe("gpt-6-sol"));
    expect(screen.queryByRole("button", { name: "Sync model" })).toBeNull();
    openSettings();
    expect((screen.getByRole("combobox", { name: "Context window:" }) as HTMLSelectElement).value)
      .toBe("long_context");
    expect((screen.getByRole("combobox", { name: "Reasoning effort:" }) as HTMLSelectElement).value)
      .toBe("xhigh");
  });

  it("reads full settings when a model-change event omits context and effort", async () => {
    vi.mocked(api.getCopilotModelState).mockResolvedValueOnce({
      currentModel: "gpt-6-sol", contextTier: "long_context",
      reasoningEffort: "xhigh", pending: false, queued: false, warning: null
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await waitFor(() => expect(
      (screen.getByRole("combobox", { name: "Model:" }) as HTMLSelectElement).disabled
    ).toBe(false));
    openSettings();
    act(() => channels[0].onmessage?.({
      type: "modelChanged", eventId: "incomplete-model-change", modelId: "gpt-6-sol",
      contextTier: null, reasoningEffort: null
    }));
    await waitFor(() => expect(api.getCopilotModelState).toHaveBeenCalledWith("app-1"));
    await waitFor(() => expect(
      (screen.getByRole("combobox", { name: "Context window:" }) as HTMLSelectElement).value
    ).toBe("long_context"));
    expect((screen.getByRole("combobox", { name: "Reasoning effort:" }) as HTMLSelectElement).value).toBe("xhigh");
  });

  it("does not claim an unconfirmed model switch succeeded or blame host policy", async () => {
    vi.mocked(api.setCopilotModel).mockResolvedValueOnce({
      currentModel: null, contextTier: null, reasoningEffort: null,
      pending: true, queued: false, warning: "Copilot has not confirmed the new settings"
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    const select = await screen.findByRole("combobox", { name: "Model:" }) as HTMLSelectElement;
    await waitFor(() => expect(select.disabled).toBe(false));
    fireEvent.change(select, { target: { value: "gpt-6-sol" } });
    expect(await screen.findByText(/Model gpt-6-sol requested; awaiting Copilot confirmation/)).toBeTruthy();
    expect(select.value).toBe("gpt-5.4");
    fireEvent.click(screen.getByRole("button", { name: "Sync model" }));
    expect(await screen.findByText(/Copilot has not confirmed the requested model settings/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Sync model" })).toBeNull();
    expect(select.value).toBe("gpt-5.4");
    expect(select.disabled).toBe(false);
  });

  it("leaves Stopping after abort acknowledgement and allows retry on failure", async () => {
    let acknowledge!: () => void;
    vi.mocked(api.abortChat).mockReturnValueOnce(new Promise<void>((resolve) => {
      acknowledge = resolve;
    }));
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    act(() => channels[0].onmessage?.({ type: "working", eventId: "turn-start" }));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect((screen.getByRole("button", { name: "Stopping..." }) as HTMLButtonElement).disabled).toBe(true);
    await act(async () => { acknowledge(); });
    expect(screen.getByRole("button", { name: "Send" })).toBeTruthy();
    expect(screen.getByText("Copilot stopped this turn.")).toBeTruthy();

    act(() => channels[0].onmessage?.({ type: "working", eventId: "next-turn" }));
    vi.mocked(api.abortChat).mockRejectedValueOnce(new Error("Abort failed"));
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(await screen.findByText(/Could not stop Copilot: Abort failed/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "Stop" }) as HTMLButtonElement).disabled).toBe(false);
  });

  it("does not force-scroll while the user reads earlier messages", async () => {
    const { container } = render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    const viewport = container.querySelector(".overflow-y-auto");
    if (!(viewport instanceof HTMLDivElement)) throw new Error("Chat viewport is missing");
    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 1000 });
    Object.defineProperty(viewport, "clientHeight", { configurable: true, value: 200 });
    fireEvent.wheel(viewport, { deltaY: -50 });
    viewport.scrollTop = 100;
    fireEvent.scroll(viewport);
    act(() => channels[0].onmessage?.({
      type: "assistantDelta", eventId: "d-scroll", messageId: "m-scroll", content: "New output"
    }));
    expect(viewport.scrollTop).toBe(100);
    fireEvent.click(screen.getByRole("button", { name: "Jump to latest" }));
    expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();
  });

  it("announces completion when the user is reading above the latest turn", async () => {
    const { container } = render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    const viewport = container.querySelector(".chat-viewport");
    if (!(viewport instanceof HTMLDivElement)) throw new Error("Chat viewport is missing");
    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 1000 });
    Object.defineProperty(viewport, "clientHeight", { configurable: true, value: 200 });
    viewport.scrollTop = 800;
    fireEvent.wheel(viewport, { deltaY: -50 });
    viewport.scrollTop = 400;
    fireEvent.scroll(viewport);
    expect(screen.getByRole("button", { name: "Jump to latest" })).toBeTruthy();
    act(() => {
      channels[0].onmessage?.({ type: "working", eventId: "work" });
      channels[0].onmessage?.({ type: "assistantMessage", eventId: "answer",
        messageId: "answer", content: "Final answer." });
      channels[0].onmessage?.({ type: "idle", eventId: "idle", aborted: false });
    });
    expect(screen.getByRole("button", { name: "Copilot finished · Jump to latest" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Copilot finished · Jump to latest" }));
    expect(screen.queryByRole("button", { name: /Jump to latest/ })).toBeNull();
  });

  it("pauses auto-follow immediately on upward wheel input during streaming", async () => {
    const { container } = render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    const viewport = container.querySelector(".chat-viewport");
    if (!(viewport instanceof HTMLDivElement)) throw new Error("Chat viewport is missing");
    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 1000 });
    Object.defineProperty(viewport, "clientHeight", { configurable: true, value: 200 });
    viewport.scrollTop = 800;
    fireEvent.wheel(viewport, { deltaY: -1 });
    viewport.scrollTop = 795;
    fireEvent.scroll(viewport);
    act(() => channels[0].onmessage?.({
      type: "assistantDelta", eventId: "while-reading", messageId: "m-reading", content: "More output"
    }));
    expect(viewport.scrollTop).toBe(795);
    expect(screen.getByRole("button", { name: "Jump to latest" })).toBeTruthy();
    fireEvent.wheel(viewport, { deltaY: 10 });
    viewport.scrollTop = 800;
    fireEvent.scroll(viewport);
    expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();
  });

  it("lets keyboard scrolling pause and resume follow on the chat viewport", async () => {
    const { container } = render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    const viewport = container.querySelector(".chat-viewport");
    if (!(viewport instanceof HTMLDivElement)) throw new Error("Chat viewport is missing");
    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 1000 });
    Object.defineProperty(viewport, "clientHeight", { configurable: true, value: 200 });
    viewport.scrollTop = 800;
    fireEvent.keyDown(viewport, { key: "PageUp" });
    viewport.scrollTop = 400;
    fireEvent.scroll(viewport);
    expect(screen.getByRole("button", { name: "Jump to latest" })).toBeTruthy();
    fireEvent.keyDown(viewport, { key: "End" });
    viewport.scrollTop = 800;
    fireEvent.scroll(viewport);
    expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();
  });

  it("keeps following through virtual row height corrections without bouncing", async () => {
    const observers: Array<{ callback: ResizeObserverCallback; elements: Set<Element> }> = [];
    const originalObserver = window.ResizeObserver;
    class RowResizeObserver implements ResizeObserver {
      private readonly instance: { callback: ResizeObserverCallback; elements: Set<Element> };
      constructor(callback: ResizeObserverCallback) {
        this.instance = { callback, elements: new Set<Element>() };
        observers.push(this.instance);
      }
      observe(target: Element) { this.instance.elements.add(target); }
      unobserve(target: Element) { this.instance.elements.delete(target); }
      disconnect() { this.instance.elements.clear(); }
    }
    window.ResizeObserver = RowResizeObserver;
    try {
      vi.mocked(api.connectChat).mockResolvedValue({
        ...connected, events: Array.from({ length: 12 }, (_, index) => ({
          type: "assistantMessage" as const, eventId: `event-${index}`,
          messageId: `message-${index}`, content: `Step ${index}`
        }))
      });
      const { container } = render(<ChatPanel sessionId="app-1" active autoCreate={false}
        onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
      const viewport = container.querySelector(".chat-viewport");
      if (!(viewport instanceof HTMLDivElement)) throw new Error("Chat viewport is missing");
      Object.defineProperty(viewport, "scrollHeight", {
        configurable: true,
        get: () => Number.parseFloat((viewport.firstElementChild as HTMLElement).style.height)
      });
      Object.defineProperty(viewport, "clientHeight", { configurable: true, value: 560 });
      await waitFor(() => expect(viewport.scrollTop).toBeGreaterThan(1000));
      await screen.findByText("Step 11");
      const scrollTo = vi.spyOn(viewport, "scrollTo");
      const lastRow = container.querySelector('[data-index="11"]');
      if (!(lastRow instanceof HTMLDivElement)) throw new Error("Latest row is missing");
      const observer = observers.find(({ elements }) => elements.has(lastRow));
      if (!observer) throw new Error("Latest row has no resize observer");
      for (const height of [280, 240, 320]) {
        act(() => observer.callback([{
          target: lastRow, borderBoxSize: [{ blockSize: height, inlineSize: 760 }]
        } as unknown as ResizeObserverEntry], {} as ResizeObserver));
        fireEvent.scroll(viewport);
        expect(screen.queryByRole("button", { name: "Jump to latest" })).toBeNull();
      }
      act(() => {
        for (let index = 0; index < 20; index++) {
          channels[0].onmessage?.({
            type: "assistantDelta", eventId: `stream-${index}`,
            messageId: "streamed", content: "x"
          });
        }
      });
      await screen.findByText("x".repeat(20));
      expect(container.querySelector('[data-index="12"]')).not.toBeNull();
      expect(viewport.scrollTop).toBeGreaterThan(1000);
      expect(scrollTo.mock.calls.length).toBeLessThan(10);
    } finally {
      if (originalObserver) window.ResizeObserver = originalObserver;
      else Reflect.deleteProperty(window, "ResizeObserver");
    }
  });

  it("does not offer approval when SDK permission details are unavailable", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    act(() => channels[0].onmessage?.({
      type: "permissionRequested", eventId: "p-hidden", requestId: "req-hidden",
      kind: "Shell", command: null, workingDirectory: "/home/coder/repo",
      description: null, warning: null, details: [], approvable: false
    }));
    const approve = screen.getByRole("button", { name: "Allow once" }) as HTMLButtonElement;
    expect(approve.disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Deny" }));
    await waitFor(() => expect(api.respondCopilotPermission).toHaveBeenCalledWith(
      "app-1", "req-hidden", false
    ));
  });

  it("reconnects a dropped stream without creating another Copilot conversation", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await screen.findByRole("textbox", { name: "Message to Copilot" });
    expect(api.connectChat).toHaveBeenCalledTimes(1);
    vi.useFakeTimers();
    try {
      act(() => channels[0].onmessage?.({
        type: "disconnected", eventId: "lost", message: "Connection closed"
      }));
      expect(screen.getByRole("button", { name: "Reconnect Copilot" })).toBeTruthy();
      await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
      expect(api.connectChat).toHaveBeenCalledTimes(2);
      expect(api.createChat).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("retries a new chat through an initial host outage without replacing its session", async () => {
    vi.mocked(api.createChat)
      .mockResolvedValueOnce({
        ...connected, status: "hostOffline", copilotSessionId: null,
        error: "SSH unavailable"
      })
      .mockResolvedValueOnce(connected);
    vi.useFakeTimers();
    try {
      render(<ChatPanel sessionId="new-session" active autoCreate
        onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
      await act(async () => { await Promise.resolve(); });
      expect(api.createChat).toHaveBeenCalledTimes(1);
      await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
      expect(api.createChat).toHaveBeenCalledTimes(2);
      expect(api.connectChat).not.toHaveBeenCalled();
      expect(api.replaceChat).not.toHaveBeenCalled();
      expect((screen.getByRole("textbox", { name: "Message to Copilot" }) as HTMLTextAreaElement).disabled)
        .toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps retrying a saved chat after more than three temporary failures", async () => {
    vi.mocked(api.connectChat)
      .mockResolvedValueOnce(connected)
      .mockResolvedValueOnce({ ...connected, status: "hostOffline", error: "Temporary outage" })
      .mockResolvedValueOnce({ ...connected, status: "hostOffline", error: "Temporary outage" })
      .mockResolvedValueOnce({ ...connected, status: "hostOffline", error: "Temporary outage" })
      .mockResolvedValueOnce({ ...connected, status: "hostOffline", error: "Temporary outage" })
      .mockResolvedValueOnce(connected);
    vi.useFakeTimers();
    try {
      render(<ChatPanel sessionId="app-1" active autoCreate={false}
        onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
      await act(async () => { await Promise.resolve(); });
      act(() => channels[0].onmessage?.({
        type: "disconnected", eventId: "drop", message: "Connection closed"
      }));
      for (const delay of [1000, 2000, 4000, 8000, 16_000]) {
        await act(async () => { await vi.advanceTimersByTimeAsync(delay); });
      }
      expect(api.connectChat).toHaveBeenCalledTimes(6);
      expect(api.createChat).not.toHaveBeenCalled();
      expect(api.replaceChat).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("detects a silent tunnel failure via a bounded SDK health check", async () => {
    vi.mocked(api.chatHealth).mockResolvedValue(false);
    vi.useFakeTimers();
    try {
      render(<ChatPanel sessionId="app-1" active autoCreate={false}
        onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
      await act(async () => { await Promise.resolve(); });
      expect(api.connectChat).toHaveBeenCalledTimes(1);
      await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
      expect(api.chatHealth).toHaveBeenCalledWith("app-1");
      expect(api.connectChat).toHaveBeenCalledTimes(2);
      expect(api.createChat).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("detects a lost connection in a hidden cached Chat while work remains active", async () => {
    vi.mocked(api.chatHealth).mockResolvedValue(false);
    vi.useFakeTimers();
    try {
      render(<ChatPanel sessionId="app-1" active={false} autoCreate={false}
        onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
      await act(async () => { await Promise.resolve(); });
      act(() => channels[0].onmessage?.({ type: "working", eventId: "hidden-work" }));
      await act(async () => { await vi.advanceTimersByTimeAsync(15_000); });
      expect(api.chatHealth).toHaveBeenCalledWith("app-1");
      expect(api.connectChat).toHaveBeenCalledTimes(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("requires confirmation before enabling session-scoped Allow all", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    openSettings();
    const allowAll = await screen.findByRole("button", { name: "Allow all..." }) as HTMLButtonElement;
    await waitFor(() => expect(allowAll.disabled).toBe(false));
    fireEvent.click(allowAll);
    expect(screen.getByRole("dialog", { name: "Enable Allow all for this session?" })).toBeTruthy();
    expect(api.setCopilotPermissionMode).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.setCopilotPermissionMode).not.toHaveBeenCalled();
    openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Allow all..." }));
    fireEvent.click(screen.getByRole("button", { name: "Enable Allow all" }));
    await waitFor(() => expect(api.setCopilotPermissionMode).toHaveBeenCalledWith(
      "app-1", "allowAll", true
    ));
    expect(useUi.getState().permissionModes["app-1"]).toBe("allowAll");
    act(() => channels[0].onmessage?.({
      type: "permissionAutoApproved", eventId: "auto-1", requestId: "request-1",
      kind: "Shell", command: "npm test", workingDirectory: "/home/coder/repo", source: "allowAll"
    }));
    expect(screen.getByText("Auto-approval submitted (Allow all)")).toBeTruthy();
    openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Ask" }));
    await waitFor(() => expect(api.setCopilotPermissionMode).toHaveBeenCalledWith(
      "app-1", "ask", false
    ));
    expect(useUi.getState().permissionModes["app-1"]).toBe("ask");
  });

  it("toggles actual SDK Autopilot mode without changing permissions", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    openSettings();
    const autopilot = await screen.findByRole("button", { name: "Autopilot..." }) as HTMLButtonElement;
    await waitFor(() => expect(autopilot.disabled).toBe(false));
    fireEvent.click(autopilot);
    expect(screen.getByRole("dialog", { name: "Enable Autopilot for this conversation?" })).toBeTruthy();
    expect(api.setCopilotAutopilot).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Enable Autopilot" }));
    await waitFor(() => expect(api.setCopilotAutopilot).toHaveBeenCalledWith("app-1", true));
    openSettings();
    expect(screen.getByText(/Autopilot active/)).toBeTruthy();
    expect(useUi.getState().permissionModes["app-1"]).toBeUndefined();
    openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Interactive" }));
    await waitFor(() => expect(api.setCopilotAutopilot).toHaveBeenCalledWith("app-1", false));
  });

  it("shows automatic Read approval and distinguishes Autopilot from Allow all", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({ ...connected, mode: "autopilot" });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    openSettings();
    expect(await screen.findByText(/Autopilot active · complete Read requests allowed/)).toBeTruthy();
    expect(useUi.getState().permissionModes["app-1"]).toBeUndefined();
    act(() => channels[0].onmessage?.({
      type: "permissionAutoApproved", eventId: "read-auto", requestId: "read-1",
      kind: "Read", command: "/remote/repo/src/main.rs",
      workingDirectory: "/remote/repo", source: "autopilot"
    }));
    expect(screen.getAllByText("Autopilot allowed Read").length).toBeGreaterThan(0);
    expect(screen.queryByText("Copilot requests permission: Read")).toBeNull();
  });

  it("requires a separate confirmation for reviewed MCP approvals and displays each automatic call", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({ ...connected, mode: "autopilot" });
    useUi.setState({ permissionModes: { "app-1": "allowAll" } });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await waitFor(() => expect(api.reviewedMcpApprovalState).toHaveBeenCalledWith("app-1"));
    openSettings();
    expect(screen.getByText(/Available in this Chat: example-tools/)).toBeTruthy();
    const grant = screen.getByRole("button", { name: "Approve reviewed MCP automatically..." });
    fireEvent.click(grant);
    expect(screen.queryByRole("dialog", { name: "Chat settings" })).toBeNull();
    expect(api.setReviewedMcpApproval).not.toHaveBeenCalled();
    const dialog = screen.getByRole("dialog", {
      name: "Auto-approve reviewed MCP tools for this session?"
    });
    expect(dialog.textContent).toContain("example-tools");
    fireEvent.click(screen.getByRole("button", { name: "Enable reviewed MCP approvals" }));
    await waitFor(() => expect(api.setReviewedMcpApproval).toHaveBeenCalledWith(
      "app-1", true, true
    ));
    act(() => channels[0].onmessage?.({
      type: "permissionAutoApproved", eventId: "mcp-allowed", requestId: "mcp-1",
      kind: "Mcp", command: "search", workingDirectory: "/remote/repo",
      source: "reviewedMcp"
    }));
    expect(screen.getByText("Approved reviewed MCP call")).toBeTruthy();
    openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Disable reviewed MCP approvals" }));
    await waitFor(() => expect(api.setReviewedMcpApproval).toHaveBeenCalledWith(
      "app-1", false, false
    ));
  });

  it("reconnects a saved Chat from Settings to load reviewed remote MCP configuration", async () => {
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await waitFor(() => expect(api.connectChat).toHaveBeenCalledTimes(1));
    openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Reconnect Copilot" }));
    await waitFor(() => expect(api.connectChat).toHaveBeenCalledTimes(2));
    expect(api.createChat).not.toHaveBeenCalled();
  });

  it("closes Settings before showing reconnect progress or a confirmation dialog", async () => {
    let finishReconnect: ((snapshot: AgentSnapshot) => void) | undefined;
    vi.mocked(api.connectChat).mockResolvedValueOnce(connected)
      .mockImplementationOnce(() => new Promise((resolve) => { finishReconnect = resolve; }));
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await waitFor(() => expect(api.connectChat).toHaveBeenCalledTimes(1));
    openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Reconnect Copilot" }));
    expect(screen.queryByRole("dialog", { name: "Chat settings" })).toBeNull();
    expect(screen.getByText("Connecting to Copilot...")).toBeTruthy();
    await act(async () => { finishReconnect?.(connected); });
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message to Copilot" })
      .hasAttribute("disabled")).toBe(false));
    openSettings();
    fireEvent.click(screen.getByRole("button", { name: "Allow all..." }));
    expect(screen.queryByRole("dialog", { name: "Chat settings" })).toBeNull();
    expect(screen.getByRole("dialog", { name: "Enable Allow all for this session?" })).toBeTruthy();
    expect(api.replaceChat).not.toHaveBeenCalled();
  });

  it("warns when a reviewed remote CLI MCP config changes instead of enabling it", async () => {
    vi.mocked(api.connectChat).mockResolvedValue({
      ...connected, mcpWarnings: ["example-tools"]
    });
    render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    expect(await screen.findByText(/MCP configuration changed or disappeared on remote host: example-tools/)).toBeTruthy();
  });

  it("renders only a bounded number of rows for a long restored conversation", async () => {
    const events: AgentEvent[] = Array.from({ length: 2_000 }, (_, index) => ({
      type: "userMessage", eventId: `event-${index}`, messageId: `message-${index}`,
      content: `Message ${index}`
    }));
    vi.mocked(api.connectChat).mockResolvedValue({ ...connected, events });
    const { container } = render(<ChatPanel sessionId="app-1" active autoCreate={false}
      onLinked={vi.fn(async () => {})} onError={vi.fn()} />);
    await waitFor(() => expect(container.querySelectorAll("[data-index]").length).toBeGreaterThan(0));
    const rendered = container.querySelectorAll("[data-index]").length;
    expect(rendered).toBeLessThan(40);
    const size = container.querySelector(".chat-viewport > div") as HTMLDivElement | null;
    expect(Number.parseFloat(size?.style.height ?? "0")).toBeGreaterThan(100_000);
    const viewport = container.querySelector(".chat-viewport");
    if (!(viewport instanceof HTMLDivElement)) throw new Error("Chat viewport is missing");
    Object.defineProperty(viewport, "scrollHeight", { configurable: true, value: 352_000 });
    Object.defineProperty(viewport, "clientHeight", { configurable: true, value: 560 });
    fireEvent.wheel(viewport, { deltaY: -1 });
    viewport.scrollTop = 176_000;
    fireEvent.scroll(viewport);
    await waitFor(() => expect(container.querySelector('[data-index="1000"]')).not.toBeNull());
    expect(container.querySelectorAll("[data-index]").length).toBeLessThan(40);
    fireEvent.click(screen.getByRole("button", { name: "Jump to latest" }));
    await waitFor(() => expect(container.querySelector('[data-index="1999"]')).not.toBeNull());
    expect(viewport.scrollTop).toBeGreaterThan(340_000);
  });
});
