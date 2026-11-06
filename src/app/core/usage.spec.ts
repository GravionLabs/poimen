import { Injector } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { TauriBridge } from "./tauri";
import {
  figures,
  localTime,
  Usage,
  usageColor,
  usageTitle,
  type UsageReport,
  type UsageWindow,
} from "./usage";

const win = (over: Partial<UsageWindow>): UsageWindow => ({
  window: "seven_day",
  unit: "percent",
  used: 54,
  limit: 100,
  resets_at: "2026-11-07T12:00:00Z",
  ...over,
});

const report = (
  windows: UsageWindow[],
  status = "ok",
  extra: Partial<UsageReport["providers"][number]> = {},
): UsageReport => ({
  schema_version: 1,
  generated_at: "2026-11-03T10:05:00Z",
  providers: [{ provider: "claude", status, windows, ...extra }],
});

const NOVEMBER = new Date(2026, 10, 3); // 30 days
const labels = (r: UsageReport | null, now = NOVEMBER) => figures(r, now).map((f) => f.label);

describe("figures", () => {
  it("computes 5h, 7d, 1d and 1m", () => {
    const r = report([
      win({ window: "five_hour", used: 12 }),
      win({ used: 54, today_usage: 4, month_usage: 4 }),
    ]);
    // 1d: 4 / (100 / 7) = 28 %; 1m: 4 / (100 * 30 / 7) = 0.93 %
    expect(labels(r)).toEqual(["5h 12%", "7d 54%", "1d 28%", "1m 1%"]);
  });

  it("uses the days of the month for 1m", () => {
    const r = report([win({ month_usage: 100 })]);
    expect(labels(r, new Date(2026, 1, 10))).toContain("1m 25%"); // 100 / (100*28/7)
    expect(labels(r, new Date(2026, 0, 10))).toContain("1m 23%"); // 100 / (100*31/7) = 22.6
  });

  it("scales with a limit other than 100", () => {
    const r = report([win({ used: 50, limit: 200, today_usage: 25 })]);
    expect(labels(r)).toEqual(["7d 25%", "1d 88%"]);
  });

  it("leaves out a figure whose source field is absent", () => {
    expect(labels(report([win({})]))).toEqual(["7d 54%"]);
    expect(labels(report([win({ today_usage: 0 })]))).toEqual(["7d 54%", "1d 0%"]);
  });

  it("leaves out windows with a limit of 0", () => {
    expect(labels(report([win({ limit: 0, today_usage: 1, month_usage: 1 })]))).toEqual([]);
  });

  it("adds Copilot's monthly requests as CP", () => {
    const r = report([win({})]);
    r.providers.push({
      provider: "copilot",
      status: "ok",
      windows: [win({ window: "monthly", unit: "requests", used: 26, limit: 300 })],
    });
    expect(labels(r)).toEqual(["7d 54%", "CP 9%"]);
  });

  it("is empty for no report and for unavailable providers", () => {
    expect(figures(null)).toEqual([]);
    expect(figures(report([], "unavailable"))).toEqual([]);
  });

  it("colors by percent, all grey when stale", () => {
    expect(usageColor(49.9)).toBe("green");
    expect(usageColor(50)).toBe("yellow");
    expect(usageColor(79.9)).toBe("yellow");
    expect(usageColor(80)).toBe("red");
    const f = figures(report([win({ used: 90 })]), NOVEMBER);
    expect(f[0].color).toBe("red");
    const stale = figures(report([win({ used: 90 })], "stale"), NOVEMBER);
    expect(stale[0].color).toBe("grey");
    expect(stale[0].label).toBe("7d 90%");
  });
});

describe("usageTitle", () => {
  it("names reset times in local time and today's budget", () => {
    const five = new Date(2026, 10, 3, 14, 30).toISOString();
    const seven = new Date(2026, 10, 7, 9, 5).toISOString();
    const r = report([
      win({ window: "five_hour", resets_at: five }),
      win({ resets_at: seven, today_usage: 4, budget: 10, ratio: 0.4 }),
    ]);
    expect(usageTitle(r).split("\n")).toEqual([
      "5h resets 11-03 14:30",
      "7d resets 11-07 09:05",
      "Today's budget: 4 of 10% (40%)",
    ]);
  });

  it("says since when a provider is stale", () => {
    const since = new Date(2026, 10, 3, 9, 0).toISOString();
    const r = report([win({})], "stale", { stale_since: since });
    expect(usageTitle(r)).toContain("Claude: stale since 11-03 09:00");
  });

  it("is empty without a report and keeps an unparsable time", () => {
    expect(usageTitle(null)).toBe("");
    expect(localTime("soon")).toBe("soon");
  });
});

describe("Usage service", () => {
  function setup(first: unknown, fail = false) {
    let handler: ((r: UsageReport | null) => void) | undefined;
    const bridge = {
      invoke: vi.fn(async () => {
        if (fail) throw new Error("no tauri");
        return first;
      }),
      listen: vi.fn(async (_e: string, h: (r: UsageReport | null) => void) => {
        handler = h;
        return () => {};
      }),
    };
    const injector = Injector.create({
      providers: [{ provide: TauriBridge, useValue: bridge }, Usage],
    });
    return { usage: injector.get(Usage), push: (r: UsageReport | null) => handler?.(r), bridge };
  }

  it("reads once, then follows the usage events", async () => {
    const r = report([win({})]);
    const { usage, push, bridge } = setup(r);
    await vi.waitFor(() => expect(usage.report()).toEqual(r));
    expect(bridge.invoke).toHaveBeenCalledWith("get_usage");
    push(null);
    expect(usage.report()).toBeNull();
  });

  it("stays hidden when the read fails", async () => {
    const { usage, bridge } = setup(null, true);
    await vi.waitFor(() => expect(bridge.invoke).toHaveBeenCalled());
    expect(usage.report()).toBeNull();
  });
});
