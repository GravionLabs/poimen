// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { parseNotify } from "../core/notify-settings";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import { tauri } from "../core/tauri.mock";
import { NotificationsPage } from "./notifications-page";

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

function setup(notify: unknown) {
  tauri.reset();
  TestBed.resetTestingModule();
  TestBed.overrideComponent(NotificationsPage, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      styleUrls: undefined,
      template: readFileSync("src/app/settings-view/notifications-page.html", "utf8"),
    },
  });
  TestBed.inject(SettingsStore)["_settings"].set({
    ...DEFAULT_SETTINGS,
    notify: parseNotify(notify),
  });
  const fixture = TestBed.createComponent(NotificationsPage);
  fixture.detectChanges();
  const host = fixture.nativeElement as HTMLElement;
  const test = (status: string) =>
    host.querySelector<HTMLButtonElement>(`button[aria-label="Test ${status} notification"]`)!;
  const settle = async () => {
    await fixture.whenStable();
    fixture.detectChanges();
  };
  return { fixture, host, test, settle };
}

describe("NotificationsPage", () => {
  it("shows what is configured and disables Test for a status without a hook", () => {
    const { host, test } = setup({
      blocked: { command: ["ntfy", "publish", "t", "{text}"] },
      done: {},
    });
    expect(host.textContent).toContain("ntfy publish t {text}");
    expect(host.textContent).toContain("Nothing configured.");
    expect(test("blocked").disabled).toBe(false);
    expect(test("done").disabled).toBe(true);
  });

  it("the Test button runs the hook through the backend and says so", async () => {
    const { host, test, settle } = setup({
      done: { url: "https://ntfy.sh/t" },
    });
    tauri.answer("notify_test", () => null);
    test("done").click();
    await settle();
    expect(tauri.calls("notify_test")).toEqual([{ status: "done" }]);
    expect(host.querySelector('[role="status"]')?.textContent).toContain("Sent");
  });

  it("shows the reason when the hook fails", async () => {
    const { host, test, settle } = setup({ blocked: { command: ["nope"] } });
    tauri.answer("notify_test", () => {
      throw new Error("notify command nope: not found");
    });
    test("blocked").click();
    await settle();
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("not found");
  });
});
