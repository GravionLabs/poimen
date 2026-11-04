import { describe, expect, it } from "vitest";
import type { AgentStatus, BranchPull, Terminal, PaneRow, SpaceNode } from "../core/herdr";
import {
  aggregateStatus,
  paneDetail,
  paneLine2,
  paneMarks,
  paneMeta,
  prText,
  prTooltip,
  renderRow,
  DEFAULT_SIDEBAR_ROWS,
  spaceContext,
  spaceMeta,
  stateLabel,
  subline,
} from "./spaces";

const row = (
  over: Omit<Partial<PaneRow>, "pane"> & { pane?: Partial<Terminal> } = {},
): PaneRow => ({
  tab: "t",
  name: "claude",
  worktree: false,
  ...over,
  pane: {
    terminal_id: "w1:p1",
    tab_id: "w1:t1",
    space_id: "w1",
    agent_status: "idle",
    ...over.pane,
  },
});
const space = (rows: PaneRow[]): SpaceNode => ({
  workspace: { space_id: "w1", label: "One", number: 1, agent_status: "idle" },
  rows,
});

describe("stateLabel", () => {
  it("returns the state word, nothing for unknown", () => {
    expect(stateLabel("blocked")).toBe("blocked");
    expect(stateLabel("unknown")).toBeUndefined();
  });
});

describe("paneDetail and subline", () => {
  it("prefers the deliver_step token over the terminal title", () => {
    const r = row({
      name: "Fix login",
      agent: "claude",
      pane: { tokens: { deliver_step: "review" }, terminal_title_stripped: "title" },
    });
    expect(paneDetail(r)).toBe("review");
    expect(subline(r)).toBe("claude · review");
  });

  it("falls back to the title and handles plain terminals", () => {
    expect(
      subline(row({ name: "other", agent: "codex", pane: { terminal_title_stripped: "fix bug" } })),
    ).toBe("codex · fix bug");
    expect(subline(row())).toBe("");
    expect(subline(row({ pane: { terminal_title_stripped: "jkaufmann@M01:~/a/b" } }))).toBe("");
  });
});

describe("subline never repeats the name", () => {
  it("drops the title when it is the bold name", () => {
    const r = row({
      name: "Projektstand",
      agent: "claude",
      pane: { terminal_title_stripped: "Projektstand" },
    });
    expect(subline(r)).toBe("claude");
  });

  it("keeps a step next to the kind while the title is the name", () => {
    const r = row({
      name: "Projektstand",
      agent: "claude",
      pane: { tokens: { deliver_step: "review" }, terminal_title_stripped: "Projektstand" },
    });
    expect(subline(r)).toBe("claude · review");
  });

  it("ignores case and whitespace differences against the name", () => {
    const r = row({
      name: "Projektstand",
      agent: "Claude",
      pane: { terminal_title_stripped: "  projektstand " },
    });
    expect(subline(r)).toBe("Claude");
    expect(subline(row({ name: "claude  code", agent: "Claude Code" }))).toBe("");
  });

  it("shows the kind once when the title repeats it", () => {
    const r = row({ name: "Fix", agent: "claude", pane: { terminal_title_stripped: "claude" } });
    expect(subline(r)).toBe("claude");
    const upper = row({
      name: "Fix",
      agent: "claude",
      pane: { terminal_title_stripped: "Claude" },
    });
    expect(subline(upper)).toBe("claude");
  });

  it("is empty when the name is the kind and nothing else is new", () => {
    expect(subline(row({ name: "claude", agent: "claude" }))).toBe("");
  });
});

describe("paneMarks", () => {
  const open = new Set(["a", "b"]);
  it("marks the active pane and the open ones apart", () => {
    expect(paneMarks("a", open, "a")).toEqual({ open: true, active: true });
    expect(paneMarks("b", open, "a")).toEqual({ open: true, active: false });
    expect(paneMarks("c", open, "a")).toEqual({ open: false, active: false });
  });

  it("has no active pane without an active tab", () => {
    expect(paneMarks("a", open, null)).toEqual({ open: true, active: false });
  });
});

describe("spaceContext, spaceMeta and paneMeta", () => {
  const a = row({ dir: "/a/b/c", branch: "main" });
  const b = row({ dir: "/a/b/c", branch: "main" });

  it("shows shared branch and path on the workspace, not on the panes", () => {
    const ctx = spaceContext(space([a, b]));
    expect(spaceMeta(ctx)).toBe("main · …/b/c");
    expect(paneMeta(a, ctx)).toBe("");
  });

  it("shows what differs on the pane", () => {
    const c = row({ dir: "/a/b/d", branch: "feat", worktree: true });
    const ctx = spaceContext(space([a, c]));
    expect(spaceMeta(ctx)).toBe("");
    expect(paneMeta(c, ctx)).toBe("wt feat · …/b/d");
  });

  it("is empty for a space without rows", () => {
    expect(spaceMeta(spaceContext(space([])))).toBe("");
  });
});

