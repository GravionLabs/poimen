import { describe, expect, it } from "vitest";
import { Attention, workspaceUnread } from "./attention";
import type { AgentStatus, SpaceNode } from "./herdr";

const st = (o: Record<string, AgentStatus>) => new Map(Object.entries(o));
const none = new Set<string>();

describe("Attention", () => {
  it("rings a pane that turns blocked while not viewed", () => {
    const a = new Attention();
    a.observe(st({ p1: "working" }), none);
    a.observe(st({ p1: "blocked" }), none);
    expect([...a.attention()]).toEqual(["p1"]);
  });

  it("marks done as unread, not as attention", () => {
    const a = new Attention();
    a.observe(st({ p1: "working" }), none);
    a.observe(st({ p1: "done" }), none);
    expect([...a.unread()]).toEqual(["p1"]);
    expect(a.attention().size).toBe(0);
  });

  it("clears both marks when the tab is viewed", () => {
    const a = new Attention();
    a.observe(st({ p1: "working", p2: "working" }), none);
    a.observe(st({ p1: "blocked", p2: "done" }), none);
    a.observe(st({ p1: "blocked", p2: "done" }), new Set(["p1", "p2"]));
    expect(a.attention().size).toBe(0);
    expect(a.unread().size).toBe(0);
  });

  it("does not mark a pane that changes while viewed", () => {
    const a = new Attention();
    a.observe(st({ p1: "working" }), new Set(["p1"]));
    a.observe(st({ p1: "done" }), new Set(["p1"]));
    expect(a.unread().size).toBe(0);
  });

  it("drops the ring when the pane is no longer blocked, and marks of gone panes", () => {
    const a = new Attention();
    a.observe(st({ p1: "working", p2: "working" }), none);
    a.observe(st({ p1: "blocked", p2: "done" }), none);
    a.observe(st({ p1: "working" }), none);
    expect(a.attention().size).toBe(0);
    expect(a.unread().size).toBe(0);
  });

  it("rings a pane already blocked at start but ignores an old done", () => {
    const a = new Attention();
    a.observe(st({ p1: "blocked", p2: "done" }), none);
    expect([...a.attention()]).toEqual(["p1"]);
    expect(a.unread().size).toBe(0);
  });

  it("sums unread panes per workspace", () => {
    const row = (id: string) => ({ pane: { pane_id: id } }) as SpaceNode["rows"][number];
    const space = { workspace: {}, rows: [row("a"), row("b"), row("c")] } as SpaceNode;
    expect(workspaceUnread(space, new Set(["a", "c", "z"]))).toBe(2);
    expect(workspaceUnread(space, none)).toBe(0);
  });
});
