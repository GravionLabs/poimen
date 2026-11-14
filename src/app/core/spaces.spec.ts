import { describe, expect, it } from "vitest";
import {
  agentKind,
  buildSpaces,
  filterSpaces,
  paneDir,
  shortPath,
  statusText,
  type Terminal,
  type Snapshot,
} from "./snapshot";

const pane = (over: Partial<Terminal>): Terminal => ({
  terminal_id: "w1:p1",
  tab_id: "w1:t1",
  space_id: "w1",
  agent_status: "unknown",
  ...over,
});

describe("pane details", () => {
  it("prefers foreground_cwd over cwd", () => {
    expect(paneDir(pane({ cwd: "/a", foreground_cwd: "/b" }))).toBe("/b");
    expect(paneDir(pane({ cwd: "/a" }))).toBe("/a");
    expect(paneDir(pane({}))).toBeUndefined();
  });

  it("tells agents from plain terminals", () => {
    expect(agentKind(pane({ agent: "codex" }))).toBe("codex");
    expect(agentKind(pane({ agent: " " }))).toBeUndefined();
    expect(agentKind(pane({}))).toBeUndefined();
  });

  it("shows the agent state label plus the deliver_step token", () => {
    expect(
      statusText(
        pane({
          agent: "claude",
          agent_status: "working",
          tokens: { deliver: "#20", deliver_step: "select: next" },
        }),
      ),
    ).toBe("working · select: next");
    expect(statusText(pane({ agent: "claude", agent_status: "idle" }))).toBe("idle");
    expect(statusText(pane({ agent: "claude", tokens: { deliver_step: "x" } }))).toBe("x");
    expect(statusText(pane({ agent: "claude" }))).toBeUndefined();
    expect(statusText(pane({ agent_status: "idle" }))).toBeUndefined();
  });

  it("parses a pinned real `herdr pane list` pane", () => {
    const real = JSON.parse(
      `{"agent":"claude","agent_session":{"agent":"claude","kind":"id","source":"herdr:claude","value":"x"},"agent_status":"working","cwd":"/Users/u/poimen","focused":false,"foreground_cwd":"/Users/u/poimen","terminal_id":"w3:p2","revision":10,"scroll":{"max_offset_from_bottom":0,"offset_from_bottom":0,"viewport_rows":62},"tab_id":"w3:t2","terminal_id":"term_1","terminal_title":"◐ Deliver milestone 0.1.0","terminal_title_stripped":"Deliver milestone 0.1.0","tokens":{"deliver":"#20","deliver_step":"select: next"},"space_id":"w3"}`,
    ) as Terminal;
    expect(agentKind(real)).toBe("claude");
    expect(statusText(real)).toBe("working · select: next");
  });

  it("shortens long paths to their last two segments", () => {
    expect(shortPath("/Users/x/Source/gravionlabs/poimen/")).toBe("…/gravionlabs/poimen");
    expect(shortPath("/tmp/x")).toBe("/tmp/x");
    expect(shortPath("C:\\Users\\x\\Source\\poimen\\")).toBe("…/Source/poimen");
  });
});

describe("buildSpaces and filterSpaces", () => {
  const repos = new Map([
    ["/src/poimen", { branch: "feat/20", worktree: true }],
    ["/src/ci", { branch: null, worktree: false }],
  ]);
  const snapshot: Snapshot = {
    spaces: [
      { space_id: "w2", label: "Empty", number: 2, agent_status: "idle" },
      { space_id: "w1", label: "First", number: 1, agent_status: "idle" },
    ],
    tabs: [
      { tab_id: "w1:t1", space_id: "w1", label: "agent", number: 1, agent_status: "idle" },
      { tab_id: "w1:t2", space_id: "w1", label: "terminal", number: 2, agent_status: "idle" },
    ],
    terminals: [
      pane({
        terminal_id: "w1:p1",
        tab_id: "w1:t1",
        agent: "claude",
        foreground_cwd: "/src/poimen",
      }),
      pane({ terminal_id: "w1:p2", tab_id: "w1:t2", cwd: "/src/ci" }),
    ],
  };
  const spaces = buildSpaces(snapshot, repos);

  it("builds rows with tab, agent, branch and directory", () => {
    expect(spaces.map((s) => s.workspace.space_id)).toEqual(["w1", "w2"]);
    expect(spaces[0].rows[0]).toMatchObject({
      tab: "agent",
      agent: "claude",
      branch: "feat/20",
      worktree: true,
      dir: "/src/poimen",
    });
    expect(spaces[0].rows[1]).toMatchObject({
      tab: "terminal",
      agent: undefined,
      branch: undefined,
      dir: "/src/ci",
    });
    expect(spaces[1].rows).toEqual([]);
  });

  it("returns everything for a blank query", () => {
    expect(filterSpaces(spaces, "  ")).toBe(spaces);
  });

  it("filters by agent, branch and name, case-insensitively, and drops empty workspaces", () => {
    expect(filterSpaces(spaces, "CLAUDE")[0].rows.map((r) => r.pane.terminal_id)).toEqual([
      "w1:p1",
    ]);
    expect(filterSpaces(spaces, "feat/20")).toHaveLength(1);
    expect(filterSpaces(spaces, "claude feat")).toHaveLength(1);
    expect(filterSpaces(spaces, "claude nomatch")).toEqual([]);
  });

  it("matches only the given values when a values function is passed", () => {
    expect(filterSpaces(spaces, "claude", () => ["zzz"])).toEqual([]);
    expect(filterSpaces(spaces, "zzz", () => ["zzz"])[0].rows).toHaveLength(2);
  });

  it("matches the workspace label for all its panes", () => {
    expect(filterSpaces(spaces, "first")[0].rows).toHaveLength(2);
  });
});
