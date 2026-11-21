import { describe, expect, it } from "vitest";
import { TAB_MAX_WIDTH, TAB_MIN_WIDTH, dropSide, tabWidth } from "./tab-bar-rules";

describe("tabWidth", () => {
  it("shares the strip between the tabs", () => {
    expect(tabWidth(1000, 5)).toBe(200);
  });

  it("caps a tab at the maximum when there is room", () => {
    expect(tabWidth(1600, 2)).toBe(TAB_MAX_WIDTH);
    expect(tabWidth(1000, 0)).toBe(TAB_MAX_WIDTH);
  });

  it("never goes below the minimum, so the strip scrolls", () => {
    expect(tabWidth(1000, 8)).toBe(TAB_MIN_WIDTH);
    expect(tabWidth(0, 3)).toBe(TAB_MIN_WIDTH);
  });

  it("does not scroll while all tabs fit at the minimum", () => {
    const width = tabWidth(1152, 8);
    expect(width * 8).toBeLessThanOrEqual(1152);
  });
});

describe("dropSide", () => {
  it("puts the bar after the tab when dragging from the left", () => {
    expect(dropSide(0, 2)).toBe("after");
  });

  it("puts the bar before the tab when dragging from the right", () => {
    expect(dropSide(3, 1)).toBe("before");
  });

  it("shows no bar over the dragged tab or without a drag", () => {
    expect(dropSide(2, 2)).toBeNull();
    expect(dropSide(null, 2)).toBeNull();
    expect(dropSide(2, null)).toBeNull();
  });
});
