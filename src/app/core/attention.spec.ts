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

  it("treats each finished turn of the poimen backend (idle, working, done, working, done) as done", () => {
    const a = new Attention();
    a.observe(st({ p1: "idle" }), none);
    expect(a.observe(st({ p1: "working" }), none)).toEqual([]);
    expect(a.observe(st({ p1: "done" }), none)).toEqual([{ paneId: "p1", status: "done" }]);
    expect([...a.unread()]).toEqual(["p1"]);
    a.observe(st({ p1: "done" }), new Set(["p1"]));
    expect(a.unread().size).toBe(0);
    a.observe(st({ p1: "working" }), none);
    a.observe(st({ p1: "done" }), none);
    expect([...a.unread()]).toEqual(["p1"]);
    expect(a.count()).toBe(1);
    expect(a.lastChanges()).toEqual([{ paneId: "p1", status: "done" }]);
  });

  it("does not mark an agent that only goes working to idle (what the poimen backend reported before #406)", () => {
    const a = new Attention();
    a.observe(st({ p1: "working" }), none);
    expect(a.observe(st({ p1: "idle" }), none)).toEqual([]);
    expect(a.unread().size).toBe(0);
  });

  it("does not mark panes that were already done when the real snapshot arrives", () => {
    const a = new Attention();
    a.observe(st({}), none);
    a.observe(st({ p1: "done" }), none);
    expect(a.unread().size).toBe(0);
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
    const row = (id: string) => ({ pane: { terminal_id: id } }) as SpaceNode["rows"][number];
    const space = { workspace: {}, rows: [row("a"), row("b"), row("c")] } as SpaceNode;
    expect(workspaceUnread(space, new Set(["a", "c", "z"]))).toBe(2);
    expect(workspaceUnread(space, none)).toBe(0);
  });
});

describe("Attention changes", () => {
  it("returns panes that turned blocked or done, not first sightings or repeats", () => {
    const a = new Attention();
    expect(a.observe(st({}), none)).toEqual([]);
    expect(a.observe(st({ p1: "blocked", p2: "working" }), none)).toEqual([]);
    expect(a.observe(st({ p1: "blocked", p2: "done" }), none)).toEqual([
      { paneId: "p2", status: "done" },
    ]);
    expect(a.observe(st({ p1: "working", p2: "done" }), none)).toEqual([]);
    expect(a.observe(st({ p1: "blocked", p2: "done" }), new Set(["p1"]))).toEqual([
      { paneId: "p1", status: "blocked" },
    ]);
  });

  it("keeps the latest changes for observers, replaced only by a snapshot that has some", () => {
    const a = new Attention();
    a.observe(st({ p1: "working" }), none);
    a.observe(st({ p1: "done" }), none);
    const first = a.lastChanges();
    expect(first).toEqual([{ paneId: "p1", status: "done" }]);
    a.observe(st({ p1: "done" }), none);
    expect(a.lastChanges()).toBe(first);
    a.observe(st({ p1: "working" }), none);
    a.observe(st({ p1: "done" }), none);
    expect(a.lastChanges()).not.toBe(first);
  });

  it("does not notify done while a linked run keeps the pane working, then once it ends", () => {
    const a = new Attention();
    a.observe(st({ p1: "working" }), none);
    // Stop reports done, but the run it started still runs: the effective status stays working.
    expect(a.observe(st({ p1: "working" }), none)).toEqual([]);
    expect(a.unread().size).toBe(0);
    expect(a.observe(st({ p1: "done" }), none)).toEqual([{ paneId: "p1", status: "done" }]);
    expect([...a.unread()]).toEqual(["p1"]);
  });
});
