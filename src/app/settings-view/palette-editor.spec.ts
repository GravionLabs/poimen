// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import { tauri } from "../core/tauri.mock";
import { PaletteEditor } from "./palette-editor";

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

function setup(hidden: Record<string, { source: string; name: string }[]> = {}) {
  tauri.reset();
  tauri.answer("set_palette", () => null);
  TestBed.resetTestingModule();
  TestBed.overrideComponent(PaletteEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/settings-view/palette-editor.html", "utf8"),
    },
  });
  TestBed.inject(SettingsStore)["_settings"].set({
    ...DEFAULT_SETTINGS,
    palette: { scripts: true, skills: true, hidden },
  });
  const fixture = TestBed.createComponent(PaletteEditor);
  fixture.detectChanges();
  return fixture;
}

describe("PaletteEditor", () => {
  it("lists the sources on and the hidden entries per repository", () => {
    const host = setup({ "/repo": [{ source: "npm", name: "build" }] })
      .nativeElement as HTMLElement;
    const checks = host.querySelectorAll<HTMLInputElement>("input[type=checkbox]");
    expect([...checks].map((c) => c.checked)).toEqual([true, true]);
    expect(host.querySelector(".repo-root")?.textContent).toBe("/repo");
    expect(host.querySelector(".row-item")?.textContent).toContain("build");
  });

  it("says when nothing is hidden", () => {
    expect((setup().nativeElement as HTMLElement).textContent).toContain("No entry is hidden");
  });

  it("turns a source off by writing only palette", async () => {
    const fixture = setup();
    const box = (fixture.nativeElement as HTMLElement).querySelector<HTMLInputElement>(
      "[name=palette-source-skills]",
    )!;
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    await fixture.whenStable();
    expect(tauri.calls("set_palette")).toEqual([
      { palette: { sources: { scripts: true, skills: false }, hidden: {} } },
    ]);
  });

  it("shows a hidden entry again", async () => {
    const fixture = setup({ "/repo": [{ source: "npm", name: "build" }] });
    const host = fixture.nativeElement as HTMLElement;
    host.querySelector<HTMLButtonElement>(".row-item button")!.click();
    await fixture.whenStable();
    expect(tauri.calls("set_palette")).toEqual([
      { palette: { sources: { scripts: true, skills: true }, hidden: {} } },
    ]);
  });
});
