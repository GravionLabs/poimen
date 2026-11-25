// @vitest-environment jsdom
import type { DestroyRef } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { NotificationCenter } from "./notification-center";
import { Schedules } from "./schedules";
import type { Snapshot } from "./snapshot";
import { SpaceActions } from "./space-actions";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

const notify = vi.fn();
const fail = vi.fn();

beforeEach(() => {
  tauri.reset();
  TestBed.configureTestingModule({
    providers: [{ provide: SpaceActions, useValue: { notify, fail } }],
  });
});

afterEach(() => {
  TestBed.resetTestingModule();
  vi.clearAllMocks();
});

const alert = (seq: number) => ({
  id: seq,
  seq,
  key: "k",
  dir: "/repo",
  mode: "loop",
  outcome: "failed",
  reason: "exit code 1",
  at_ms: seq,
  count: 1,
});

const schedules = () => TestBed.inject(Schedules);
const entries = () => TestBed.inject(NotificationCenter).entries();

describe("Schedules.readAlerts", () => {
  it("adds the alerts to the center and acknowledges what it added, without a toast", async () => {
    tauri.answer("schedules_alerts", () => ({ alerts: [alert(3), alert(7)] }));
    await schedules().readAlerts();
    expect(entries().map((e) => e.time)).toEqual([7, 3]);
    expect(tauri.calls("schedules_ack_alerts")).toEqual([{ upTo: 7 }]);
    expect(notify).not.toHaveBeenCalled();
    expect(fail).not.toHaveBeenCalled();
  });

  it("acknowledges nothing without an alert", async () => {
    tauri.answer("schedules_alerts", () => ({ alerts: [] }));
    await schedules().readAlerts();
    expect(entries()).toEqual([]);
    expect(tauri.calls("schedules_ack_alerts")).toEqual([]);
  });

  it("is read at the start of every server session, until the owner is destroyed", async () => {
    tauri.answer("schedules_alerts", () => ({ alerts: [alert(1)] }));
    const listeners = new Set<(snapshot: Snapshot) => void>();
    const store = {
      onSessionStart: (listener: (snapshot: Snapshot) => void) => {
        listeners.add(listener);
        return () => void listeners.delete(listener);
      },
    };
    const destroy: (() => void)[] = [];
    const owner = {
      onDestroy: (fn: () => void) => {
        destroy.push(fn);
        return () => {};
      },
    } as DestroyRef;
    schedules().followSessions(store, owner);
    expect(tauri.calls("schedules_alerts")).toEqual([]);
    listeners.forEach((l) => l({} as Snapshot));
    await vi.waitFor(() => expect(entries()).toHaveLength(1));
    destroy.forEach((fn) => fn());
    expect(listeners.size).toBe(0);
  });

  it("is read with every refresh of the list", async () => {
    tauri.answer("schedules_list", () => ({ paused: false, schedules: [] }));
    tauri.answer("schedules_alerts", () => ({ alerts: [alert(1)] }));
    await schedules().refresh();
    await vi.waitFor(() => expect(tauri.calls("schedules_ack_alerts")).toEqual([{ upTo: 1 }]));
    expect(entries()).toHaveLength(1);
  });

  it("reads once while a read is running", async () => {
    tauri.answer("schedules_alerts", () => ({ alerts: [alert(1)] }));
    await Promise.all([schedules().readAlerts(), schedules().readAlerts()]);
    expect(tauri.calls("schedules_alerts")).toHaveLength(1);
  });

  it("adds no entry twice when the acknowledgement failed", async () => {
    tauri.answer("schedules_alerts", () => ({ alerts: [alert(1)] }));
    tauri.answer("schedules_ack_alerts", () => Promise.reject("no server"));
    await schedules().readAlerts();
    await schedules().readAlerts();
    expect(entries()).toHaveLength(1);
    expect(tauri.calls("schedules_ack_alerts")).toHaveLength(2);
  });

  it("stays quiet when the alerts cannot be read", async () => {
    tauri.answer("schedules_alerts", () => Promise.reject("no server"));
    await schedules().readAlerts();
    expect(entries()).toEqual([]);
    expect(schedules().error()).toBeNull();
    expect(fail).not.toHaveBeenCalled();
  });
});
