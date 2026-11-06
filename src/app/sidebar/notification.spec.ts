import { describe, expect, it } from "vitest";
import { applyStatus, type PaneRow, type Snapshot, type SpaceNode } from "../core/herdr";
import { renderRow, spaceContext } from "./spaces";

const row = (id: string, message?: string, at?: number): PaneRow => ({
  pane: {
    terminal_id: id,
    tab_id: id,
    space_id: "s1",
    agent: "claude",
    agent_status: "blocked",
    agent_message: message,
    agent_message_at: at,
  },
  tab: "t",
  name: id,
  agent: "claude",
  worktree: false,
});

const space = (...rows: PaneRow[]): SpaceNode => ({
  workspace: { space_id: "s1", label: "api", number: 1, agent_status: "blocked" },
  rows,
});

const lines = [[{ token: "notification" }]];

describe("notification token", () => {
  it("shows the text of a pane on one line with the full text as tooltip", () => {
    const node = space(row("a", "  Allow Bash to run rm?  ", 5));
    const out = renderRow("agent", lines, node.rows[0], spaceContext(node));
    expect(out).toHaveLength(1);
    expect(out[0]).toMatchObject([
      { kind: "text", text: "Allow Bash to run rm?", tip: "Allow Bash to run rm?" },
    ]);
  });

  it("is empty without a message (herdr backend)", () => {
    const node = space(row("a"));
    expect(renderRow("agent", lines, node.rows[0], spaceContext(node))).toEqual([]);
    expect(renderRow("workspace", lines, node, spaceContext(node))).toEqual([]);
  });

  it("shows the newest message of the agents on the workspace row", () => {
    const node = space(row("a", "older", 100), row("b", "newest", 300), row("c", undefined));
    const ctx = spaceContext(node);
    expect(ctx.notification).toBe("newest");
    const out = renderRow("workspace", lines, node, ctx);
    expect(out[0][0]).toMatchObject({ text: "newest", tip: "newest" });
  });
});

describe("applyStatus with a notification text", () => {
  const snapshot = (): Snapshot => ({
    spaces: [],
    tabs: [],
    terminals: [row("a").pane, row("b", "keep", 1).pane],
  });
  const get = (s: Snapshot, id: string) => s.terminals.find((t) => t.terminal_id === id)!;

  it("stores the text with its time", () => {
    const s = applyStatus(snapshot(), "a", "blocked", { text: "Allow?", at: 42 });
    expect(get(s, "a")).toMatchObject({ agent_message: "Allow?", agent_message_at: 42 });
    expect(get(s, "b").agent_message).toBe("keep");
  });

  it("keeps the text on done and clears it when the agent works again or ends", () => {
    expect(get(applyStatus(snapshot(), "b", "done"), "b").agent_message).toBe("keep");
    const working = get(applyStatus(snapshot(), "b", "working"), "b");
    expect(working).not.toHaveProperty("agent_message");
    expect(working).not.toHaveProperty("agent_message_at");
    expect(get(applyStatus(snapshot(), "b", "unknown"), "b").agent_message).toBeUndefined();
  });
});
