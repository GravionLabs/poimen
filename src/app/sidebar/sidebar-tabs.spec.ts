import { describe, expect, it } from "vitest";
import { buildSpaces, filterSpaces, type PaneRow, type Snapshot } from "../core/snapshot";
import { nestTabs, spaceNodes } from "./sidebar";

const pane = (id: string, tab: string, extra = {}) => ({
  terminal_id: id,
  tab_id: tab,
  space_id: "s1",
  agent_status: "idle" as const,
  ...extra,
});

/** Tabs: claude (agent, two panes), terminal and logs below claude, shell top level. */
function snapshot(): Snapshot {
  const tab = (tab_id: string, label: string, number: number, parent?: string) => ({
    tab_id,
    space_id: "s1",
    label,
    number,
    agent_status: "idle" as const,
    ...(parent ? { parent } : {}),
  });
  return {
    spaces: [{ space_id: "s1", label: "space", number: 1, agent_status: "idle" }],
    tabs: [
      tab("claude", "claude", 1),
      tab("term", "Terminal", 2, "claude"),
      tab("logs", "Logs", 3, "claude"),
      tab("shell", "shell", 4),
    ],
    terminals: [
      pane("t1", "claude", { agent: "claude", label: "claude" }),
      pane("t2", "term", { label: "Terminal" }),
      pane("t3", "logs", { label: "Logs" }),
      pane("t4", "shell", { label: "shell" }),
    ],
  } as unknown as Snapshot;
}

const rows = () => buildSpaces(snapshot(), new Map())[0].rows;
const keys = (nodes: { key: string; children?: unknown[] }[]): unknown[] =>
  nodes.map((n) => (n.children?.length ? [n.key, keys(n.children as never)] : n.key));

describe("child tabs in the tree", () => {
  it("nests the rows of a child tab below the first row of its parent, in tab order", () => {
    const [space] = buildSpaces(snapshot(), new Map());
    const [node] = spaceNodes([space]);
    expect(keys(node.children!)).toEqual([["t1", ["t2", "t3"]], "t4"]);
    const parent = node.children![0];
    expect(parent.leaf).toBe(false);
    expect(parent.expanded).toBe(true);
    expect(node.children![1].leaf).toBe(true);
  });

  it("keeps one row per pane of a split tab, all under the tab's place", () => {
    const s = snapshot();
    s.terminals.push(pane("t5", "term", { label: "Second" }) as never);
    const [space] = buildSpaces(s, new Map());
    const [node] = spaceNodes([space]);
    expect(keys(node.children!)).toEqual([["t1", ["t2", "t5", "t3"]], "t4"]);
  });

  it("makes a tab top level when its parent has no row or the parents form a cycle", () => {
    const orphan = rows().filter((r) => r.tabId !== "claude");
    expect(
      nestTabs(orphan, (r) => ({ key: r.pane.terminal_id, label: r.name })).map((n) => n.key),
    ).toEqual(["t2", "t3", "t4"]);
    const cyclic = rows().map((r) =>
      r.tabId === "claude" ? ({ ...r, tabParent: "logs" } as PaneRow) : r,
    );
    const nodes = nestTabs(cyclic, (r) => ({ key: r.pane.terminal_id, label: r.name }));
    expect(nodes.map((n) => n.key)).toEqual(["t1", "t3", "t4"]);
    expect(nodes[0].children?.map((n) => n.key)).toEqual(["t2"]);
    expect(nodes[1].children).toBeUndefined();
  });

  it("shows the parent of a matching child, and nothing else of the parent's siblings", () => {
    const [space] = buildSpaces(snapshot(), new Map());
    const [kept] = filterSpaces([space], "logs");
    expect(kept.rows.map((r) => r.pane.terminal_id)).toEqual(["t1", "t3"]);
    const [node] = spaceNodes([kept]);
    expect(keys(node.children!)).toEqual([["t1", ["t3"]]]);
  });
});
