// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";

import { api } from "../api";
import { MarkdownMessage } from "./MarkdownMessage";

vi.mock("../api", () => ({
  api: { openUrl: vi.fn(async () => {}) },
  errorMessage: (error: unknown) => error instanceof Error ? error.message : String(error)
}));

describe("Copilot Markdown", () => {
  const writeText = vi.fn(async (_text: string) => {});
  beforeEach(() => {
    writeText.mockClear();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText }
    });
  });
  afterEach(cleanup);

  it("renders headings, GFM tables, inline code, and fenced blocks with copy", async () => {
    render(<MarkdownMessage onError={vi.fn()} content={[
      "# Findings", "- **Fixed** callback", "",
      "| File | Status |", "| --- | --- |", "| auth.ts | Modified |", "",
      "Use `npm test`.", "", "```typescript", "const answer = 42;", "```"
    ].join("\n")} />);
    expect(screen.getByRole("heading", { name: "Findings" })).toBeTruthy();
    expect(screen.getByRole("table")).toBeTruthy();
    expect(screen.getByText("npm test")).toBeTruthy();
    expect(screen.getByLabelText("typescript code block").textContent).toContain("const answer = 42;");
    fireEvent.click(screen.getByRole("button", { name: "Copy code" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("const answer = 42;\n"));
  });

  it("never executes raw HTML or loads remote images from chat content", () => {
    const { container } = render(<MarkdownMessage onError={vi.fn()} content={
      '<script>alert("unsafe")</script>\n\n[Unsafe](javascript:alert(1))\n\n![secret](https://example.com/image.png)'
    } />);
    expect(container.querySelector("script")).toBeNull();
    expect(container.querySelector("img")).toBeNull();
    expect(screen.getByText("Unsafe").closest("a")).toBeNull();
    expect(screen.getByText(/Image not loaded: secret/)).toBeTruthy();
  });

  it("opens only HTTP(S) links through the desktop backend", async () => {
    vi.mocked(api.openUrl).mockClear();
    render(<MarkdownMessage onError={vi.fn()} content="[Docs](https://example.com/guide)" />);
    fireEvent.click(screen.getByRole("link", { name: "Docs" }));
    await waitFor(() => expect(api.openUrl).toHaveBeenCalledWith("https://example.com/guide"));
  });

  it("can render an incomplete fenced block while the answer streams", () => {
    const { rerender } = render(<MarkdownMessage onError={vi.fn()} content={"```ts\nconst x = 1"} />);
    expect(screen.getByLabelText("ts code block").textContent).toContain("const x = 1");
    rerender(<MarkdownMessage onError={vi.fn()} content={"```ts\nconst x = 12;\n```"} />);
    expect(screen.getByLabelText("ts code block").textContent).toContain("const x = 12;");
  });
});