describe("paneLine2", () => {
  it("is one line: agent, title, then only a differing branch or path", () => {
    const a = row({ name: "Fix", agent: "claude", dir: "/a/b/c", branch: "main" });
    const c = row({
      name: "Other",
      agent: "codex",
      dir: "/a/b/c",
      branch: "feat",
      pane: { terminal_title_stripped: "fix" },
    });
    const ctx = spaceContext(space([a, a, row({ dir: "/a/b/c", branch: "main" })]));
    expect(paneLine2(a, ctx)).toBe("claude");
    expect(paneLine2(c, ctx)).toBe("codex · fix · feat");
    expect(paneLine2(row({ dir: "/a/b/c", branch: "main" }), ctx)).toBe("");
  });
});

describe("aggregateStatus", () => {
  const withStatuses = (ws: AgentStatus, ...panes: AgentStatus[]): SpaceNode => ({
    workspace: { space_id: "w1", label: "One", number: 1, agent_status: ws },
    rows: panes.map((s) => row({ pane: { agent_status: s } })),
  });

  it("ranks blocked > working > done > idle > unknown", () => {
    expect(aggregateStatus(withStatuses("unknown", "idle", "blocked", "working"))).toBe("blocked");
    expect(aggregateStatus(withStatuses("unknown", "done", "working"))).toBe("working");
    expect(aggregateStatus(withStatuses("unknown", "idle", "done"))).toBe("done");
    expect(aggregateStatus(withStatuses("unknown", "unknown", "idle"))).toBe("idle");
  });

  it("keeps herdr's value when it is higher or there are no panes", () => {
    expect(aggregateStatus(withStatuses("working"))).toBe("working");
    expect(aggregateStatus(withStatuses("working", "idle"))).toBe("working");
    expect(aggregateStatus(withStatuses("unknown"))).toBe("unknown");
  });
});

