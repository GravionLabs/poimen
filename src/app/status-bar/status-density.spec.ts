import { describe, expect, it } from "vitest";
import {
  MAX_DENSITY,
  densityFlags,
  nextLevel,
  shortRuns,
  type DensityLevel,
} from "./status-density";

describe("densityFlags", () => {
  it("shortens in the order limits, runs, low-priority items", () => {
    expect(densityFlags(0)).toEqual({ limitsShort: false, runsShort: false, iconOnly: false });
    expect(densityFlags(1)).toEqual({ limitsShort: true, runsShort: false, iconOnly: false });
    expect(densityFlags(2)).toEqual({ limitsShort: true, runsShort: true, iconOnly: false });
    expect(densityFlags(3)).toEqual({ limitsShort: true, runsShort: true, iconOnly: true });
  });
});

describe("nextLevel", () => {
  const needed = [900, 800, 700, 600];

  it("stays while the content fits", () => {
    expect(nextLevel(0, [900], 900)).toBe(0);
  });

  it("steps down one level at a time as the bar overflows", () => {
    let level: DensityLevel = 0;
    const seen: DensityLevel[] = [];
    for (const available of [850, 750, 650, 550]) {
      level = nextLevel(level, needed, available);
      seen.push(level);
    }
    expect(seen).toEqual([1, 2, 3, 3]);
  });

  it("settles on the first level that fits", () => {
    let level: DensityLevel = 0;
    for (let i = 0; i < 6; i++) level = nextLevel(level, needed, 720);
    expect(level).toBe(2);
  });

  it("never goes past the last level", () => {
    expect(nextLevel(MAX_DENSITY, needed, 100)).toBe(MAX_DENSITY);
  });

  it("steps back up when the wider level was measured and now fits", () => {
    expect(nextLevel(2, needed, 800)).toBe(1);
    expect(nextLevel(2, needed, 799)).toBe(2);
  });

  it("does not step up to a level it never measured", () => {
    expect(nextLevel(1, [undefined, 800], 5000)).toBe(1);
  });
});

describe("shortRuns", () => {
  it("keeps the mark and the progress or the number of runs", () => {
    expect(shortRuns("▶ M5 7/19")).toBe("▶ 7/19");
    expect(shortRuns("⚠ M4 Deliver loop 3/9")).toBe("⚠ 3/9");
    expect(shortRuns("▶ 3 runs")).toBe("▶ 3");
    expect(shortRuns("▶ session")).toBe("▶");
  });
});
