import { describe, expect, it } from "vitest";
import {
  groupIds,
  removeGroup,
  resizeSizes,
  setSplitSizes,
  splitGroup,
  type LayoutNode,
} from "./editor-layout";

const g = (id: number): LayoutNode => ({ kind: "group", id });

describe("splitGroup", () => {
  it("turns a lone group into a split of two halves", () => {
    expect(splitGroup(g(1), 1, 2, "row", 10)).toEqual({
      kind: "split",
      id: 10,
      dir: "row",
      children: [g(1), g(2)],
      sizes: [0.5, 0.5],
    });
  });

  it("adds a sibling in a split of the same direction and halves the source", () => {
    const row = splitGroup(g(1), 1, 2, "row", 10);
    expect(splitGroup(row, 2, 3, "row", 11)).toEqual({
      kind: "split",
      id: 10,
      dir: "row",
      children: [g(1), g(2), g(3)],
      sizes: [0.5, 0.25, 0.25],
    });
  });

  it("nests a split of the other direction", () => {
    const row = splitGroup(g(1), 1, 2, "row", 10);
    const nested = splitGroup(row, 2, 3, "column", 11);
    expect(groupIds(nested)).toEqual([1, 2, 3]);
    expect(nested).toMatchObject({
      dir: "row",
      children: [g(1), { kind: "split", id: 11, dir: "column", children: [g(2), g(3)] }],
    });
  });

  it("ignores an unknown group", () => {
    expect(splitGroup(g(1), 9, 2, "row", 10)).toEqual(g(1));
  });
});

describe("removeGroup", () => {
  it("collapses a split left with one child", () => {
    expect(removeGroup(splitGroup(g(1), 1, 2, "row", 10), 2)).toEqual(g(1));
  });

  it("gives the space to the remaining siblings", () => {
    let layout = splitGroup(g(1), 1, 2, "row", 10);
    layout = splitGroup(layout, 2, 3, "row", 11); // sizes 0.5 0.25 0.25
    expect(removeGroup(layout, 1)).toMatchObject({ sizes: [0.5, 0.5], children: [g(2), g(3)] });
  });

  it("returns null for the last group", () => {
    expect(removeGroup(g(1), 1)).toBeNull();
  });
});

describe("setSplitSizes", () => {
  const row = splitGroup(g(1), 1, 2, "row", 10);

  it("sets normalized sizes on the split", () => {
    expect(setSplitSizes(row, 10, [3, 1])).toMatchObject({ sizes: [0.75, 0.25] });
  });

  it("ignores a list of the wrong length or an unknown split", () => {
    expect(setSplitSizes(row, 10, [1])).toBe(row);
    expect(setSplitSizes(row, 99, [3, 1])).toEqual(row);
  });
});

describe("resizeSizes", () => {
  it("moves the divider between two neighbors", () => {
    const next = resizeSizes([0.5, 0.5], 0, 0.2);
    expect(next[0]).toBeCloseTo(0.7);
    expect(next[1]).toBeCloseTo(0.3);
  });

  it("keeps each neighbor above the minimum", () => {
    const high = resizeSizes([0.5, 0.5], 0, 0.9);
    expect(high[0]).toBeCloseTo(0.9);
    expect(high[1]).toBeCloseTo(0.1);
    expect(resizeSizes([0.5, 0.5], 0, -0.9)[0]).toBeCloseTo(0.1);
  });

  it("leaves other panes alone and ignores a bad divider index", () => {
    expect(resizeSizes([0.2, 0.4, 0.4], 1, 0.1)[0]).toBe(0.2);
    expect(resizeSizes([0.5, 0.5], 1, 0.1)).toEqual([0.5, 0.5]);
  });
});
