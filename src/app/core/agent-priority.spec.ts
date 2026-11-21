import { describe, expect, it } from "vitest";
import { formatElapsed, silentLabel, withSilent } from "./agent-priority";

describe("formatElapsed", () => {
  const MIN = 60_000;
  it.each([
    [0, "now"],
    [59_999, "now"],
    [-5000, "now"],
    [Number.NaN, "now"],
    [MIN, "1m"],
    [5 * MIN + 30_000, "5m"],
    [59 * MIN, "59m"],
    [60 * MIN, "1h"],
    [2 * 60 * MIN + 59 * MIN, "2h"],
    [24 * 60 * MIN, "1d"],
    [3 * 24 * 60 * MIN + 5 * 60 * MIN, "3d"],
  ])("%d ms is %s", (ms, text) => {
    expect(formatElapsed(ms)).toBe(text);
  });
});

describe("silentLabel (#899)", () => {
  it("names the elapsed time and is absent when cleared", () => {
    expect(silentLabel(1_000, 1_000 + 12 * 60_000)).toBe("silent 12m");
    expect(silentLabel(undefined, 5)).toBeUndefined();
  });
});

describe("withSilent (#1149)", () => {
  it("adds the label to the tooltip of a silent tab and leaves a working one alone", () => {
    expect(withSilent("ws / agent", 1_000, 1_000 + 12 * 60_000)).toBe("ws / agent · silent 12m");
    expect(withSilent("ws / agent", undefined, 5)).toBe("ws / agent");
  });
});
