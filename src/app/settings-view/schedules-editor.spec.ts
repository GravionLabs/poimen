// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { ScheduleList } from "../core/schedules-model";
import { tauri } from "../core/tauri.mock";
import { SchedulesEditor } from "./schedules-editor";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

afterEach(() => TestBed.resetTestingModule());

const LIST: ScheduleList = {
  paused: false,
  schedules: [
    {
      key: "/repo|loop|0 9 * * *|UTC",
      cron: "0 9 * * *",
      timezone: "UTC",
      dir: "/repo",
      mode: "loop",
      enabled: true,
      next_ms: new Date(2026, 0, 11, 9, 0).getTime(),
      last: {
        outcome: "skipped",
        at_ms: new Date(2026, 0, 10, 9, 0).getTime(),
        reason: "paused",
      },
    },
    {
      key: "/other|loop|0 9 * * *|UTC",
      cron: "0 9 * * *",
      timezone: "UTC",
      dir: "/other",
      mode: "loop",
      enabled: false,
    },
  ],
};

async function setup(answer: () => unknown = () => LIST) {
  tauri.reset();
  tauri.answer("schedules_list", answer);
  TestBed.resetTestingModule();
  TestBed.overrideComponent(SchedulesEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/settings-view/schedules-editor.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(SchedulesEditor);
  await fixture.whenStable();
  fixture.detectChanges();
  return fixture;
}

function texts(host: HTMLElement, id: string): string[] {
  return [...host.querySelectorAll(`[data-testid=${id}]`)].map((e) => e.textContent!.trim());
}

describe("SchedulesEditor", () => {
  it("lists each schedule with its next run and last result", async () => {
    const host = (await setup()).nativeElement as HTMLElement;
    expect(texts(host, "schedule-next")).toEqual(["Next: 2026-01-11 09:00", "Next: Off"]);
    expect(texts(host, "schedule-last")).toEqual([
      "Last: skipped: paused (2026-01-10 09:00)",
      "Last: No run yet",
    ]);
  });

  it("switches a schedule off and reads the list again", async () => {
    const fixture = await setup();
    const box = (fixture.nativeElement as HTMLElement).querySelector<HTMLInputElement>(
      '[aria-label="Run loop in /repo"]',
    )!;
    expect(box.checked).toBe(true);
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    await fixture.whenStable();
    expect(tauri.calls("set_schedule_enabled")).toEqual([
      { key: "/repo|loop|0 9 * * *|UTC", enabled: false },
    ]);
    expect(tauri.calls("schedules_list").length).toBe(2);
  });

  it("pauses all schedules", async () => {
    const fixture = await setup();
    const box = (fixture.nativeElement as HTMLElement).querySelector<HTMLInputElement>(
      "[name=schedules-paused]",
    )!;
    box.checked = true;
    box.dispatchEvent(new Event("change"));
    await fixture.whenStable();
    expect(tauri.calls("set_schedules_paused")).toEqual([{ paused: true }]);
  });

  it("puts a switch back that could not be written", async () => {
    const fixture = await setup();
    tauri.answer("set_schedules_paused", () => {
      throw "triggers is not an object";
    });
    const host = fixture.nativeElement as HTMLElement;
    const box = host.querySelector<HTMLInputElement>("[name=schedules-paused]")!;
    box.checked = true;
    box.dispatchEvent(new Event("change"));
    await fixture.whenStable();
    await vi.waitFor(() => expect(tauri.calls("schedules_list").length).toBe(2));
    await fixture.whenStable();
    fixture.detectChanges();
    expect(box.checked).toBe(false);
    expect(host.querySelector("[role=alert]")?.textContent).toContain("triggers is not an object");
  });

  it("says why the list cannot be read", async () => {
    const host = (
      await setup(() => {
        throw "unknown method schedule.list";
      })
    ).nativeElement as HTMLElement;
    expect(host.querySelector("[role=alert]")?.textContent).toContain(
      "unknown method schedule.list",
    );
  });
});
