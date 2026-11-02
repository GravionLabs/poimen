import { describe, expect, it } from "vitest";
import { paneSizes } from "./split-pane";

describe("paneSizes", () => {
  it("uses the given sizes when they match the count", () => {
    expect(paneSizes([0.7, 0.3], 2)).toEqual([0.7, 0.3]);
  });

  it("falls back to equal shares", () => {
    expect(paneSizes(undefined, 4)).toEqual([0.25, 0.25, 0.25, 0.25]);
    expect(paneSizes([1], 2)).toEqual([0.5, 0.5]);
  });
});
