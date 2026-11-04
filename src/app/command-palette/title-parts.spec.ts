import { describe, expect, it } from "vitest";
import { titleParts } from "./title-parts";

describe("titleParts", () => {
  it("merges neighboring characters into runs", () => {
    expect(titleParts("Close Tab", [0, 1, 6])).toEqual([
      { text: "Cl", hit: true },
      { text: "ose ", hit: false },
      { text: "T", hit: true },
      { text: "ab", hit: false },
    ]);
  });

  it("returns one plain run without positions and nothing for an empty title", () => {
    expect(titleParts("Refresh", [])).toEqual([{ text: "Refresh", hit: false }]);
    expect(titleParts("", [])).toEqual([]);
  });
});
