import { describe, expect, it } from "vitest";
import type { SpaceNode } from "../core/herdr";
import {
  filterValues,
  isDoubleClick,
  menuTarget,
  sidebarMenuItems,
  spaceNodes,
  spaceRepoDir,
} from "./sidebar";
import { DEFAULT_SIDEBAR_ROWS, type SidebarRows } from "./spaces";

const space: SpaceNode = {
  workspace: { space_id: "w1", label: "One", number: 1, agent_status: "idle" },
  rows: [
    {
      pane: { terminal_id: "w1:p1", tab_id: "w1:t1", space_id: "w1", agent_status: "idle" },
      tab: "agent",
      name: "claude",
      agent: "claude",
      worktree: false,
    },
  ],
};

describe("spaceRepoDir", () => {
  const empty = (repo?: SpaceNode["repo"]): SpaceNode => ({
    workspace: { space_id: "s1", label: "app", number: 1, agent_status: "unknown", cwd: "/w/app" },
    rows: [],
    ...(repo ? { repo } : {}),
  });

  it("is the space's own directory for an empty space in a repository", () => {
    expect(spaceRepoDir([empty({ branch: "main", worktree: false })], "s1")).toBe("/w/app");
  });

  it("is undefined when the space's directory is no repository or unknown", () => {
    expect(spaceRepoDir([empty({ branch: null, worktree: false })], "s1")).toBeUndefined();
    expect(spaceRepoDir([empty()], "s1")).toBeUndefined();
    expect(spaceRepoDir([empty()], "s9")).toBeUndefined();
  });

  it("prefers a pane in a repository", () => {
    const node: SpaceNode = {
      ...empty({ branch: "main", worktree: false }),
      rows: [{ ...space.rows[0], dir: "/w/app/sub", branch: "feat" }],
    };
    expect(spaceRepoDir([node], "s1")).toBe("/w/app/sub");
  });
});

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
      "rename-space",
      "close-space",
    ]);
  });

  it("offers to start a task only for a space in a repository", () => {
    const actions = (repo: boolean, kind: "workspace" | "pane") =>
      sidebarMenuItems(kind, repo).flatMap((g) => g.items.map((i) => i.action));
    expect(actions(true, "workspace")).toEqual([
      "new-terminal",
      "show-changes",
      "start-task",
      "cleanup-tasks",
      "rename-space",
      "close-space",
    ]);
    expect(actions(false, "workspace")).not.toContain("start-task");
    expect(actions(false, "workspace")).not.toContain("show-changes");
    expect(actions(false, "workspace")).not.toContain("cleanup-tasks");
    expect(actions(true, "pane")).not.toContain("start-task");
  });

  it("offers the restart on the new host only for a terminal on an older host", () => {
    const actions = (older: boolean) =>
      sidebarMenuItems("pane", false, older).flatMap((g) => g.items.map((i) => i.action));
    expect(actions(true)).toEqual([
      "split-right",
      "split-down",
      "start-agent",
      "rename",
      "restart",
      "close",
    ]);
    expect(actions(false)).not.toContain("restart");
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

describe("sidebarMenuItems", () => {
  it("offers the built-in pane actions in one group", () => {
    const groups = sidebarMenuItems("pane");
    expect(groups.map((g) => g.label)).toEqual([null]);
    expect(groups[0].items.map((i) => i.label)).toEqual([
      "Split right",
      "Split down",
      "Start agent...",
      "Rename...",
      "Close pane...",
    ]);
  });

  it("offers new terminal and close space on a workspace row", () => {
    const groups = sidebarMenuItems("workspace");
    expect(groups).toHaveLength(1);
    expect(groups[0].items.map((i) => i.action)).toEqual([
      "new-terminal",
      "rename-space",
      "close-space",
    ]);
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

describe("sidebarMenuItems on an offline machine (#693)", () => {
  it("disables every entry with the reason", () => {
    const items = sidebarMenuItems("pane", false, false, "lab is offline").flatMap((g) => g.items);
    expect(items.length).toBeGreaterThan(0);
    expect(items.every((i) => i.disabled === "lab is offline")).toBe(true);
  });

  it("leaves the entries enabled otherwise", () => {
    const items = sidebarMenuItems("workspace").flatMap((g) => g.items);
    expect(items.some((i) => i.disabled)).toBe(false);
  });
});
