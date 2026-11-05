// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Attention } from "./attention";
import { DockBadge } from "./dock-badge";
import { TauriBridge } from "./tauri";

function setup(fail = false) {
  const setBadgeCount = vi.fn<(count: number | undefined) => Promise<void>>(() =>
    fail ? Promise.reject(new Error("unsupported")) : Promise.resolve(),
  );
  TestBed.configureTestingModule({
    providers: [{ provide: TauriBridge, useValue: { setBadgeCount } }],
  });
  return { badge: TestBed.inject(DockBadge), setBadgeCount };
}

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

describe("DockBadge", () => {
  afterEach(() => TestBed.resetTestingModule());

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

  it("follows the Attention count: blocked ring plus unread done, cleared once viewed", () => {
    const { setBadgeCount } = setup();
    const attention = TestBed.inject(Attention);
    const st = (o: Record<string, "working" | "blocked" | "done">) => new Map(Object.entries(o));
    attention.observe(st({ a: "working", b: "working" }), new Set());
    attention.observe(st({ a: "blocked", b: "done" }), new Set());
    TestBed.tick();
    expect(attention.count()).toBe(2);
    expect(setBadgeCount.mock.calls.at(-1)?.[0]).toBe(2);
    attention.observe(st({ a: "blocked", b: "done" }), new Set(["a", "b"]));
    TestBed.tick();
    expect(attention.count()).toBe(0);
    expect(setBadgeCount.mock.calls.at(-1)?.[0]).toBeUndefined();
  });
});
