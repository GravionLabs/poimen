import { describe, expect, it } from "vitest";
import type { SpaceNode } from "../core/herdr";
import { filterValues, isDoubleClick, menuTarget, sidebarMenuItems, spaceNodes } from "./sidebar";
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
    expect(sidebarMenuItems("workspace").flatMap((g) => g.items.map((i) => i.action))).toEqual([
      "new-terminal",
      "close-space",
    ]);
  });

  it("offers split and close on a pane", () => {
    expect(sidebarMenuItems("pane").flatMap((g) => g.items.map((i) => i.action))).toEqual([
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
  const act = (plugin_id: string, action_id: string, title: string) => ({
    plugin_id,
    action_id,
    title,
    description: null,
  });

  it("puts built-in actions first, then one group per plugin by name and title", () => {
    const groups = sidebarMenuItems("pane", [
      act("review-popup", "open", "Review"),
      act("deliver-board", "start", "Start deliver"),
      act("deliver-board", "open", "Open board"),
    ]);
    expect(groups.map((g) => g.label)).toEqual([null, "deliver-board", "review-popup"]);
    expect(groups[0].items).toHaveLength(5);
    expect(groups[1].items).toEqual([
      { action: 'plugin:["deliver-board","open"]', label: "Open board" },
      { action: 'plugin:["deliver-board","start"]', label: "Start deliver" },
    ]);
  });

  it("keeps the order stable regardless of the input order", () => {
    const a = [act("b", "1", "Y"), act("a", "2", "Z"), act("a", "3", "X")];
    expect(sidebarMenuItems("pane", a)).toEqual(sidebarMenuItems("pane", [...a].reverse()));
  });

  it("makes no group for a plugin without actions and none on a workspace row", () => {
    expect(sidebarMenuItems("pane")).toHaveLength(1);
    const groups = sidebarMenuItems("workspace", [act("p", "x", "X")]);
    expect(groups).toHaveLength(1);
    expect(groups[0].items).toHaveLength(2);
  });
});

describe("menuTarget", () => {
  const labels = ["Split right", "Split down", "Rename...", "Review"];

  it("moves with the arrows and wraps", () => {
    expect(menuTarget(labels, -1, "ArrowDown")).toBe(0);
    expect(menuTarget(labels, 3, "ArrowDown")).toBe(0);
    expect(menuTarget(labels, 0, "ArrowUp")).toBe(3);
    expect(menuTarget(labels, -1, "ArrowUp")).toBe(3);
  });

  it("jumps with Home and End", () => {
    expect(menuTarget(labels, 2, "Home")).toBe(0);
    expect(menuTarget(labels, 1, "End")).toBe(3);
  });

  it("type-ahead finds the next label that starts with the character", () => {
    expect(menuTarget(labels, 0, "s")).toBe(1);
    expect(menuTarget(labels, 1, "r")).toBe(2);
    expect(menuTarget(labels, 2, "R")).toBe(3);
    expect(menuTarget(labels, 0, "z")).toBeNull();
  });

  it("ignores other keys and an empty menu", () => {
    expect(menuTarget(labels, 0, "Tab")).toBeNull();
    expect(menuTarget(labels, 0, " ")).toBeNull();
    expect(menuTarget([], -1, "ArrowDown")).toBeNull();
  });
});
