// @vitest-environment jsdom
import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import { api } from "../api";
import type { PortInventory } from "../types";
import { PortsPanel } from "./PortsPanel";

vi.mock("../api", () => ({
  errorMessage: (error: unknown) => String(error),
  api: {
    listPortForwards: vi.fn(),
    pauseChatTunnel: vi.fn(async () => {}),
    resumeChatTunnel: vi.fn(async () => {})
  }
}));

const inventory: PortInventory = {
  chat: { localPort: 50123, remotePort: 30123, paused: false },
  configured: [
    { direction: "Remote", spec: "0.0.0.0:4443 localhost:4443" },
    { direction: "Remote", spec: "0.0.0.0:9000 localhost:9000" }
  ]
};

describe("Ports view", () => {
  afterEach(cleanup);

  it("distinguishes app-owned Chat forwarding from external SSH configuration", async () => {
    vi.mocked(api.listPortForwards).mockReset().mockResolvedValue(inventory);
    render(<PortsPanel sessionId="app-1" host="remote-host" />);
    expect(await screen.findByText(/127\.0\.0\.1:50123/)).toBeTruthy();
    expect(screen.getByText(/0\.0\.0\.0:4443/)).toBeTruthy();
    expect(screen.getByText(/0\.0\.0\.0:9000/)).toBeTruthy();
    expect(screen.getByText(/manage those forwards in the process that opened them/i)).toBeTruthy();
    expect(screen.getAllByRole("button").map((button) => button.textContent)).not.toContain("Release 4443");
  });

  it("pauses only the owned tunnel and can resume without touching configured forwards", async () => {
    vi.mocked(api.listPortForwards).mockReset()
      .mockResolvedValueOnce(inventory)
      .mockResolvedValueOnce({ ...inventory, chat: { ...inventory.chat, localPort: null, paused: true } })
      .mockResolvedValueOnce(inventory);
    vi.mocked(api.pauseChatTunnel).mockClear();
    vi.mocked(api.resumeChatTunnel).mockClear();
    render(<PortsPanel sessionId="app-1" host="remote-host" />);
    fireEvent.click(await screen.findByRole("button", { name: "Release Chat tunnel" }));
    await waitFor(() => expect(api.pauseChatTunnel).toHaveBeenCalledWith("app-1"));
    fireEvent.click(await screen.findByRole("button", { name: "Resume Chat tunnel" }));
    await waitFor(() => expect(api.resumeChatTunnel).toHaveBeenCalledWith("app-1"));
    expect(screen.getByText(/0\.0\.0\.0:4443/)).toBeTruthy();
  });

  it("shows why release was rejected instead of pretending the port is free", async () => {
    vi.mocked(api.listPortForwards).mockReset().mockResolvedValue(inventory);
    vi.mocked(api.pauseChatTunnel).mockRejectedValueOnce(new Error("Agent still working"));
    render(<PortsPanel sessionId="app-1" host="remote-host" />);
    fireEvent.click(await screen.findByRole("button", { name: "Release Chat tunnel" }));
    expect(await screen.findByText(/Agent still working/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Release Chat tunnel" })).toBeTruthy();
  });
});
