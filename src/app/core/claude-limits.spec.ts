import { Injector, signal } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { ClaudeLimits, MIN_GAP_MS, limitsItem, tightest, type LimitWindow } from "./claude-limits";
import { DEFAULT_SETTINGS, SettingsStore } from "./settings";
import { TauriBridge } from "./tauri";

const w = (label: string, usedPercent: number | null, resetsAt: string | null = null) =>
  ({ label, usedPercent, resetsAt }) satisfies LimitWindow;

function setup(invoke: () => Promise<LimitWindow[]>, claude = true) {
  const fn = vi.fn(invoke);
  const injector = Injector.create({
    providers: [
      { provide: ClaudeLimits, useClass: ClaudeLimits },
      { provide: TauriBridge, useValue: { invoke: fn } },
      {
        provide: SettingsStore,
        useValue: { settings: signal({ ...DEFAULT_SETTINGS, usage: { claude } }) },
      },
    ],
  });
  return { limits: injector.get(ClaudeLimits), invoke: fn };
}

describe("limitsItem", () => {
  it("shows the tightest window and lists all in the tooltip", () => {
    const item = limitsItem([
      w("5h", 62.4, "2026-11-18T20:00:00Z"),
      w("7d", 30),
      w("7d Opus", null),
    ]);
    expect(item?.text).toBe("5h 62 %");
    expect(item?.full).toContain("5h 62 %");
    expect(item?.warning).toBe(false);
    expect(item?.title).toContain("7d: 30 %, reset unknown");
    expect(item?.title).toContain("7d Opus: unknown");
    expect(item?.title).toContain("5h: 62 %, resets ");
  });

  it("warns from 80 %", () => {
    expect(limitsItem([w("5h", 79.9)])?.warning).toBe(false);
    expect(limitsItem([w("5h", 20), w("7d", 80)])).toMatchObject({
      text: "7d 80 %",
      warning: true,
    });
  });

  it("hides without a known use", () => {
    expect(limitsItem([])).toBeNull();
    expect(limitsItem([w("5h", null)])).toBeNull();
    expect(tightest([w("a", 1), w("b", 2)])?.label).toBe("b");
  });
});

describe("ClaudeLimits", () => {
  it("sets the item from the windows", async () => {
    const { limits } = setup(async () => [w("5h", 90)]);
    await limits.refresh(1_000_000);
    expect(limits.item()).toMatchObject({ text: "5h 90 %", warning: true });
  });

  it("asks at most once a minute", async () => {
    const { limits, invoke } = setup(async () => [w("5h", 10)]);
    await limits.refresh(1_000_000);
    await limits.refresh(1_000_000 + MIN_GAP_MS - 1);
    expect(invoke).toHaveBeenCalledTimes(1);
    await limits.refresh(1_000_000 + MIN_GAP_MS);
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it("hides the item on an error and reports each cause once", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    let fail = false;
    const { limits } = setup(async () => {
      if (fail) throw "no Claude Code login found";
      return [w("5h", 10)];
    });
    await limits.refresh(1_000_000);
    expect(limits.item()).not.toBeNull();
    fail = true;
    await limits.refresh(2_000_000);
    await limits.refresh(3_000_000);
    expect(limits.item()).toBeNull();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn.mock.calls[0][0]).toContain("no Claude Code login found");
    warn.mockRestore();
  });

  it("asks nothing while usage.claude is off", async () => {
    const { limits, invoke } = setup(async () => [w("5h", 10)], false);
    await limits.refresh(1_000_000);
    expect(invoke).not.toHaveBeenCalled();
    expect(limits.item()).toBeNull();
  });
});
