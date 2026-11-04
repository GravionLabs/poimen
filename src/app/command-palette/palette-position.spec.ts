import { describe, expect, it } from "vitest";
import { palettePosition } from "./palette-position";

const viewport = { width: 1200, height: 800 };

describe("palettePosition", () => {
  it("centres on the editor area right of a 300px sidebar", () => {
    const area = { left: 348, top: 40, width: 852, height: 700 };
    // left 348 + (852 - 480) / 2, top 40 + (700 - 320) / 2
    expect(palettePosition(area, viewport)).toEqual({ left: 534, top: 230 });
  });

  it("falls back to the window centre without an editor area", () => {
    expect(palettePosition(null, viewport)).toEqual({ left: 360, top: 240 });
  });

  it("uses the maximum height as basis, so the list length cannot move the top edge", () => {
    const area = { left: 0, top: 0, width: 1200, height: 800 };
    expect(palettePosition(area, viewport, 320).top).toBe(240);
  });

  it("stays inside a small window", () => {
    const small = { width: 300, height: 200 };
    const pos = palettePosition({ left: 200, top: 100, width: 100, height: 100 }, small);
    expect(pos).toEqual({ left: 22, top: 38 });
  });
});
