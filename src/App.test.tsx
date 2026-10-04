// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import { api } from "./api";
import App from "./App";
import { useUi } from "./store";
import type { SessionWithStatus } from "./types";

vi.mock("./api", () => ({
  errorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
  api: {
    listHosts: vi.fn(async () => []),
    listAllWorkspaces: vi.fn(async () => []),
    listSessions: vi.fn(),
    deleteSession: vi.fn(),
    forgetSession: vi.fn()
  }
}));

vi.mock("./components/MainPane", () => ({
  MainPane: ({ session, onDelete }: {
    session: SessionWithStatus | undefined;
    onDelete: () => void;
  }) => session ? <button type="button" onClick={onDelete}>Delete</button> : null
}));

const existing: SessionWithStatus = {
  id: "session-1", workspaceId: "workspace", name: "Example", tmuxSessionName: "crui_example",
  copilotSessionId: null,
  copilotHasMessages: false,
  command: "copilot", pinned: false, createdAt: "2026-09-25T00:00:00Z",
  lastOpenedAt: null, status: "dead",
  host: { id: "host", name: "Remote", sshHost: "remote", createdAt: "2026-09-25T00:00:00Z" },
  workspace: {
    id: "workspace", hostId: "host", repoPath: "/home/coder/repo",
    displayName: "repo", createdAt: "2026-09-25T00:00:00Z"
  }
};

describe("session deletion", () => {
  let present: boolean;

  beforeEach(() => {
    present = true;
    useUi.setState({ activeSessionId: existing.id, connections: {} });
    vi.mocked(api.listSessions).mockImplementation(async () => present ? [existing] : []);
    vi.mocked(api.deleteSession).mockReset();
    vi.mocked(api.forgetSession).mockReset();
    vi.spyOn(window, "confirm").mockImplementation(() => {
      throw new Error("Native confirmation must not be used");
    });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("uses an in-app confirmation and deletes the remote tmux session", async () => {
    vi.mocked(api.deleteSession).mockImplementation(async () => { present = false; });
    const { container } = render(<App />);
    expect(container.querySelector(".relative.flex")?.classList.contains("min-h-0")).toBe(true);
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    expect(screen.getByRole("dialog", { name: "Delete session" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Delete and stop tmux" }));
    await waitFor(() => expect(api.deleteSession).toHaveBeenCalledWith(existing.id));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Delete session" })).toBeNull());
    expect(api.forgetSession).not.toHaveBeenCalled();
  });

  it("reports host errors and allows an explicit metadata-only removal", async () => {
    vi.mocked(api.deleteSession).mockRejectedValue(new Error("Host unavailable"));
    vi.mocked(api.forgetSession).mockImplementation(async () => { present = false; });
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Delete" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete and stop tmux" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("Host unavailable"));
    fireEvent.click(screen.getByRole("button", { name: "Forget only" }));
    await waitFor(() => expect(api.forgetSession).toHaveBeenCalledWith(existing.id));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Delete session" })).toBeNull());
  });
});
