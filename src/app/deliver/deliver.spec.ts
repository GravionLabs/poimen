// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { NotificationCenter } from "../core/notification-center";
import { tauri } from "../core/tauri.mock";
import { UI_STORAGE } from "../core/ui-state";
import { Deliver } from "./deliver";
import type { DeliverRead } from "./deliver-status";

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
