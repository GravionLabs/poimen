import { Injector } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { DockBadge } from "./dock-badge";
import { TauriBridge } from "./tauri";

function setup(fail = false) {
  const setBadgeCount = vi.fn<(count: number | undefined) => Promise<void>>(() =>
    fail ? Promise.reject(new Error("unsupported")) : Promise.resolve(),
  );
  const injector = Injector.create({
    providers: [{ provide: TauriBridge, useValue: { setBadgeCount } }, DockBadge],
  });
  return { badge: injector.get(DockBadge), setBadgeCount };
}

describe("DockBadge", () => {
  it("shows the waiting count and follows changes", () => {
    const { badge, setBadgeCount } = setup();
    badge.set(2);
    badge.set(3);
    expect(setBadgeCount.mock.calls.map(([c]) => c)).toEqual([2, 3]);
  });

  it("clears the badge at 0", () => {
    const { badge, setBadgeCount } = setup();
    badge.set(1);
    badge.set(0);
    expect(setBadgeCount.mock.calls.map(([c]) => c)).toEqual([1, undefined]);
  });

  it("hides the badge when the count falls to zero after growing", () => {
    const { badge, setBadgeCount } = setup();
    badge.set(3);
    badge.set(1);
    badge.set(0);
    expect(setBadgeCount.mock.calls.map(([c]) => c)).toEqual([3, 1, undefined]);
  });

  it("does not call again for an unchanged count", () => {
    const { badge, setBadgeCount } = setup();
    badge.set(1);
    badge.set(1);
    expect(setBadgeCount).toHaveBeenCalledTimes(1);
  });

  it("warns instead of throwing where badges are unsupported", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const { badge } = setup(true);
    expect(() => badge.set(1)).not.toThrow();
    await Promise.resolve();
    await Promise.resolve();
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});
