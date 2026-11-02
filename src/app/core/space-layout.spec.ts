import { describe, expect, it } from "vitest";
import {
  addPane,
  isValidLayout,
  layoutErrors,
  movePane,
  parseSpaceLayout,
  removePane,
  sameLayout,
  toConfig,
  updatePane,
  type PaneSpec,
} from "./space-layout";

const layout: PaneSpec[] = [
  { name: "agent", command: "claude", split: null },
  { name: "tests", command: null, split: { from: "agent", direction: "right", ratio: 0.6 } },
  { name: "shell", command: null, split: { from: "tests", direction: "down", ratio: 0.5 } },
];

describe("space layout", () => {
  it("parses the backend shape and rejects malformed input", () => {
    const parsed = parseSpaceLayout({
      panes: [
        { name: "a", command: "nvim", split: null },
        { name: "b", command: null, split: { from: "a", direction: "down", ratio: 0.7 } },
      ],
    });
    expect(parsed?.panes[1].split).toEqual({ from: "a", direction: "down", ratio: 0.7 });
    expect(parseSpaceLayout(null)).toBeNull();
    expect(parseSpaceLayout({ panes: [] })).toBeNull();
    expect(parseSpaceLayout({ panes: [{ command: "x" }] })).toBeNull();
  });

  it("validates names, splits and ratios like layout.rs", () => {
    expect(isValidLayout(layout)).toBe(true);
    const bad: PaneSpec[] = [
      { name: " ", command: null, split: null },
      { name: "a", command: null, split: { from: "a", direction: "right", ratio: 0.5 } },
      { name: "b", command: null, split: { from: "nope", direction: "right", ratio: 0.5 } },
      { name: "a", command: null, split: { from: "b", direction: "down", ratio: 0.5 } },
      { name: "c", command: null, split: { from: "b", direction: "down", ratio: 0.95 } },
    ];
    expect(layoutErrors(bad)).toEqual([
      "A name is required",
      "Split from must be an earlier pane",
      "Split from must be an earlier pane",
      'Duplicate name "a"',
      "Ratio must be from 0.1 to 0.9",
    ]);
    expect(isValidLayout([])).toBe(false);
  });

  it("adds a shell pane split down from the last one with a free name", () => {
    const next = addPane(addPane(layout));
    expect(next.slice(3).map((p) => [p.name, p.split?.from])).toEqual([
      ["pane", "shell"],
      ["pane-2", "pane"],
    ]);
    expect(addPane([])[0].split).toBeNull();
  });

  it("removes a pane and repoints splits from it, never the last pane", () => {
    const next = removePane(layout, 1);
    expect(next.map((p) => p.name)).toEqual(["agent", "shell"]);
    expect(next[1].split?.from).toBe("agent");
    const rootGone = removePane(layout, 0);
    expect(rootGone[0]).toMatchObject({ name: "tests", split: null });
    expect(rootGone[1].split?.from).toBe("tests");
    expect(removePane([layout[0]], 0)).toHaveLength(1);
  });

  it("moves panes: the new first pane loses its split, the old root gets one", () => {
    const next = movePane(layout, 1, -1);
    expect(next[0]).toMatchObject({ name: "tests", split: null });
    expect(next[1]).toMatchObject({ name: "agent", split: { from: "tests" } });
    expect(movePane(layout, 2, 1)).toEqual(layout);
    const later = movePane(layout, 2, -1);
    expect(layoutErrors(later)[1]).toBe("Split from must be an earlier pane");
  });

  it("renames splits together with their pane", () => {
    const next = updatePane(layout, 0, { name: "main" });
    expect(next[1].split?.from).toBe("main");
    const duplicate = updatePane(layout, 2, { name: "agent" });
    expect(updatePane(duplicate, 2, { name: "shell" })[1].split?.from).toBe("agent");
    expect(updatePane(layout, 2, { split: { ratio: 0.3 } })[2].split?.ratio).toBe(0.3);
    expect(updatePane(layout, 0, { split: { ratio: 0.3 } })[0].split).toBeNull();
  });

  it("writes trimmed names, no empty command and no split on the first pane", () => {
    const panes: PaneSpec[] = [
      { name: " a ", command: "  ", split: null },
      { name: "b", command: " htop ", split: { from: "a", direction: "down", ratio: 0.333 } },
    ];
    expect(toConfig(panes)).toEqual({
      panes: [
        { name: "a" },
        { name: "b", command: "htop", split: { from: "a", direction: "down", ratio: 0.33 } },
      ],
    });
    expect(
      sameLayout(
        panes,
        panes.map((p) => ({ ...p })),
      ),
    ).toBe(true);
    expect(sameLayout(panes, layout)).toBe(false);
  });
});
