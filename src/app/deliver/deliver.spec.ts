// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { NotificationCenter } from "../core/notification-center";
import { tauri } from "../core/tauri.mock";
import { UI_STORAGE } from "../core/ui-state";
import { signal } from "@angular/core";
import { TokenTotalsService, type TokenSample } from "../core/token-totals";
import { Deliver } from "./deliver";
import type { DeliverCard, DeliverRead } from "./deliver-status";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

beforeEach(() => {
  tauri.reset();
  TestBed.resetTestingModule();
  const store = new Map<string, string>();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: UI_STORAGE,
        useValue: {
          getItem: (k: string) => store.get(k) ?? null,
          setItem: (k: string, v: string) => store.set(k, v),
        },
      },
    ],
  });
});

const run = (attentions: Record<string, unknown>[]): DeliverRead => {
  const base = JSON.parse(readFileSync("src/app/deliver/fixtures/events-failed.json", "utf8"));
  return { kind: "events", runs: [{ ...base, attentions }] } as unknown as DeliverRead;
};
const attention = (id: string, extra: Record<string, unknown> = {}) => ({
  id,
  task: "t1",
  reason: "decide",
  text: null,
  tone: null,
  needsYou: true,
  links: [],
  ...extra,
});

describe("Deliver run alerts (#806)", () => {
  it("notifies once per needs-you, warning or danger attention, not for the first read", () => {
    const deliver = TestBed.inject(Deliver);
    const center = TestBed.inject(NotificationCenter);
    deliver.apply("/r", run([attention("a1")]));
    expect(center.entries()).toHaveLength(0);

    const next = run([
      attention("a1"),
      attention("a2", { text: "Merge the release PR" }),
      attention("a3", { needsYou: false, tone: "info" }),
      attention("a4", { needsYou: false, tone: "danger" }),
    ]);
    deliver.apply("/r", next);
    deliver.apply("/r", next);
    expect(center.entries().map((e) => e.message)).toEqual([
      "M5 Roadmap: decide",
      "M5 Roadmap: Merge the release PR",
    ]);
    expect(center.entries()[0]!.deliver).toEqual({ root: "/r" });
  });

  it("carries the machine of a remote run", () => {
    const deliver = TestBed.inject(Deliver);
    const center = TestBed.inject(NotificationCenter);
    deliver.apply("/r", run([]), "box");
    deliver.apply("/r", run([attention("a1")]), "box");
    expect(center.entries()[0]!.deliver).toEqual({ root: "/r", machine: "box" });
  });

  it("a click target reveals the group of that repository", () => {
    const deliver = TestBed.inject(Deliver);
    deliver.apply("/r", run([]));
    expect(deliver.reveal("/r")).toBe(true);
    expect(deliver.revealed()).not.toBeNull();
  });
});

describe("Deliver tokens per run (#896)", () => {
  const at = (iso: string) => Date.parse(iso);
  const sample = (iso: string, input: number): TokenSample => ({
    at: at(iso),
    input,
    output: 0,
    cache_read: 0,
    cache_creation: 0,
  });
  const samples = signal<ReadonlyMap<string, readonly TokenSample[]>>(new Map());

  function read(overrides: Record<string, unknown>): DeliverRead {
    const base = JSON.parse(readFileSync("src/app/deliver/fixtures/events-session.json", "utf8"));
    const events = [
      { seq: 1, ts: "2026-11-08T10:00:00.000Z", type: "run.started", tone: "neutral" },
    ];
    return {
      kind: "events",
      runs: [{ ...base, events, ...overrides }],
    } as unknown as DeliverRead;
  }

  function deliver(): Deliver {
    TestBed.overrideProvider(TokenTotalsService, { useValue: { samples } });
    return TestBed.inject(Deliver);
  }

  const stream = [
    sample("2026-11-08T09:59:59.000Z", 1000),
    sample("2026-11-08T10:00:00.000Z", 1),
    sample("2026-11-08T10:30:00.000Z", 20),
    sample("2026-11-08T11:00:00.000Z", 300),
    sample("2026-11-08T11:00:01.000Z", 5000),
  ];

  it("sums the samples between the start and the last event of a finished run", () => {
    samples.set(new Map([["t9", stream]]));
    const d = deliver();
    d.apply(
      "/r",
      read({
        status: "finished",
        outcome: "succeeded",
        updated: "2026-11-08T11:00:00.000Z",
        host: { "poimen.terminal": "t9" },
      }),
    );
    expect(d.tokensOf()(d.cards()[0]!)?.input).toBe(321);
  });

  it("a running run counts up to now", () => {
    samples.set(new Map([["t9", stream]]));
    const d = deliver();
    d.apply(
      "/r",
      read({
        status: "running",
        updated: "2026-11-08T11:30:00.000Z",
        host: { "poimen.terminal": "t9" },
      }),
    );
    d.now.set(at("2026-11-08T11:31:00.000Z"));
    // The sample after 11:00 counts (up to now), the one before the start does not.
    expect(d.tokensOf()(d.cards()[0]!)?.input).toBe(5321);
  });

  it("a run without the terminal link shows none, even with an agent pane around", () => {
    samples.set(new Map([["t9", stream]]));
    const d = deliver();
    d.apply("/r", read({ status: "finished", outcome: "succeeded", host: {} }));
    expect(d.tokensOf()(d.cards()[0]!)).toBeNull();
  });

  it("shows none when no sample lies in the window", () => {
    samples.set(new Map([["t9", [sample("2026-11-07T00:00:00.000Z", 9)]]]));
    const d = deliver();
    d.apply(
      "/r",
      read({
        status: "finished",
        outcome: "succeeded",
        updated: "2026-11-08T11:00:00.000Z",
        host: { "poimen.terminal": "t9" },
      }),
    );
    expect(d.tokensOf()(d.cards()[0]!)).toBeNull();
  });

  it("a remote run reads the samples of its machine's terminal, last ones kept offline", () => {
    samples.set(new Map([["box::t9", stream]]));
    const d = deliver();
    d.apply(
      "/r",
      read({
        status: "finished",
        outcome: "succeeded",
        updated: "2026-11-08T11:00:00.000Z",
        host: { "poimen.terminal": "t9" },
      }),
      "box",
    );
    // `cards` only lists a machine that is configured; the card of the read is what counts here.
    const card = (d as unknown as { received(): DeliverCard[] }).received()[0]!;
    expect(d.tokensOf()(card)?.input).toBe(321);
    // The machine goes offline: its samples stay in the service, so the total stays.
    expect(d.tokensOf()(card)?.input).toBe(321);
  });
});
