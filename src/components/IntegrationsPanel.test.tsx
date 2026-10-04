// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";

import { api } from "../api";
import type { McpInventory } from "../types";
import { IntegrationsPanel } from "./IntegrationsPanel";

vi.mock("../api", () => ({
  errorMessage: (error: unknown) => error instanceof Error ? error.message : String(error),
  api: {
    listMcpServers: vi.fn(),
    addMcpServer: vi.fn(async () => {}),
    importCliMcpServer: vi.fn(async () => {}),
    activateCliMcpServer: vi.fn(async () => "connected"),
    removeMcpServer: vi.fn(async () => {}),
    authenticateMcpServer: vi.fn(),
    openUrl: vi.fn(async () => {})
  }
}));

const inventory: McpInventory = {
  reviewed: [{
    name: "approved-server", url: "https://mcp.example.com/mcp",
    status: "needs-auth", error: null
  }],
  imported: [],
  availableCli: [{
    name: "example-tools", transport: "stdio", command: "example-tools",
    endpointHost: null, reviewed: false, needsReview: false
  }],
  external: ["external-managed-server"],
  warning: null
};

describe("Reviewed MCP integrations", () => {
  beforeEach(() => {
    vi.mocked(api.listMcpServers).mockReset().mockResolvedValue(inventory);
    vi.mocked(api.addMcpServer).mockClear();
    vi.mocked(api.importCliMcpServer).mockClear();
    vi.mocked(api.activateCliMcpServer).mockClear();
    vi.mocked(api.removeMcpServer).mockClear();
    vi.mocked(api.authenticateMcpServer).mockReset();
    vi.mocked(api.openUrl).mockClear();
  });
  afterEach(cleanup);

  it("lists remote host MCP status without displaying unreviewed server configuration", async () => {
    render(<IntegrationsPanel sessionId="app-1" host="remote-1" />);
    expect(await screen.findByText("approved-server")).toBeTruthy();
    expect(screen.getByText("needs-auth")).toBeTruthy();
    expect(screen.getByText(/external-managed-server/)).toBeTruthy();
    expect(screen.getByText(/disabled for Chat/)).toBeTruthy();
  });

  it("requires explicit review before storing a new HTTPS MCP endpoint", async () => {
    render(<IntegrationsPanel sessionId="app-1" host="remote-1" />);
    const add = screen.getByRole("button", { name: "Add reviewed server" }) as HTMLButtonElement;
    fireEvent.change(screen.getByRole("textbox", { name: "Server name" }), {
      target: { value: "tools" }
    });
    fireEvent.change(screen.getByRole("textbox", { name: "HTTPS endpoint" }), {
      target: { value: "https://example.com/mcp" }
    });
    expect(add.disabled).toBe(true);
    expect(api.addMcpServer).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("checkbox"));
    fireEvent.click(add);
    await waitFor(() => expect(api.addMcpServer).toHaveBeenCalledWith(
      "app-1", "tools", "https://example.com/mcp", true
    ));
  });

  it("reviews and enables an existing remote MCP configuration without copying credentials", async () => {
    vi.mocked(api.listMcpServers).mockResolvedValueOnce(inventory).mockResolvedValue({
      ...inventory,
      imported: [{ name: "example-tools", status: "disabled", needsReview: false, error: null }],
      availableCli: [{ ...inventory.availableCli[0], reviewed: true }]
    });
    render(<IntegrationsPanel sessionId="app-1" host="remote-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Review & enable..." }));
    let dialog = screen.getByRole("dialog", { name: "Review existing MCP configuration" });
    expect(within(dialog).getByText(/full remote CLI configuration/)).toBeTruthy();
    expect((within(dialog).getByRole("button", { name: "Enable for Chat" }) as HTMLButtonElement).disabled)
      .toBe(true);
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(api.importCliMcpServer).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Review & enable..." }));
    dialog = screen.getByRole("dialog", { name: "Review existing MCP configuration" });
    fireEvent.click(within(dialog).getByRole("checkbox"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Enable for Chat" }));
    await waitFor(() => expect(api.importCliMcpServer).toHaveBeenCalledWith("app-1", "example-tools", true));
    expect(await screen.findByText(/Use Activate in Chat/)).toBeTruthy();
    expect(screen.getByText("· Copilot CLI config")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Activate in Chat" }));
    await waitFor(() => expect(api.activateCliMcpServer).toHaveBeenCalledWith("app-1", "example-tools"));
  });

  it("requires fresh approval when a previously imported CLI config changes", async () => {
    vi.mocked(api.listMcpServers).mockResolvedValue({
      ...inventory,
      imported: [{ name: "example-tools", status: "disabled", needsReview: true, error: null }],
      availableCli: [{ ...inventory.availableCli[0], reviewed: true, needsReview: true }]
    });
    render(<IntegrationsPanel sessionId="app-1" host="remote-1" />);
    expect(await screen.findByText(/Remote configuration changed or disappeared/)).toBeTruthy();
    expect((screen.getByRole("button", { name: "Activate in Chat" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Review changes..." }));
    const dialog = screen.getByRole("dialog", { name: "Review existing MCP configuration" });
    expect((within(dialog).getByRole("button", { name: "Enable for Chat" }) as HTMLButtonElement).disabled).toBe(true);
    fireEvent.click(within(dialog).getByRole("checkbox"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Enable for Chat" }));
    await waitFor(() => expect(api.importCliMcpServer).toHaveBeenCalledWith("app-1", "example-tools", true));
  });

  it("provides remote-browser sign-in without claiming a Mac callback exists", async () => {
    vi.mocked(api.authenticateMcpServer).mockResolvedValueOnce({
      authorizationUrl: "https://login.example.com/auth?state=opaque",
      note: "Use an approved browser on remote host."
    }).mockResolvedValueOnce({
      authorizationUrl: "https://login.example.com/auth?state=next",
      note: "Use an approved browser on remote host."
    });
    render(<IntegrationsPanel sessionId="app-1" host="remote-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Authenticate" }));
    await waitFor(() => expect(api.authenticateMcpServer).toHaveBeenCalledWith("app-1", "approved-server", false));
    expect(screen.getByText("https://login.example.com/auth?state=opaque")).toBeTruthy();
    expect(screen.getByText(/Mac browser cannot finish this flow/)).toBeTruthy();
    expect(api.openUrl).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Reauthenticate" }));
    expect(screen.getByRole("dialog", { name: "Reauthenticate MCP server?" })).toBeTruthy();
    expect(api.authenticateMcpServer).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Start reauthentication" }));
    await waitFor(() => expect(api.authenticateMcpServer).toHaveBeenCalledWith("app-1", "approved-server", true));
    expect(screen.queryByRole("button", { name: "Open sign-in in browser" })).toBeNull();
    expect(screen.getByText(/callback is on remote host/)).toBeTruthy();
  });

  it("requires confirmation before removing a reviewed server", async () => {
    render(<IntegrationsPanel sessionId="app-1" host="remote-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Remove..." }));
    expect(screen.getByRole("dialog", { name: "Remove reviewed MCP server?" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(api.removeMcpServer).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Remove..." }));
    fireEvent.click(screen.getByRole("button", { name: "Remove server" }));
    await waitFor(() => expect(api.removeMcpServer).toHaveBeenCalledWith("app-1", "approved-server"));
  });
});
