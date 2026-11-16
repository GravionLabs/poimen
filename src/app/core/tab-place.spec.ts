import { describe, expect, it } from "vitest";
import { openAtFor } from "./tab-place";
import type { Snapshot, Tab, Terminal } from "./snapshot";

const tab = (id: string, extra: Partial<Tab> = {}): Tab =>
  ({
    tab_id: id,
    space_id: "s1",
    label: id,
    number: 1,
    agent_status: "idle",
    root: { kind: "terminal", terminal_id: `t-${id}` },
    ...extra,
  }) as Tab;
const term = (id: string): Terminal =>
  ({ terminal_id: `t-${id}`, tab_id: id, space_id: "s1" }) as Terminal;
const snap = (tabs: Tab[]): Snapshot =>
  ({ spaces: [], tabs, terminals: tabs.map((t) => term(t.tab_id)) }) as unknown as Snapshot;

describe("openAtFor", () => {
  it("places the tabs of the #608 example: claude left, Terminal right top, Logs right bottom", () => {
    const tabs = [
      tab("claude"),
      tab("terminal", {
        parent: "claude",
        place: { direction: "right", of: "claude", ratio: 0.6 },
      }),
      tab("logs", { parent: "claude", place: { direction: "down", of: "terminal", ratio: 0.5 } }),
    ];
    const s = snap(tabs);
    expect(openAtFor(s, tabs[0])).toEqual({ main: true });
    expect(openAtFor(s, tabs[1])).toEqual({
      place: { ofPaneId: "t-claude", dir: "row", ratio: 0.6, key: "t-claude|row|0.6" },
    });
    expect(openAtFor(s, tabs[2])).toEqual({
      place: { ofPaneId: "t-terminal", dir: "column", ratio: 0.5, key: "t-terminal|column|0.5" },
    });
  });

  it("gives tabs with the same place the same key so they share a group", () => {
    const place = { direction: "right", of: "claude", ratio: 0.5 } as const;
    const tabs = [tab("claude"), tab("a", { place }), tab("b", { place })];
    const s = snap(tabs);
    expect(openAtFor(s, tabs[1])?.place?.key).toBe(openAtFor(s, tabs[2])?.place?.key);
  });

  it("opens a tab without place beside its parent, a top-level one in the main group", () => {
    const tabs = [tab("claude"), tab("child", { parent: "claude" }), tab("other"), tab("x2")];
    const s = snap(tabs);
    expect(openAtFor(s, tabs[1])).toEqual({ besidePaneId: "t-claude" });
    expect(openAtFor(s, tabs[2])).toEqual({ main: true });
  });

  it("ignores an unknown of and keeps the plain behavior for a space without layout tabs", () => {
    const lost = tab("lost", {
      parent: "claude",
      place: { direction: "right", of: "nowhere", ratio: 0.5 },
    });
    const s = snap([tab("claude"), lost]);
    expect(openAtFor(s, lost)).toEqual({ besidePaneId: "t-claude" });
    const plain = [tab("a"), tab("b")];
    expect(openAtFor(snap(plain), plain[0])).toBeUndefined();
  });
});
