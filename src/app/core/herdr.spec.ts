import { describe, expect, it } from "vitest";
import {
  appVersionLabel,
  herdrVersionLabel,
  protocolWarning,
  applyStatus,
  buildTree,
  decodeBase64,
  nextWaiting,
  paneLabel,
  rowName,
  tabHeader,
  waitingCount,
  closeWarning,
  workspaceDirs,
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

describe("tabHeader", () => {
  it("puts the workspace before the pane name and builds the full tooltip", () => {
    expect(tabHeader("Poimen", "Deliver milestone 0.1.0")).toEqual({
      group: "Poimen",
      label: "Deliver milestone 0.1.0",
      title: "POIMEN / Deliver milestone 0.1.0",
    });
    expect(tabHeader("Poimen", "terminal").title).toBe("POIMEN / terminal");
  });

  it("keeps a long name whole for truncation by CSS", () => {
    const name = "x".repeat(200);
    expect(tabHeader("W", name).title).toBe(`W / ${name}`);
  });

  it("is only the name without a workspace", () => {
    expect(tabHeader(undefined, "claude")).toEqual({
      group: undefined,
      label: "claude",
      title: "claude",
    });
    expect(tabHeader("  ", "claude").group).toBeUndefined();
  });
});

describe("version labels", () => {
  const info = { version: "0.9.5", protocol: 22, supported: 22 };
  it("names the versions", () => {
    expect(herdrVersionLabel(info)).toBe("herdr 0.9.5");
    expect(appVersionLabel("0.2.0")).toBe("poimen 0.2.0");
  });
  it("shows ? when unknown", () => {
    expect(herdrVersionLabel({ version: "0.1", supported: 22 })).toBe("herdr 0.1");
    expect(herdrVersionLabel(null)).toBe("herdr ?");
    expect(herdrVersionLabel(undefined)).toBe("herdr ?");
    expect(appVersionLabel(null)).toBe("poimen ?");
  });
  it("warns only on a protocol mismatch", () => {
    expect(protocolWarning(info)).toBe("");
    expect(protocolWarning(null)).toBe("");
    expect(protocolWarning({ version: "0.1", supported: 22 })).toBe("");
    expect(protocolWarning({ ...info, protocol: 23 })).toBe(
      "herdr protocol 23, poimen supports 22",
    );
  });
});

describe("workspaceDirs", () => {
  const snapshot: Snapshot = {
    workspaces: [],
    tabs: [],
    panes: [
      pane({ pane_id: "a", cwd: "/work/a", foreground_cwd: "/work/a/sub" }),
      pane({ pane_id: "b", cwd: "/work/b" }),
      pane({ pane_id: "c", cwd: "/work/b" }),
      pane({ pane_id: "d" }),
      pane({ pane_id: "e", workspace_id: "w2", cwd: "/other" }),
    ],
  };

  it("lists the distinct directories of the workspace in pane order", () => {
    expect(workspaceDirs(snapshot, "w1")).toEqual(["/work/a/sub", "/work/b"]);
  });

  it("is empty for a workspace without panes", () => {
    expect(workspaceDirs(snapshot, "w9")).toEqual([]);
  });
});

describe("closeWarning", () => {
  it("warns that a working or waiting agent ends", () => {
    expect(closeWarning(pane({ agent: "claude", agent_status: "working" }))).toBe(
      "The claude agent is working; closing the pane ends it.",
    );
    expect(closeWarning(pane({ agent: "codex", agent_status: "blocked" }))).toBe(
      "The codex agent is waiting for you; closing the pane ends it.",
    );
  });

  it("names an idle agent without the alarm", () => {
    expect(closeWarning(pane({ agent: "claude", agent_status: "idle" }))).toBe(
      "The claude agent in the pane ends.",
    );
  });

  it("is plain for a terminal or an unknown pane", () => {
    expect(closeWarning(pane({}))).toBe("The process in the pane ends.");
    expect(closeWarning(undefined)).toBe("The process in the pane ends.");
  });
});
