import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { Attention, warningIds } from "./attention";
import { tabMarks, withWarning } from "./agent-priority";
import { DEFAULT_SIDEBAR_ROWS, recordRows, renderRow } from "../sidebar/spaces";
import {
  applyStatus,
  nextWaiting,
  paneNeedsAttention,
  statusText,
  stateText,
  toSnapshot,
  waitingCount,
  type AgentStatus,
  type PaneRow,
  type SpaceNode,
  type Terminal,
} from "./snapshot";

const pane = (over: Partial<Terminal> = {}): Terminal => ({
  terminal_id: "p1",
  tab_id: "t1",
  space_id: "s1",
  agent: "codex",
  agent_status: "done",
  ...over,
});

const none = new Set<string>();
const st = (o: Record<string, AgentStatus>) => new Map(Object.entries(o));

describe("done with a warning (#1152)", () => {
  it("is the tone warning on a done agent and means nothing for another status", () => {
    expect(stateText("done", { tone: "warning" })).toBe("done: warning");
    expect(stateText("done")).toBe("done");
    expect(stateText("working", { tone: "warning" })).toBe("working");
    expect(paneNeedsAttention(pane({ agent_tone: "warning" }))).toBe(true);
    expect(paneNeedsAttention(pane())).toBe(false);
    expect(paneNeedsAttention(pane({ agent_status: "working", agent_tone: "warning" }))).toBe(
      false,
    );
    expect(statusText(pane({ agent_tone: "warning" }))).toBe("done: warning");
  });

  it("counts like blocked and error in the waiting count and the next waiting pane", () => {
    const panes = [pane({ terminal_id: "a" }), pane({ terminal_id: "b", agent_tone: "warning" })];
    expect(waitingCount(panes)).toBe(1);
    expect(nextWaiting(panes, null)?.terminal_id).toBe("b");
  });

  it("is kept by the event and cleared by a later one without it", () => {
    const snapshot = toSnapshot({ terminals: [pane()] });
    const warned = applyStatus(snapshot, "p1", "done", undefined, { tone: "warning" });
    expect(warned.terminals[0]!.agent_tone).toBe("warning");
    expect(applyStatus(warned, "p1", "done").terminals[0]!.agent_tone).toBeUndefined();
    expect(
      applyStatus(snapshot, "p1", "working", undefined, { tone: "warning" }).terminals[0]!
        .agent_tone,
    ).toBeUndefined();
    // A wire snapshot drops a tone that does not belong to its status.
    const wire = toSnapshot({
      terminals: [
        pane({ agent_tone: "evil" }),
        pane({ agent_status: "idle", agent_tone: "warning" }),
      ],
    });
    expect(wire.terminals.map((t) => t.agent_tone)).toEqual([undefined, undefined]);
  });

  it("rings a pane that is done with a warning until it is viewed", () => {
    const a = new Attention();
    a.observe(st({ p1: "working" }), none);
    const changes = a.observe(st({ p1: "done" }), none, 5, new Set(["p1"]));
    expect(changes).toEqual([{ paneId: "p1", status: "done" }]);
    expect([...a.attention()]).toEqual(["p1"]);
    expect(a.unread().size).toBe(0);
    a.observe(st({ p1: "done" }), new Set(["p1"]), 6, new Set(["p1"]));
    expect(a.attention().size).toBe(0);
    // Without the warning, done is only unread.
    const b = new Attention();
    b.observe(st({ p1: "working" }), none);
    b.observe(st({ p1: "done" }), none);
    expect(b.attention().size).toBe(0);
    expect([...b.unread()]).toEqual(["p1"]);
  });

  it("marks the dot of a pane, a space and a record, with the state word", () => {
    const paneRow: PaneRow = {
      tab: "t",
      name: "deliver",
      worktree: false,
      agent: "codex",
      pane: pane({ agent_tone: "warning" }),
    };
    const dot = (r: PaneRow) =>
      renderRow("agent", DEFAULT_SIDEBAR_ROWS.agent, r, { worktree: false })[0]!;
    const line = dot(paneRow);
    expect(line.find((s) => s.kind === "dot")?.warning).toBe(true);
    expect(line.find((s) => s.kind === "state")?.text).toBe("done: warning");
    const plain = dot({ ...paneRow, pane: pane() });
    expect(plain.find((s) => s.kind === "dot")?.warning).toBe(false);
    const space: SpaceNode = {
      workspace: { space_id: "s1", label: "One", number: 1, agent_status: "done" },
      rows: [paneRow],
    };
    const header = renderRow("workspace", DEFAULT_SIDEBAR_ROWS.workspace, space, {
      worktree: false,
    })[0]!;
    expect(header.find((s) => s.kind === "dot")?.warning).toBe(true);
    const records = recordRows(
      pane({
        records: [
          { id: "run/a", status: "done", tone: "warning" },
          { id: "run/b", status: "done" },
        ],
      }),
    );
    expect(records.map((r) => [r.warning, r.state])).toEqual([
      [true, "done: warning"],
      [false, "done"],
    ]);
  });

  it("shows the message in the tab tooltip", () => {
    expect(withWarning("Tab", false, "x")).toBe("Tab");
    expect(withWarning("Tab", true, " blocked: 2 ")).toBe("Tab · done with a warning: blocked: 2");
    expect(withWarning("Tab", true, undefined)).toBe("Tab · done with a warning");
    const warned = pane({
      agent_tone: "warning",
      agent_message: "waiting 2",
      agent_silent_since: 0,
    });
    expect(tabMarks("Tab", warned, 60_000)).toBe(
      "Tab · silent 1m · done with a warning: waiting 2",
    );
    expect(tabMarks("Tab", pane(), 0)).toBe("Tab");
    expect(
      warningIds(
        new Map([
          ["a", warned],
          ["b", pane()],
        ]),
      ),
    ).toEqual(new Set(["a"]));
  });

  it("draws the dot as a diamond in the warning token, not by color alone", () => {
    const css = readFileSync("src/styles.css", "utf8");
    const rule = /\.status-done\.warning \{([^}]*)\}/.exec(css)?.[1] ?? "";
    expect(rule).toContain("var(--status-warning)");
    expect(rule).toContain("rotate(45deg)");
    expect(css).toMatch(/--status-warning: var\(--status-blocked\)/);
    const tab = readFileSync("src/app/app.html", "utf8");
    expect(tab).toContain('[class.warning]="p?.agent_tone !== undefined"');
    const card = readFileSync("src/app/sidebar/space-card.html", "utf8");
    expect(card.match(/\[class\.warning\]/g)?.length).toBeGreaterThanOrEqual(4);
  });
});
