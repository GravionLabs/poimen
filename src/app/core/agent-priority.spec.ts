import { describe, expect, it } from "vitest";
import {
  agentSection,
  formatElapsed,
  orderAgents,
  silentLabel,
  type AgentSection,
} from "./agent-priority";

describe("agentSection", () => {
  it("maps statuses onto the four sections", () => {
    expect(agentSection("blocked", false)).toBe("needs");
    expect(agentSection("blocked", true)).toBe("needs");
    expect(agentSection("done", true)).toBe("done");
    expect(agentSection("working", false)).toBe("working");
  });

  it("puts done and read, idle and unknown into idle", () => {
    expect(agentSection("done", false)).toBe("idle");
    expect(agentSection("idle", false)).toBe("idle");
    expect(agentSection("unknown", false)).toBe("idle");
  });
});

describe("formatElapsed", () => {
  const MIN = 60_000;
  it.each([
    [0, "now"],
    [59_999, "now"],
    [-5000, "now"],
    [Number.NaN, "now"],
    [MIN, "1m"],
    [5 * MIN + 30_000, "5m"],
    [59 * MIN, "59m"],
    [60 * MIN, "1h"],
    [2 * 60 * MIN + 59 * MIN, "2h"],
    [24 * 60 * MIN, "1d"],
    [3 * 24 * 60 * MIN + 5 * 60 * MIN, "3d"],
  ])("%d ms is %s", (ms, text) => {
    expect(formatElapsed(ms)).toBe(text);
  });
});

describe("orderAgents", () => {
  const item = (id: string, name = id) => ({ id, name });
  const order = (
    items: { id: string; name: string }[],
    sections: Record<string, AgentSection>,
    at: Record<string, number>,
  ) =>
    orderAgents(
      items,
      (i) => i.id,
      (i) => i.name,
      new Map(Object.entries(sections)),
      new Map(Object.entries(at)),
      1000,
    ).map((i) => i.id);

  it("orders by section: needs, done, working, idle", () => {
    const items = [item("i"), item("w"), item("d"), item("n")];
    expect(order(items, { i: "idle", w: "working", d: "done", n: "needs" }, {})).toEqual([
      "n",
      "d",
      "w",
      "i",
    ]);
  });

  it("puts the agent whose status changed first first inside a section", () => {
    const items = [item("a"), item("b"), item("c")];
    const sections = { a: "working", b: "working", c: "working" } as const;
    expect(order(items, sections, { a: 300, b: 100, c: 200 })).toEqual(["b", "c", "a"]);
  });

  it("breaks ties by name, then id", () => {
    const items = [item("1", "zed"), item("2", "alpha"), item("3", "alpha")];
    const sections = { "1": "idle", "2": "idle", "3": "idle" } as const;
    expect(order(items, sections, { "1": 5, "2": 5, "3": 5 })).toEqual(["2", "3", "1"]);
  });

  it("treats an agent without a section as idle and one without a time as just changed", () => {
    const items = [item("x"), item("y"), item("z")];
    expect(order(items, { y: "needs", z: "idle" }, { z: 2000 })).toEqual(["y", "x", "z"]);
  });

  it("is empty without agents", () => {
    expect(order([], {}, {})).toEqual([]);
  });
});

describe("silentLabel (#899)", () => {
  it("names the elapsed time and is absent when cleared", () => {
    expect(silentLabel(1_000, 1_000 + 12 * 60_000)).toBe("silent 12m");
    expect(silentLabel(undefined, 5)).toBeUndefined();
  });
});
