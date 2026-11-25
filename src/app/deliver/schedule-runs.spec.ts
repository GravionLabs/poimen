// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Schedules } from "../core/schedules";
import { SpacesStore } from "../core/spaces-store";
import { tauri } from "../core/tauri.mock";
import { ScheduleRuns } from "./schedule-runs";

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

async function setup(runs: unknown[]) {
  tauri.reset();
  tauri.answer("schedules_list", () => ({
    paused: false,
    schedules: [
      {
        key: "k",
        cron: "0 9 * * *",
        timezone: "UTC",
        dir: "/repo",
        mode: "loop",
        enabled: true,
        runs,
      },
    ],
  }));
  TestBed.configureTestingModule({
    providers: [{ provide: SpacesStore, useValue: { panes: signal(new Map([["t2", {}]])) } }],
  });
  TestBed.overrideComponent(ScheduleRuns, {
    set: {
      templateUrl: undefined,
      template: readFileSync("src/app/deliver/schedule-runs.html", "utf8"),
    },
  });
  await TestBed.inject(Schedules).refresh();
  const fixture = TestBed.createComponent(ScheduleRuns);
  await fixture.whenStable();
  fixture.detectChanges();
  return fixture;
}

describe("ScheduleRuns", () => {
  it("lists the runs newest first with the trigger and opens a terminal that exists", async () => {
    const fixture = await setup([
      {
        terminal_id: "t1",
        due_ms: 0,
        started_ms: new Date(2026, 0, 9, 9, 0).getTime(),
        exit_code: 3,
        outcome: "failed",
      },
      {
        terminal_id: "t2",
        due_ms: 0,
        started_ms: new Date(2026, 0, 10, 9, 0).getTime(),
        outcome: "running",
      },
    ]);
    const chosen: string[] = [];
    fixture.componentInstance.paneChosen.subscribe((id) => chosen.push(id));
    const host = fixture.nativeElement as HTMLElement;
    const lines = [...host.querySelectorAll("li")].map((li) =>
      li.textContent!.replace(/\s+/g, " ").trim(),
    );
    expect(lines.length).toBe(2);
    expect(lines[0]).toContain("schedule");
    expect(lines[0]).toContain("2026-01-10 09:00");
    expect(lines[1]).toContain("exit code 3");
    // Only the run whose terminal is still there offers it.
    const buttons = host.querySelectorAll("button");
    expect(buttons.length).toBe(1);
    buttons[0]!.click();
    expect(chosen).toEqual(["t2"]);
  });
});
