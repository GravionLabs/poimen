import { describe, expect, it } from "vitest";
import { clampMenu, menuTarget, plainGroups } from "./menu-model";

describe("clampMenu", () => {
  const size = { width: 100, height: 80 };
  const view = { width: 400, height: 300 };

  it("keeps the point when the menu fits", () => {
    expect(clampMenu({ x: 50, y: 60 }, size, view)).toEqual({ left: 50, top: 60 });
  });

  it("moves a menu that would leave the right or bottom edge back inside", () => {
    expect(clampMenu({ x: 390, y: 295 }, size, view)).toEqual({ left: 296, top: 216 });
  });

  it("never goes above or left of the margin", () => {
    expect(clampMenu({ x: -20, y: -5 }, size, view)).toEqual({ left: 4, top: 4 });
    expect(clampMenu({ x: 10, y: 10 }, { width: 500, height: 400 }, view)).toEqual({
      left: 4,
      top: 4,
    });
  });

  it("ends at the point when aligned right, and still stays inside", () => {
    expect(clampMenu({ x: 300, y: 10 }, size, view, true)).toEqual({ left: 200, top: 10 });
    expect(clampMenu({ x: 50, y: 10 }, size, view, true)).toEqual({ left: 4, top: 10 });
  });
});

describe("menuTarget", () => {
  it("moves with the arrows, wraps and jumps to Home and End", () => {
    const labels = ["A", "B", "C"];
    expect(menuTarget(labels, -1, "ArrowDown")).toBe(0);
    expect(menuTarget(labels, 2, "ArrowDown")).toBe(0);
    expect(menuTarget(labels, 0, "ArrowUp")).toBe(2);
    expect(menuTarget(labels, 1, "Home")).toBe(0);
    expect(menuTarget(labels, 1, "End")).toBe(2);
    expect(menuTarget([], 0, "ArrowDown")).toBeNull();
  });
});

describe("plainGroups", () => {
  it("is one group without a heading", () => {
    const groups = plainGroups([{ id: "a", label: "A" }]);
    expect(groups).toHaveLength(1);
    expect(groups[0].label ?? null).toBeNull();
  });
});
