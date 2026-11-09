import { describe, expect, it } from "vitest";
import {
  appVersionLabel,
  windowTitle,
  herdrVersionLabel,
  protocolWarning,
  applyStatus,
  buildTree,
  decodeBase64,
  flowAgentName,
  flowSession,
  nextWaiting,
  paneLabel,
  rowName,
  supportsFlow,
  tabHeader,
  transcriptEmptyText,
  toSnapshot,
  waitingCount,
  agentKinds,
  closeWarning,
  emptySpaces,
  spaceContaining,
  workspaceDirs,
  type Terminal,
  type Snapshot,
} from "./herdr";

const pane = (over: Partial<Terminal>): Terminal => ({
  terminal_id: "w1:p1",
  tab_id: "w1:t1",
  space_id: "w1",
  agent_status: "unknown",
  ...over,
});

const snapshot: Snapshot = {
  spaces: [
    { space_id: "w2", label: "Second", number: 2, agent_status: "idle" },
    { space_id: "w1", label: "First", number: 1, agent_status: "blocked" },
  ],
  tabs: [
    { tab_id: "w1:t2", space_id: "w1", label: "b", number: 5, agent_status: "idle" },
    { tab_id: "w1:t1", space_id: "w1", label: "a", number: 1, agent_status: "blocked" },
    { tab_id: "w2:t1", space_id: "w2", label: "c", number: 1, agent_status: "idle" },
  ],
  terminals: [
    pane({ terminal_id: "w1:p1", tab_id: "w1:t1", agent_status: "blocked" }),
    pane({ terminal_id: "w1:p2", tab_id: "w1:t2" }),
    pane({ terminal_id: "w1:p3", tab_id: "w1:t1" }),
  ],
};

describe("buildTree", () => {
  it("orders workspaces and tabs by number and groups panes by tab", () => {
    const tree = buildTree(snapshot);
    expect(tree.map((w) => w.workspace.space_id)).toEqual(["w1", "w2"]);
    expect(tree[0].tabs.map((t) => t.tab.tab_id)).toEqual(["w1:t1", "w1:t2"]);
    expect(tree[0].tabs[0].panes.map((p) => p.terminal_id)).toEqual(["w1:p1", "w1:p3"]);
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
    expect(rowName(pane({ agent: "claude", terminal_title_stripped: "Projektstand" }), "t")).toBe(
      "Projektstand",
    );
  });
  it("shows the label of an agent pane over its session title and agent name", () => {
    expect(
      rowName(
        pane({ agent: "claude", terminal_title_stripped: "Projektstand", label: " review " }),
      ),
    ).toBe("review");
    expect(rowName(pane({ agent: "claude", label: "review" }))).toBe("review");
  });
  it("falls back to the title and agent name when the label is blank", () => {
    expect(rowName(pane({ agent: "claude", label: "  " }))).toBe("claude");
  });
});

