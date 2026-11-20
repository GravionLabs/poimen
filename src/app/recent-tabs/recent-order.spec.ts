import { describe, expect, it } from "vitest";
import { holdKey, stepIndex, switchList, touch } from "./recent-order";

describe("recent order", () => {
  it("moves a used tab to the front", () => {
    expect(touch(["a", "b", "c"], "c")).toEqual(["c", "a", "b"]);
    expect(touch([], "a")).toEqual(["a"]);
  });

  it("lists used tabs first, drops closed ones and appends never-used ones", () => {
    expect(switchList(["c", "x", "a"], ["a", "b", "c"])).toEqual(["c", "a", "b"]);
  });

  it("steps with wrap-around in both directions", () => {
    expect(stepIndex(0, -1, 3)).toBe(2);
    expect(stepIndex(2, 1, 3)).toBe(0);
    expect(stepIndex(0, 1, 0)).toBe(0);
  });

  it("holds the first modifier of the chord", () => {
    expect(holdKey({ ctrl: true, alt: false, meta: false })).toBe("Control");
    expect(holdKey({ ctrl: false, alt: true, meta: false })).toBe("Alt");
    expect(holdKey({ ctrl: false, alt: false, meta: false })).toBeNull();
  });
});
