import { describe, expect, it } from "vitest";
import type { SpaceNode } from "../core/herdr";
import { filterValues, isDoubleClick, sidebarMenuItems, spaceNodes } from "./sidebar";
import { DEFAULT_SIDEBAR_ROWS, type SidebarRows } from "./spaces";

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

describe("spaceNodes rows", () => {
  it("renders the configured lines per row type and falls back to the defaults", () => {
    const [def] = spaceNodes([space]);
    expect(def.children![0].data).toMatchObject({ kind: "pane" });
    const rows = {
      ...DEFAULT_SIDEBAR_ROWS,
      agent: [[{ token: "tab", bold: true }]],
    };
    const [node] = spaceNodes([space], rows);
    const pane = node.children![0].data as { lines: { text: string; bold: boolean }[][] };
    expect(pane.lines.map((l) => l.map((s) => [s.text, s.bold]))).toEqual([[["agent", true]]]);
  });

  it("filters on the values of the shown tokens", () => {
    const rows: SidebarRows = { ...DEFAULT_SIDEBAR_ROWS, agent: [[{ token: "tab" }]] };
    expect(filterValues(space, space.rows[0], rows)).toContain("agent");
    expect(filterValues(space, space.rows[0], rows)).not.toContain("claude");
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

describe("sidebarMenuItems with plugin actions", () => {
  const plugin = {
    plugin_id: "review-popup",
    action_id: "open",
    title: "Review",
    description: null,
  };

  it("appends the plugin actions to a pane row menu", () => {
    const items = sidebarMenuItems("pane", [plugin]);
    expect(items.at(-1)).toEqual({
      action: 'plugin:["review-popup","open"]',
      label: "Review",
    });
    expect(items[0]).toEqual({ action: "split-right", label: "Split right" });
  });

  it("leaves the workspace menu and a menu without plugins alone", () => {
    expect(sidebarMenuItems("workspace", [plugin])).toHaveLength(1);
    expect(sidebarMenuItems("pane")).toHaveLength(5);
  });
});
