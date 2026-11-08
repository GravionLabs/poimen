import { describe, expect, it } from "vitest";
import type { PaneRow, SpaceNode, Terminal } from "../core/herdr";
import { activityText, overviewRows, spaceDirectory, spacePullUrl } from "./overview";

const row = (id: string, over: Partial<PaneRow> = {}, pane: Partial<Terminal> = {}): PaneRow => ({
  pane: { terminal_id: id, tab_id: "t", space_id: "w1", agent_status: "idle", ...pane },
  tab: "t",
  name: id,
  worktree: false,
  ...over,
});
const space = (rows: PaneRow[]): SpaceNode => ({
  workspace: { space_id: "w1", label: "One", number: 1, agent_status: "idle" },
  rows,
});

describe("overviewRows", () => {
  it("lists kind, label, status and activity per pane", () => {
    const rows = overviewRows(
      space([
        row("w1:p1", { name: "shell" }),
        row(
          "w1:p2",
          { name: "claude" },
          { agent: "claude", agent_status: "working", agent_message_at: 5 },
        ),
      ]),
    );
    expect(rows).toEqual([
      {
        paneId: "w1:p1",
        kind: "terminal",
        agent: undefined,
        label: "shell",
        status: "idle",
        activityAt: undefined,
      },
      {
        paneId: "w1:p2",
        kind: "agent",
        agent: "claude",
        label: "claude",
        status: "working",
        activityAt: 5,
      },
    ]);
  });

  it("is empty for a space without panes", () => {
    expect(overviewRows(space([]))).toEqual([]);
  });
});

describe("activityText", () => {
  it.each([
    [undefined, "-"],
    [100_000, "just now"],
    [100_000 - 5 * 60_000, "5 min ago"],
    [100_000 - 3 * 3_600_000, "3 h ago"],
    [100_000 - 2 * 86_400_000, "2 d ago"],
  ])("formats %s as %s", (at, text) => {
    expect(activityText(at, 100_000)).toBe(text);
  });
});

describe("space facts", () => {
  it("finds the first directory and pull request", () => {
    const pr = { number: 1, url: "https://github.com/a/b/pull/1" } as PaneRow["pr"];
    const s = space([row("a"), row("b", { dir: "/x", pr })]);
    expect(spaceDirectory(s)).toBe("/x");
    expect(spacePullUrl(s)).toBe("https://github.com/a/b/pull/1");
  });

  it("has neither without data", () => {
    const s = space([row("a")]);
    expect(spaceDirectory(s)).toBeUndefined();
    expect(spacePullUrl(s)).toBeUndefined();
  });
});
