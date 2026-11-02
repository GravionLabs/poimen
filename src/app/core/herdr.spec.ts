import { describe, expect, it } from "vitest";
import {
  applyStatus,
  buildTree,
  decodeBase64,
  nextWaiting,
  paneLabel,
  rowName,
  waitingCount,
  type Pane,
  type Snapshot,
} from "./herdr";

const pane = (over: Partial<Pane>): Pane => ({
  pane_id: "w1:p1",
  tab_id: "w1:t1",
  workspace_id: "w1",
  agent_status: "unknown",
  ...over,
});

const snapshot: Snapshot = {
  workspaces: [
    { workspace_id: "w2", label: "Second", number: 2, agent_status: "idle" },
    { workspace_id: "w1", label: "First", number: 1, agent_status: "blocked" },
  ],
  tabs: [
    { tab_id: "w1:t2", workspace_id: "w1", label: "b", number: 5, agent_status: "idle" },
    { tab_id: "w1:t1", workspace_id: "w1", label: "a", number: 1, agent_status: "blocked" },
    { tab_id: "w2:t1", workspace_id: "w2", label: "c", number: 1, agent_status: "idle" },
  ],
  panes: [
    pane({ pane_id: "w1:p1", tab_id: "w1:t1", agent_status: "blocked" }),
    pane({ pane_id: "w1:p2", tab_id: "w1:t2" }),
    pane({ pane_id: "w1:p3", tab_id: "w1:t1" }),
  ],
};

describe("buildTree", () => {
  it("orders workspaces and tabs by number and groups panes by tab", () => {
    const tree = buildTree(snapshot);
    expect(tree.map((w) => w.workspace.workspace_id)).toEqual(["w1", "w2"]);
    expect(tree[0].tabs.map((t) => t.tab.tab_id)).toEqual(["w1:t1", "w1:t2"]);
    expect(tree[0].tabs[0].panes.map((p) => p.pane_id)).toEqual(["w1:p1", "w1:p3"]);
    expect(tree[1].tabs[0].panes).toEqual([]);
  });
});

describe("paneLabel", () => {
  it("prefers the terminal title, then agent, then cwd basename, then id", () => {
    expect(paneLabel(pane({ terminal_title_stripped: " Fix auth ", agent: "claude" }))).toBe(
      "Fix auth",
    );
    expect(paneLabel(pane({ terminal_title_stripped: "", agent: "claude" }))).toBe("claude");
    expect(paneLabel(pane({ cwd: "/Users/x/Source/poimen/" }))).toBe("poimen");
    expect(paneLabel(pane({}))).toBe("w1:p1");
  });
});

describe("rowName", () => {
  const title = "jkaufmann@M01:~/Source/gravionlabs/agentlens";
  it("shows the pane label of a labelled terminal", () => {
    expect(rowName(pane({ label: "terminal", terminal_title_stripped: title }), "x")).toBe(
      "terminal",
    );
  });
  it("falls back to the tab label, then the directory name", () => {
    expect(rowName(pane({ terminal_title_stripped: title }), "terminal")).toBe("terminal");
    expect(
      rowName(pane({ terminal_title_stripped: title, cwd: "/Users/x/gravionlabs/agentlens" })),
    ).toBe("agentlens");
  });
  it("never shows user@host or a full path", () => {
    const name = rowName(pane({ terminal_title_stripped: title, cwd: "/a/b/ci" }));
    expect(name).not.toContain("@");
    expect(name).not.toContain("/");
  });
  it("keeps the session title of an agent pane", () => {
    expect(
      rowName(pane({ agent: "claude", terminal_title_stripped: "Projektstand", label: "x" }), "t"),
    ).toBe("Projektstand");
  });
});

describe("status", () => {
  it("counts waiting panes", () => {
    expect(waitingCount(snapshot.panes)).toBe(1);
  });

  it("applies a status change immutably", () => {
    const next = applyStatus(snapshot, "w1:p2", "blocked");
    expect(waitingCount(next.panes)).toBe(2);
    expect(waitingCount(snapshot.panes)).toBe(1);
  });
});

describe("decodeBase64", () => {
  it("decodes ANSI bytes including non-ASCII", () => {
    // "\x1b[2J" followed by "λ" (UTF-8 0xCE 0xBB)
    expect(Array.from(decodeBase64("G1sySs67"))).toEqual([0x1b, 0x5b, 0x32, 0x4a, 0xce, 0xbb]);
  });
});

describe("nextWaiting", () => {
  const panes = [
    pane({ pane_id: "a", agent_status: "blocked" }),
    pane({ pane_id: "b", agent_status: "idle" }),
    pane({ pane_id: "c", agent_status: "blocked" }),
  ];
  it("picks the next blocked pane after the active one", () => {
    expect(nextWaiting(panes, "a")?.pane_id).toBe("c");
  });
  it("wraps around", () => {
    expect(nextWaiting(panes, "c")?.pane_id).toBe("a");
  });
  it("starts at the first blocked pane without an active one", () => {
    expect(nextWaiting(panes, null)?.pane_id).toBe("a");
    expect(nextWaiting(panes, "gone")?.pane_id).toBe("a");
  });
  it("is undefined when nothing waits", () => {
    expect(nextWaiting([pane({ pane_id: "b" })], "b")).toBeUndefined();
  });
});
