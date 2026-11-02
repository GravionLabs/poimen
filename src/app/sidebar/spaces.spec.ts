import { describe, expect, it } from "vitest";
import type { Pane, PaneRow, SpaceNode } from "../core/herdr";
import { paneDetail, paneMeta, spaceContext, spaceMeta, stateLabel, subline } from "./spaces";

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
      agent: "claude",
      pane: { tokens: { deliver_step: "review" }, terminal_title_stripped: "title" },
    });
    expect(paneDetail(r)).toBe("review");
    expect(subline(r)).toBe("claude · review");
  });

  it("falls back to the title and handles plain terminals", () => {
    expect(subline(row({ agent: "codex", pane: { terminal_title_stripped: "fix bug" } }))).toBe(
      "codex · fix bug",
    );
    expect(subline(row())).toBe("");
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
