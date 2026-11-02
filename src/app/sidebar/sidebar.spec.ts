import { describe, expect, it } from "vitest";
import type { SpaceNode } from "../core/herdr";
import { spaceNodes } from "./sidebar";

const space: SpaceNode = {
  workspace: { workspace_id: "w1", label: "One", number: 1, agent_status: "idle" },
  rows: [
    {
      pane: { pane_id: "w1:p1", tab_id: "w1:t1", workspace_id: "w1", agent_status: "idle" },
      tab: "agent",
      name: "claude",
      agent: "claude",
      worktree: false,
    },
  ],
};

describe("spaceNodes", () => {
  it("maps workspaces to expanded parents and panes to leaves keyed by pane id", () => {
    const [node] = spaceNodes([space]);
    expect(node).toMatchObject({ key: "workspace:w1", label: "One", expanded: true });
    expect(node.children).toHaveLength(1);
    expect(node.children![0]).toMatchObject({ key: "w1:p1", label: "claude", leaf: true });
  });

  it("is empty without spaces", () => {
    expect(spaceNodes([])).toEqual([]);
  });
});