describe("status", () => {
  it("counts waiting panes", () => {
    expect(waitingCount(snapshot.terminals)).toBe(1);
  });

  it("applies a status change immutably", () => {
    const next = applyStatus(snapshot, "w1:p2", "blocked");
    expect(waitingCount(next.terminals)).toBe(2);
    expect(waitingCount(snapshot.terminals)).toBe(1);
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
    pane({ terminal_id: "a", agent_status: "blocked" }),
    pane({ terminal_id: "b", agent_status: "idle" }),
    pane({ terminal_id: "c", agent_status: "blocked" }),
  ];
  it("picks the next blocked pane after the active one", () => {
    expect(nextWaiting(panes, "a")?.terminal_id).toBe("c");
  });
  it("wraps around", () => {
    expect(nextWaiting(panes, "c")?.terminal_id).toBe("a");
  });
  it("starts at the first blocked pane without an active one", () => {
    expect(nextWaiting(panes, null)?.terminal_id).toBe("a");
    expect(nextWaiting(panes, "gone")?.terminal_id).toBe("a");
  });
  it("is undefined when nothing waits", () => {
    expect(nextWaiting([pane({ terminal_id: "b" })], "b")).toBeUndefined();
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

describe("windowTitle", () => {
  it("names the RC build and the waiting agents", () => {
    expect(windowTitle("stable", 0)).toBe("poimen");
    expect(windowTitle("stable", 2)).toBe("poimen (2 waiting)");
    expect(windowTitle("rc", 0)).toBe("poimen RC");
    expect(windowTitle("rc", 1)).toBe("poimen RC (1 waiting)");
  });
});

describe("version labels", () => {
  const info = { version: "0.9.5", protocol: 22, supported: 22 };
  it("names the versions", () => {
    expect(herdrVersionLabel(info)).toBe("backend herdr 0.9.5");
    expect(appVersionLabel("0.2.0")).toBe("frontend poimen 0.2.0");
  });
  it("shows ? when unknown", () => {
    expect(herdrVersionLabel({ version: "0.1", supported: 22 })).toBe("backend herdr 0.1");
    expect(herdrVersionLabel(null)).toBe("backend herdr ?");
    expect(herdrVersionLabel(undefined)).toBe("backend herdr ?");
    expect(herdrVersionLabel({ ...info, version: "0.9.0" }, "poimen")).toBe("backend poimen 0.9.0");
    expect(herdrVersionLabel(null, "poimen")).toBe("backend poimen ?");
    expect(protocolWarning({ ...info, protocol: 1, supported: 2 }, "poimen")).toBe(
      "poimen protocol 1, poimen supports 2",
    );
    expect(appVersionLabel(null)).toBe("frontend poimen ?");
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
    spaces: [],
    tabs: [],
    terminals: [
      pane({ terminal_id: "a", cwd: "/work/a", foreground_cwd: "/work/a/sub" }),
      pane({ terminal_id: "b", cwd: "/work/b" }),
      pane({ terminal_id: "c", cwd: "/work/b" }),
      pane({ terminal_id: "d" }),
      pane({ terminal_id: "e", space_id: "w2", cwd: "/other" }),
    ],
  };

  it("lists the distinct directories of the workspace in pane order", () => {
    expect(workspaceDirs(snapshot, "w1")).toEqual(["/work/a/sub", "/work/b"]);
  });

  it("is empty for a workspace without panes", () => {
    expect(workspaceDirs(snapshot, "w9")).toEqual([]);
  });

  it("puts the space's own directory first and lists it once", () => {
    const withCwd: Snapshot = {
      ...snapshot,
      spaces: [{ space_id: "w1", label: "a", number: 1, agent_status: "idle", cwd: "/work/b" }],
    };
    expect(workspaceDirs(withCwd, "w1")).toEqual(["/work/b", "/work/a/sub"]);
  });

  it("has the space's directory for an empty space and ignores a blank one", () => {
    const space = (cwd: string) => ({
      space_id: "w9",
      label: "x",
      number: 9,
      agent_status: "unknown" as const,
      cwd,
    });
    expect(workspaceDirs({ ...snapshot, spaces: [space("/work/x")] }, "w9")).toEqual(["/work/x"]);
    expect(workspaceDirs({ ...snapshot, spaces: [space("  ")] }, "w9")).toEqual([]);
  });
});

describe("emptySpaces", () => {
  const space = (space_id: string, cwd?: string) => ({
    space_id,
    label: space_id,
    number: 1,
    agent_status: "unknown" as const,
    ...(cwd === undefined ? {} : { cwd }),
  });

  it("lists spaces that have a directory but no terminal", () => {
    const s: Snapshot = {
      spaces: [space("s1", "/a"), space("s2", "/b"), space("s3"), space("s4", "")],
      tabs: [],
      terminals: [pane({ terminal_id: "t", space_id: "s2", cwd: "/b" })],
    };
    expect(emptySpaces(s).map((x) => x.space_id)).toEqual(["s1"]);
  });

  it("is empty without spaces", () => {
    expect(emptySpaces({ spaces: [], tabs: [], terminals: [] })).toEqual([]);
  });
});

describe("spaceContaining", () => {
  const space = (space_id: string, number: number) => ({
    space_id,
    label: space_id,
    number,
    agent_status: "idle" as const,
  });
  const snapshot = (cwds: Record<string, string>): Snapshot => ({
    spaces: Object.keys(cwds).map((id, i) => space(id, i + 1)),
    tabs: [],
    terminals: Object.entries(cwds).map(([space_id, cwd]) =>
      pane({ terminal_id: `${space_id}:p`, space_id, cwd }),
    ),
  });

  it("finds the space whose root is above the directory or the directory itself", () => {
    const s = snapshot({ w1: "/work/app", w2: "/other" });
    expect(spaceContaining(s, "/work/app/src/deep")).toBe("w1");
    expect(spaceContaining(s, "/work/app/")).toBe("w1");
  });

  it("prefers the deepest root", () => {
    const s = snapshot({ w1: "/work", w2: "/work/app" });
    expect(spaceContaining(s, "/work/app/src")).toBe("w2");
  });

  it("does not match a sibling that shares a name prefix or a deeper root", () => {
    const s = snapshot({ w1: "/work/app", w2: "/work/app/src" });
    expect(spaceContaining(s, "/work/application")).toBeUndefined();
    expect(spaceContaining(snapshot({ w2: "/work/app/src" }), "/work/app")).toBeUndefined();
  });

  it("matches Windows paths with drive letters, spaces and backslashes, ignoring case", () => {
    const s = snapshot({ w1: "C:\\Users\\Jo Doe\\My Projects" });
    expect(spaceContaining(s, "c:/users/jo doe/my projects/app")).toBe("w1");
    expect(spaceContaining(s, "D:\\Users\\Jo Doe\\My Projects\\app")).toBeUndefined();
  });

  it("is undefined for a blank directory or no spaces", () => {
    expect(spaceContaining(snapshot({}), "/a")).toBeUndefined();
    expect(spaceContaining(snapshot({ w1: "/a" }), "  ")).toBeUndefined();
  });
});

describe("agentKinds", () => {
  it("offers only the kinds that report through hooks on the poimen backend", () => {
    expect(agentKinds("poimen")).toEqual(["claude", "copilot"]);
  });

  it("keeps herdr's full list on the herdr backend", () => {
    const kinds = agentKinds("herdr");
    expect(kinds).toHaveLength(24);
    expect(kinds.slice(0, 5)).toEqual(["claude", "codex", "gemini", "opencode", "copilot"]);
    expect(kinds).toContain("muse");
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

describe("flow agents", () => {
  const session = { value: "0b7e-c0" };

  it("follows Claude Code and Copilot CLI panes", () => {
    for (const agent of ["claude", "copilot", " copilot "]) {
      expect(supportsFlow(pane({ agent })), agent).toBe(true);
      expect(flowSession(pane({ agent, agent_session: session })), agent).toBe("0b7e-c0");
    }
    expect(flowAgentName(pane({ agent: "claude" }))).toBe("Claude Code");
    expect(flowAgentName(pane({ agent: "copilot" }))).toBe("GitHub Copilot CLI");
  });

  it("does not follow other agents, plain terminals or panes without a session", () => {
    for (const agent of ["codex", "gemini", "toString", "", undefined]) {
      expect(supportsFlow(pane({ agent })), agent).toBe(false);
      expect(flowSession(pane({ agent, agent_session: session })), agent).toBeNull();
      expect(flowAgentName(pane({ agent })), agent).toBeUndefined();
    }
    expect(supportsFlow(undefined)).toBe(false);
    expect(flowSession(pane({ agent: "copilot" }))).toBeNull();
    expect(flowSession(pane({ agent: "copilot", agent_session: { value: "" } }))).toBeNull();
  });
});

describe("toSnapshot", () => {
  it("maps the wire records onto spaces and terminals and keeps the id values", () => {
    const snapshot = toSnapshot({
      workspaces: [{ workspace_id: "w1", label: "One", number: 1, agent_status: "idle" }],
      tabs: [
        { tab_id: "w1:t1", workspace_id: "w1", label: "main", number: 1, agent_status: "idle" },
      ],
      panes: [
        { pane_id: "w1:p1", tab_id: "w1:t1", workspace_id: "w1", agent_status: "done", cwd: "/a" },
      ],
    });
    expect(snapshot).toEqual({
      spaces: [{ space_id: "w1", label: "One", number: 1, agent_status: "idle" }],
      tabs: [{ tab_id: "w1:t1", space_id: "w1", label: "main", number: 1, agent_status: "idle" }],
      terminals: [
        { terminal_id: "w1:p1", tab_id: "w1:t1", space_id: "w1", agent_status: "done", cwd: "/a" },
      ],
    });
  });

  it("keeps the directory of a space without terminals (after a server restart)", () => {
    const snapshot = toSnapshot({
      workspaces: [
        { workspace_id: "s1", label: "app", number: 1, agent_status: "unknown", cwd: "/work/app" },
      ],
      tabs: [],
      panes: [],
    });
    expect(snapshot.spaces).toEqual([
      { space_id: "s1", label: "app", number: 1, agent_status: "unknown", cwd: "/work/app" },
    ]);
    expect(workspaceDirs(snapshot, "s1")).toEqual(["/work/app"]);
    expect(spaceContaining(snapshot, "/work/app/src")).toBe("s1");
    expect(emptySpaces(snapshot).map((s) => s.space_id)).toEqual(["s1"]);
  });

  it("maps an empty snapshot", () => {
    expect(toSnapshot({ workspaces: [], tabs: [], panes: [] })).toEqual({
      spaces: [],
      tabs: [],
      terminals: [],
    });
  });
});

describe("transcript empty states", () => {
  const named = "Claude Code and GitHub Copilot CLI";

  it("names both harnesses for a pane that runs no known agent", () => {
    expect(transcriptEmptyText(pane({ agent: "codex" }), "The flow")).toBe(
      `The flow is available for ${named} agents. This pane does not run one.`,
    );
    expect(transcriptEmptyText(pane({}), "Subagent output")).toBe(
      `Subagent output is available for ${named} agents. This pane does not run one.`,
    );
  });

  it("asks to select a pane when there is no pane", () => {
    expect(transcriptEmptyText(undefined, "Subagent output")).toBe(
      `Subagent output is available for ${named} agents. Select a pane that runs one.`,
    );
  });

  it("waits for the session of a known agent", () => {
    expect(transcriptEmptyText(pane({ agent: "claude" }), "The flow")).toBe(
      "Waiting for the agent to report its session.",
    );
    expect(
      transcriptEmptyText(pane({ agent: "copilot", agent_session: { value: "" } }), "The flow"),
    ).toBe("Waiting for the agent to report its session.");
  });

  it("waits for the transcript once the session is known", () => {
    expect(
      transcriptEmptyText(
        pane({ agent: "claude", agent_session: { value: "0b7e-c0" } }),
        "The flow",
      ),
    ).toBe("Waiting for the agent's transcript to appear.");
  });
});
