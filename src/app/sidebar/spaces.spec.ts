import { describe, expect, it } from "vitest";
import type { AgentStatus, Pane, PaneRow, SpaceNode } from "../core/herdr";
import {
  aggregateStatus,
  paneDetail,
  paneLine2,
  paneMarks,
  paneMeta,
  spaceContext,
  spaceMeta,
  stateLabel,
  subline,
} from "./spaces";

const row = (over: Omit<Partial<PaneRow>, "pane"> & { pane?: Partial<Pane> } = {}): PaneRow => ({
  tab: "t",
  name: "claude",
  worktree: false,
  ...over,
  pane: {
    pane_id: "w1:p1",
    tab_id: "w1:t1",
    workspace_id: "w1",
    agent_status: "idle",
    ...over.pane,
  },
});
const space = (rows: PaneRow[]): SpaceNode => ({
  workspace: { workspace_id: "w1", label: "One", number: 1, agent_status: "idle" },
  rows,
});

describe("stateLabel", () => {
  it("returns the state word, nothing for unknown", () => {
    expect(stateLabel("blocked")).toBe("blocked");
    expect(stateLabel("unknown")).toBeUndefined();
  });
});

describe("paneDetail and subline", () => {
  it("prefers the deliver_step token over the terminal title", () => {
    const r = row({
      name: "Fix login",
      agent: "claude",
      pane: { tokens: { deliver_step: "review" }, terminal_title_stripped: "title" },
    });
    expect(paneDetail(r)).toBe("review");
    expect(subline(r)).toBe("claude · review");
  });

  it("falls back to the title and handles plain terminals", () => {
    expect(
      subline(row({ name: "other", agent: "codex", pane: { terminal_title_stripped: "fix bug" } })),
    ).toBe("codex · fix bug");
    expect(subline(row())).toBe("");
    expect(subline(row({ pane: { terminal_title_stripped: "jkaufmann@M01:~/a/b" } }))).toBe("");
  });
});

describe("subline never repeats the name", () => {
  it("drops the title when it is the bold name", () => {
    const r = row({
      name: "Projektstand",
      agent: "claude",
      pane: { terminal_title_stripped: "Projektstand" },
    });
    expect(subline(r)).toBe("claude");
  });

  it("keeps a step next to the kind while the title is the name", () => {
    const r = row({
      name: "Projektstand",
      agent: "claude",
      pane: { tokens: { deliver_step: "review" }, terminal_title_stripped: "Projektstand" },
    });
    expect(subline(r)).toBe("claude · review");
  });

  it("is empty when the name is the kind and nothing else is new", () => {
    expect(subline(row({ name: "claude", agent: "claude" }))).toBe("");
  });
});

describe("paneMarks", () => {
  const open = new Set(["a", "b"]);
  it("marks the active pane and the open ones apart", () => {
    expect(paneMarks("a", open, "a")).toEqual({ open: true, active: true });
    expect(paneMarks("b", open, "a")).toEqual({ open: true, active: false });
    expect(paneMarks("c", open, "a")).toEqual({ open: false, active: false });
  });

  it("has no active pane without an active tab", () => {
    expect(paneMarks("a", open, null)).toEqual({ open: true, active: false });
  });
});

describe("spaceContext, spaceMeta and paneMeta", () => {
  const a = row({ dir: "/a/b/c", branch: "main" });
  const b = row({ dir: "/a/b/c", branch: "main" });

  it("shows shared branch and path on the workspace, not on the panes", () => {
    const ctx = spaceContext(space([a, b]));
    expect(spaceMeta(ctx)).toBe("main · …/b/c");
    expect(paneMeta(a, ctx)).toBe("");
  });

  it("shows what differs on the pane", () => {
    const c = row({ dir: "/a/b/d", branch: "feat", worktree: true });
    const ctx = spaceContext(space([a, c]));
    expect(spaceMeta(ctx)).toBe("");
    expect(paneMeta(c, ctx)).toBe("wt feat · …/b/d");
  });

  it("is empty for a space without rows", () => {
    expect(spaceMeta(spaceContext(space([])))).toBe("");
  });
});

describe("paneLine2", () => {
  it("is one line: agent, title, then only a differing branch or path", () => {
    const a = row({ name: "Fix", agent: "claude", dir: "/a/b/c", branch: "main" });
    const c = row({
      name: "Other",
      agent: "codex",
      dir: "/a/b/c",
      branch: "feat",
      pane: { terminal_title_stripped: "fix" },
    });
    const ctx = spaceContext(space([a, a, row({ dir: "/a/b/c", branch: "main" })]));
    expect(paneLine2(a, ctx)).toBe("claude");
    expect(paneLine2(c, ctx)).toBe("codex · fix · feat");
    expect(paneLine2(row({ dir: "/a/b/c", branch: "main" }), ctx)).toBe("");
  });
});

describe("aggregateStatus", () => {
  const withStatuses = (ws: AgentStatus, ...panes: AgentStatus[]): SpaceNode => ({
    workspace: { workspace_id: "w1", label: "One", number: 1, agent_status: ws },
    rows: panes.map((s) => row({ pane: { agent_status: s } })),
  });

  it("ranks blocked > working > done > idle > unknown", () => {
    expect(aggregateStatus(withStatuses("unknown", "idle", "blocked", "working"))).toBe("blocked");
    expect(aggregateStatus(withStatuses("unknown", "done", "working"))).toBe("working");
    expect(aggregateStatus(withStatuses("unknown", "idle", "done"))).toBe("done");
    expect(aggregateStatus(withStatuses("unknown", "unknown", "idle"))).toBe("idle");
  });

  it("keeps herdr's value when it is higher or there are no panes", () => {
    expect(aggregateStatus(withStatuses("working"))).toBe("working");
    expect(aggregateStatus(withStatuses("working", "idle"))).toBe("working");
    expect(aggregateStatus(withStatuses("unknown"))).toBe("unknown");
  });
});
