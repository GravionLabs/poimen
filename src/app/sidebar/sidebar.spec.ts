import { describe, expect, it } from "vitest";
import type { SpaceNode } from "../core/herdr";
import { isDoubleClick, sidebarMenuItems, spaceNodes } from "./sidebar";

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

describe("sidebarMenuItems", () => {
  it("offers a new terminal on a workspace", () => {
    expect(sidebarMenuItems("workspace").map((i) => i.action)).toEqual(["new-terminal"]);
  });

  it("offers split and close on a pane", () => {
    expect(sidebarMenuItems("pane").map((i) => i.action)).toEqual([
      "split-right",
      "split-down",
      "start-agent",
      "rename",
      "close",
    ]);
  });
});

describe("isDoubleClick", () => {
  it("is true for the same row within the interval", () => {
    expect(isDoubleClick({ key: "p1", at: 1000 }, "p1", 1300)).toBe(true);
  });

  it("is false for another row, a late click or no earlier click", () => {
    expect(isDoubleClick({ key: "p1", at: 1000 }, "p2", 1100)).toBe(false);
    expect(isDoubleClick({ key: "p1", at: 1000 }, "p1", 1500)).toBe(false);
    expect(isDoubleClick(null, "p1", 1000)).toBe(false);
  });
});
