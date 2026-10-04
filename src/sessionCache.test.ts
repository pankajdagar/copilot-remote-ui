import { describe, expect, it } from "vitest";
import { touchSessionCache } from "./sessionCache";

describe("terminal cache", () => {
  it("keeps revisited sessions attached and moves them to the newest position", () => {
    const cached = ["a", "b", "c"];
    expect(touchSessionCache(cached, "a")).toEqual(["b", "c", "a"]);
    expect(cached).toEqual(["a", "b", "c"]);
  });

  it("evicts only the least recently used session after reaching capacity", () => {
    expect(touchSessionCache(["a", "b", "c", "d"], "e")).toEqual(["b", "c", "d", "e"]);
    const cached = ["b", "c", "d", "e"];
    expect(touchSessionCache(cached, "e")).toBe(cached);
  });
});
