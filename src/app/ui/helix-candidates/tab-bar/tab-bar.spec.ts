import { describe, expect, it } from "vitest";
import { arrowTarget, dropMove } from "./tab-bar";

describe("dropMove", () => {
  it("moves the dragged tab to the drop index", () => {
    expect(dropMove(0, 2)).toEqual({ from: 0, to: 2 });
    expect(dropMove(3, 1)).toEqual({ from: 3, to: 1 });
  });

  it("is null without a drag or when dropped on itself", () => {
    expect(dropMove(null, 1)).toBeNull();
    expect(dropMove(1, 1)).toBeNull();
  });
});

describe("arrowTarget", () => {
  it("steps right and left, wrapping around", () => {
    expect(arrowTarget("ArrowRight", 1, 3)).toBe(2);
    expect(arrowTarget("ArrowRight", 2, 3)).toBe(0);
    expect(arrowTarget("ArrowLeft", 0, 3)).toBe(2);
  });

  it("ignores other keys and an empty bar", () => {
    expect(arrowTarget("Enter", 0, 3)).toBeNull();
    expect(arrowTarget("ArrowRight", 0, 0)).toBeNull();
  });
});
