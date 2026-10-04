// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import { api } from "../api";
import type { RecentCliChat } from "../types";
import { RecentCliChatsDialog } from "./RecentCliChatsDialog";

vi.mock("../api", () => ({
  errorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
  api: { listRecentCliChats: vi.fn() }
}));

const recent: RecentCliChat[] = [
  {
    id: "cli-current", summary: "Fix OAuth callback",
    startedAt: "2026-09-25T00:00:00Z", modifiedAt: "2026-09-28T01:00:00Z",
    sameWorkspace: true, isRemote: false, linkedTo: null
  },
  {
    id: "cli-other", summary: "Build a UI",
    startedAt: "2026-09-25T00:00:00Z", modifiedAt: "2026-09-28T02:00:00Z",
    sameWorkspace: false, isRemote: false, linkedTo: null
  },
  {
    id: "cli-cloud", summary: "Cloud task",
    startedAt: "2026-09-25T00:00:00Z", modifiedAt: "2026-09-28T03:00:00Z",
    sameWorkspace: false, isRemote: true, linkedTo: null
  },
  {
    id: "cli-linked", summary: "Another app session",
    startedAt: "2026-09-25T00:00:00Z", modifiedAt: "2026-09-28T04:00:00Z",
    sameWorkspace: true, isRemote: false, linkedTo: "Previous session"
  }
];

describe("Recent CLI chats picker", () => {
  beforeEach(() => {
    vi.mocked(api.listRecentCliChats).mockReset().mockResolvedValue(recent);
  });
  afterEach(cleanup);

  function show(onChoose = vi.fn(async () => {}), onClose = vi.fn()) {
    render(<RecentCliChatsDialog sessionId="app-1" workspacePath="/remote/repo"
      onChoose={onChoose} onClose={onClose} />);
    return { onChoose, onClose };
  }

  it("lists persisted sessions by host and blocks cloud and already-linked chats", async () => {
    show();
    expect(await screen.findByText("Fix OAuth callback")).toBeTruthy();
    expect(screen.getByText("Build a UI")).toBeTruthy();
    expect(screen.getByText(/Cloud session \(not attachable here\)/)).toBeTruthy();
    expect(screen.getByText(/Already linked to Previous session/)).toBeTruthy();
    expect((screen.getByRole("radio", { name: /Cloud task/ }) as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByRole("radio", { name: /Another app session/ }) as HTMLInputElement).disabled).toBe(true);
    fireEvent.change(screen.getByRole("textbox", { name: "Search summaries or IDs" }), {
      target: { value: "oauth" }
    });
    expect(screen.getByText("Fix OAuth callback")).toBeTruthy();
    expect(screen.queryByText("Build a UI")).toBeNull();
  });

  it("attaches a same-repository CLI session only when explicitly selected", async () => {
    const { onChoose, onClose } = show();
    const attach = screen.getByRole("button", { name: "Attach selected chat" }) as HTMLButtonElement;
    expect(attach.disabled).toBe(true);
    fireEvent.click(await screen.findByRole("radio", { name: /Fix OAuth callback/ }));
    fireEvent.click(attach);
    await waitFor(() => expect(onChoose).toHaveBeenCalledWith("cli-current", false));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("requires explicit working-directory confirmation for another repository", async () => {
    const { onChoose } = show();
    fireEvent.click(await screen.findByRole("radio", { name: /Build a UI/ }));
    const attach = screen.getByRole("button", { name: "Attach selected chat" }) as HTMLButtonElement;
    expect(attach.disabled).toBe(true);
    expect(screen.getByText(/may use a different working directory/)).toBeTruthy();
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(attach);
    await waitFor(() => expect(onChoose).toHaveBeenCalledWith("cli-other", true));
  });

  it("keeps the picker open on attach failure so another session can be chosen", async () => {
    const onChoose = vi.fn(async () => { throw new Error("Conversation cannot be resumed"); });
    const { onClose } = show(onChoose);
    fireEvent.click(await screen.findByRole("radio", { name: /Fix OAuth callback/ }));
    fireEvent.click(screen.getByRole("button", { name: "Attach selected chat" }));
    expect(await screen.findByText(/Conversation cannot be resumed/)).toBeTruthy();
    expect(screen.getByRole("dialog", { name: "Choose recent Copilot CLI chat" })).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("reports list failures and supports a refresh", async () => {
    vi.mocked(api.listRecentCliChats)
      .mockRejectedValueOnce(new Error("Headless runtime unavailable"))
      .mockResolvedValueOnce(recent);
    show();
    expect(await screen.findByText(/Headless runtime unavailable/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("Fix OAuth callback")).toBeTruthy();
  });

  it("searches older conversations beyond the first rendered page", async () => {
    const many = Array.from({ length: 225 }, (_, index): RecentCliChat => ({
      id: `cli-${index}`, summary: `Conversation ${index}`,
      startedAt: "2026-09-25T00:00:00Z", modifiedAt: "2026-09-28T01:00:00Z",
      sameWorkspace: true, isRemote: false, linkedTo: null
    }));
    vi.mocked(api.listRecentCliChats).mockResolvedValueOnce(many);
    show();
    expect(await screen.findByText("Conversation 0")).toBeTruthy();
    expect(screen.queryByText("Conversation 224")).toBeNull();
    expect(screen.getByRole("button", { name: "Show more (50 of 225)" })).toBeTruthy();
    fireEvent.change(screen.getByRole("textbox", { name: "Search summaries or IDs" }), {
      target: { value: "Conversation 224" }
    });
    expect(screen.getByText("Conversation 224")).toBeTruthy();
  });
});