describe("renderRow", () => {
  const texts = (lines: ReturnType<typeof renderRow>) =>
    lines.map((l) => l.filter((s) => s.kind !== "dot").map((s) => s.text));
  const a = row({ dir: "/a/b/c", branch: "main" });
  const ctx = spaceContext(space([a, a]));

  it("renders the default layouts like the former subline, paneMeta and spaceMeta", () => {
    const c = row({
      name: "Other",
      agent: "codex",
      dir: "/a/b/c",
      branch: "feat",
      pane: { terminal_title_stripped: "fix", agent_status: "working" },
    });
    expect(texts(renderRow("agent", DEFAULT_SIDEBAR_ROWS.agent, c, ctx))).toEqual([
      ["Other", "working"],
      ["codex", "fix", "feat"],
    ]);
    expect(
      texts(renderRow("workspace", DEFAULT_SIDEBAR_ROWS.workspace, space([a, a]), ctx)),
    ).toEqual([["One", "idle", "main", "…/b/c"]]);
  });

  it("skips empty values, repeats of the name or an earlier value, and empty lines", () => {
    const r = row({ name: "claude", agent: "Claude", pane: { terminal_title_stripped: "t" } });
    const lines = [[{ token: "agent" }, { token: "title" }, { token: "tab" }], [{ token: "dir" }]];
    expect(texts(renderRow("agent", lines, r, ctx))).toEqual([["t"]]);
    const dup = row({ name: "x", agent: "t", tab: "T", pane: { terminal_title_stripped: "t" } });
    expect(texts(renderRow("agent", lines, dup, ctx))).toEqual([["t"]]);
  });

  it("shows branch and dir on a pane only when they differ from the workspace", () => {
    const c = row({ dir: "/a/b/d", branch: "feat", worktree: true });
    const mixed = spaceContext(space([a, c]));
    const lines = [[{ token: "branch" }, { token: "dir" }]];
    expect(texts(renderRow("terminal", lines, c, mixed))).toEqual([["wt feat", "…/b/d"]]);
    expect(texts(renderRow("terminal", lines, a, ctx))).toEqual([]);
  });

  describe("dirty and sync", () => {
    const lines = [[{ token: "dirty" }, { token: "sync" }]];
    const ws = (r: ReturnType<typeof row>) => {
      const sp = space([r, r]);
      return texts(renderRow("workspace", lines, sp, spaceContext(sp)));
    };

    it("renders the count and the arrows on the workspace row", () => {
      expect(ws(row({ dirty: 3, ahead: 2, behind: 1 }))).toEqual([["●3", "↑2 ↓1"]]);
      expect(ws(row({ dirty: 1, ahead: 0, behind: 4 }))).toEqual([["●1", "↓4"]]);
      expect(ws(row({ ahead: 5, behind: 0 }))).toEqual([["↑5"]]);
    });

    it("renders nothing when clean, even, without upstream or unknown", () => {
      expect(ws(row({ dirty: 0, ahead: 0, behind: 0 }))).toEqual([]);
      expect(ws(row({}))).toEqual([]);
    });

    it("shows the workspace row after the branch in the default layout", () => {
      const r = row({ dir: "/a/b/c", branch: "main", dirty: 2, ahead: 1, behind: 0 });
      const sp = space([r, r]);
      expect(
        texts(renderRow("workspace", DEFAULT_SIDEBAR_ROWS.workspace, sp, spaceContext(sp))),
      ).toEqual([["One", "idle", "main", "●2", "↑1", "…/b/c"]]);
    });

    it("shows a pane value only when it differs from the workspace", () => {
      const clean = row({ dirty: 0 });
      const dirty = row({ dirty: 2, ahead: 1 });
      const mixed = spaceContext(space([clean, dirty]));
      expect(texts(renderRow("terminal", lines, dirty, mixed))).toEqual([["●2", "↑1"]]);
      expect(texts(renderRow("terminal", lines, clean, mixed))).toEqual([]);
      const same = spaceContext(space([dirty, dirty]));
      expect(texts(renderRow("terminal", lines, dirty, same))).toEqual([]);
    });
  });

  describe("pr", () => {
    const pull = (over: Partial<BranchPull> = {}): BranchPull => ({
      number: 123,
      title: "Add the thing",
      url: "https://github.com/o/r/pull/123",
      state: "open",
      isDraft: false,
      checks: { state: "passing", passed: 4, failed: 0, pending: 0 },
      ...over,
    });
    const lines = [[{ token: "pr" }]];
    const ws = (r: PaneRow) => {
      const sp = space([r, r]);
      return renderRow("workspace", lines, sp, spaceContext(sp)).map((l) =>
        l.filter((s) => s.kind !== "dot"),
      );
    };
    const checks = (state: BranchPull["checks"]["state"]) =>
      pull({ checks: { state, passed: 1, failed: 1, pending: 1 } });

    it("shows the number with a symbol for passed, failed and pending checks", () => {
      expect(prText(checks("passing"))).toBe("#123 ✓");
      expect(prText(checks("failing"))).toBe("#123 ✗");
      expect(prText(checks("pending"))).toBe("#123 …");
      expect(prText(checks("none"))).toBe("#123");
    });

    it("shows merged for a merged pull request", () => {
      expect(prText(pull({ state: "merged" }))).toBe("merged");
    });

    it("carries the url and a tooltip with title and check counts", () => {
      const [[seg]] = ws(
        row({ pr: pull({ checks: { state: "failing", passed: 3, failed: 1, pending: 2 } }) }),
      );
      expect(seg.text).toBe("#123 ✗");
      expect(seg.url).toBe("https://github.com/o/r/pull/123");
      expect(seg.tip).toBe("#123 Add the thing (open)\nChecks: 3 passed, 1 failed, 2 pending");
      expect(prTooltip(pull({ checks: { state: "none", passed: 0, failed: 0, pending: 0 } }))).toBe(
        "#123 Add the thing (open)",
      );
    });

    it("dims a draft and leaves others alone", () => {
      expect(ws(row({ pr: pull({ isDraft: true }) }))[0][0].dim).toBe(true);
      expect(ws(row({ pr: pull() }))[0][0].dim).toBe(false);
    });

    it("renders nothing without a pull request", () => {
      expect(ws(row({}))).toEqual([]);
    });

    it("shows a pane value only when its pull request differs from the workspace", () => {
      const a = row({ pr: pull() });
      const b = row({ pr: pull({ number: 9, url: "https://github.com/o/r/pull/9" }) });
      const mixed = spaceContext(space([a, b]));
      expect(mixed.pr).toBeUndefined();
      expect(renderRow("terminal", lines, b, mixed)[0].map((s) => s.text)).toEqual(["#9 ✓"]);
      const same = spaceContext(space([a, a]));
      expect(renderRow("terminal", lines, a, same)).toEqual([]);
    });
  });

  it("renders herdr tokens, tab, emphasis, the status classes data and separators", () => {
    const r = row({ tab: "build", pane: { tokens: { deliver_step: " review ", x: "" } } });
    const [line] = renderRow(
      "terminal",
      [
        [
          { token: "state_icon" },
          { token: "$deliver_step", bold: true },
          { token: "$x" },
          { token: "tab", dim: true },
        ],
      ],
      r,
      ctx,
    );
    expect(line.map((s) => [s.kind, s.text, s.bold, s.dim, s.sep, s.status])).toEqual([
      ["dot", "", false, false, false, "idle"],
      ["text", "review", true, false, false, "idle"],
      ["text", "build", false, true, true, "idle"],
    ]);
  });

  it("gives a workspace the aggregate status and no pane-only values", () => {
    const sp = space([row({ pane: { agent_status: "blocked" } })]);
    const [line] = renderRow(
      "workspace",
      [[{ token: "state_icon" }, { token: "state_text" }, { token: "agent" }, { token: "$k" }]],
      sp,
      spaceContext(sp),
    );
    expect(line.map((s) => [s.kind, s.text, s.status])).toEqual([
      ["dot", "", "blocked"],
      ["state", "blocked", "blocked"],
    ]);
  });
});
