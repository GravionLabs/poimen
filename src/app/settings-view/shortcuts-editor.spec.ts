// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import { tauri } from "../core/tauri.mock";
import { ShortcutsEditor } from "./shortcuts-editor";

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

function setup(shortcuts: Record<string, string> = {}) {
  tauri.reset();
  tauri.answer("set_shortcut", () => null);
  TestBed.resetTestingModule();
  TestBed.overrideComponent(ShortcutsEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      styleUrls: undefined,
      template: readFileSync("src/app/settings-view/shortcuts-editor.html", "utf8"),
    },
  });
  TestBed.inject(SettingsStore)["_settings"].set({
    ...DEFAULT_SETTINGS,
    shortcuts: { ...DEFAULT_SETTINGS.shortcuts, ...shortcuts },
  });
  const fixture = TestBed.createComponent(ShortcutsEditor);
  fixture.detectChanges();
  const host = fixture.nativeElement as HTMLElement;
  const button = (label: string) =>
    host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`) ??
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (b) => b.textContent?.trim() === label,
    )!;
  const press = (init: KeyboardEventInit) => {
    window.dispatchEvent(new KeyboardEvent("keydown", { cancelable: true, ...init }));
    fixture.detectChanges();
  };
  const settle = async () => {
    await fixture.whenStable();
    fixture.detectChanges();
  };
  const calls = () => tauri.calls("set_shortcut");
  return { fixture, host, button, press, settle, calls };
}

describe("ShortcutsEditor", () => {
  it("lists every action with its shortcut", () => {
    const { host } = setup({ closeTab: "" });
    expect(host.querySelectorAll("li[data-action]").length).toBe(20);
    expect(host.querySelector('[data-action="jump"]')?.textContent).toContain("K");
    expect(host.querySelector('[data-action="closeTab"]')?.textContent).toContain("Unbound");
  });

  it("records a combination and writes only that entry", async () => {
    const { button, press, settle, calls, host } = setup();
    button("Record New pane...").click();
    (host.ownerDocument.defaultView as Window).focus();
    press({ key: "n", code: "KeyN", ctrlKey: true, altKey: true });
    button("Save").click();
    await settle();
    expect(calls()).toEqual([{ action: "newTerminal", accelerator: "CmdOrCtrl+Alt+N" }]);
  });

  it("shows a conflict before saving and does not write", async () => {
    const { button, press, settle, calls, host } = setup();
    button("Record New pane...").click();
    press({ key: "w", code: "KeyW", ctrlKey: true });
    expect(host.textContent).toContain("already used by");
    expect(button("Save").disabled).toBe(true);
    button("Save").click();
    await settle();
    expect(calls()).toEqual([]);
  });

  it("refuses a key without a modifier and cancels with Escape", () => {
    const { button, press, host } = setup();
    button("Record New pane...").click();
    press({ key: "n", code: "KeyN" });
    expect(host.textContent).toContain("needs Cmd, Ctrl or Alt");
    press({ key: "Escape", code: "Escape" });
    expect(host.textContent).not.toContain("Press the new shortcut");
  });

  it("reset removes the entry and clear unbinds", async () => {
    const { button, settle, calls } = setup({ closeTab: "Ctrl+Alt+X" });
    button("Reset Close Tab").click();
    await settle();
    button("Clear Close Tab").click();
    await settle();
    expect(calls()).toEqual([
      { action: "closeTab", accelerator: null },
      { action: "closeTab", accelerator: "" },
    ]);
  });
});
