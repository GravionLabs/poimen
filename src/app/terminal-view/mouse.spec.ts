import { describe, expect, it } from "vitest";
import { buttonName, cellAt, modifierBits } from "./mouse";

const rect = { left: 100, top: 50, width: 800, height: 480 }; // 80x24 cells of 10x20 px

describe("cellAt", () => {
  it("maps a point to the zero-based cell under it", () => {
    expect(cellAt(rect, 80, 24, 100, 50)).toEqual({ column: 0, row: 0 });
    expect(cellAt(rect, 80, 24, 225, 95)).toEqual({ column: 12, row: 2 });
  });

  it("clamps points outside the grid, as during a drag out of the terminal", () => {
    expect(cellAt(rect, 80, 24, 0, 0)).toEqual({ column: 0, row: 0 });
    expect(cellAt(rect, 80, 24, 5000, 5000)).toEqual({ column: 79, row: 23 });
  });

  it("measures in xterm cells and clamps to a smaller frame grid", () => {
    const frame = { cols: 60, rows: 10 };
    expect(cellAt(rect, 80, 24, 225, 95, frame)).toEqual({ column: 12, row: 2 });
    expect(cellAt(rect, 80, 24, 890, 520, frame)).toEqual({ column: 59, row: 9 });
  });

  it("falls back to the origin for an empty rect", () => {
    expect(cellAt({ left: 0, top: 0, width: 0, height: 0 }, 80, 24, 10, 10)).toEqual({
      column: 0,
      row: 0,
    });
  });
});

describe("modifierBits and buttonName", () => {
  it("encodes Shift = 1, Ctrl = 2, Alt = 4", () => {
    expect(modifierBits({ shiftKey: false, ctrlKey: false, altKey: false })).toBe(0);
    expect(modifierBits({ shiftKey: true, ctrlKey: false, altKey: true })).toBe(5);
    expect(modifierBits({ shiftKey: true, ctrlKey: true, altKey: true })).toBe(7);
  });

  it("names the DOM buttons", () => {
    expect([0, 1, 2, 9].map(buttonName)).toEqual(["left", "middle", "right", "left"]);
  });
});
