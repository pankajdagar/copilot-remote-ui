// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";

import { api } from "../api";
import { ChangesPanel } from "./ChangesPanel";

vi.mock("../api", () => ({
  api: { listChanges: vi.fn() },
  errorMessage: (error: unknown) => error instanceof Error ? error.message : String(error)
}));

describe("remote changes", () => {
  beforeEach(() => vi.mocked(api.listChanges).mockReset());
  afterEach(cleanup);

  it("renders Git's file status independently of Copilot activity", async () => {
    vi.mocked(api.listChanges).mockResolvedValue([
      { path: "src/auth/callback.ts", status: " M" },
      { path: "tests/auth/callback.test.ts", status: "??" }
    ]);
    const onCount = vi.fn();
    render(<ChangesPanel sessionId="session-1" onCount={onCount} />);
    expect(await screen.findByText("src/auth/callback.ts")).toBeTruthy();
    expect(screen.getByText("tests/auth/callback.test.ts")).toBeTruthy();
    expect(onCount).toHaveBeenCalledWith(2);
    expect(api.listChanges).toHaveBeenCalledWith("session-1");
  });

});
