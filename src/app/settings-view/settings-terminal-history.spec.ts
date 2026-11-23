// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import type { Type } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { HxTheme } from "@gravionlabs/helix-ui";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, SettingsStore } from "../core/settings";
import { tauri } from "../core/tauri.mock";
import { UiState, UI_STORAGE } from "../core/ui-state";
import { LayoutTabsEditor } from "./layout-tabs-editor";
import { LayoutsEditor } from "./layouts-editor";
import { SettingsImportPanel } from "./settings-import-panel";
import { SettingsView } from "./settings-view";
import { ShortcutsEditor } from "./shortcuts-editor";
import { PaletteEditor } from "./palette-editor";
import { SnippetsEditor } from "./snippets-editor";
import { SpaceTemplatesEditor } from "./space-templates-editor";

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

/** The Server page, with the templates of the components read from disk (jsdom has no loader). */
function serverPage() {
  tauri.reset();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: HxTheme, useValue: { setDark: vi.fn() } },
      { provide: UI_STORAGE, useValue: null },
    ],
  });
  const templates: [Type<unknown>, string][] = [
    [SettingsView, "settings-view"],
    [LayoutTabsEditor, "layout-tabs-editor"],
    [LayoutsEditor, "layouts-editor"],
    [SettingsImportPanel, "settings-import-panel"],
    [SpaceTemplatesEditor, "space-templates-editor"],
    [SnippetsEditor, "snippets-editor"],
    [PaletteEditor, "palette-editor"],
    [ShortcutsEditor, "shortcuts-editor"],
  ];
  for (const [component, name] of templates) {
    TestBed.overrideComponent(component, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        styleUrls: undefined,
        template: readFileSync(`src/app/settings-view/${name}.html`, "utf8"),
      },
    });
  }
  TestBed.inject(UiState).selectSettingsPage("server");
  TestBed.inject(SettingsStore)["_settings"].set({ ...DEFAULT_SETTINGS });
  const fixture = TestBed.createComponent(SettingsView);
  fixture.detectChanges();
  return { fixture, host: fixture.nativeElement as HTMLElement };
}

describe("SettingsView terminal history", () => {
  it("shows the defaults and the privacy note", () => {
    const { host } = serverPage();
    const section = host.querySelector('section[aria-label="Server"]')!;
    expect(section.textContent).toContain("stored in files");
    expect(section.querySelector<HTMLInputElement>('input[role="switch"]')!.checked).toBe(true);
    expect(section.querySelector<HTMLInputElement>(".history-per-terminal")!.value).toBe("8");
    expect(section.querySelector<HTMLInputElement>(".history-total")!.value).toBe("256");
  });

  it("writes the setting when the switch and a limit change", () => {
    const { fixture, host } = serverPage();
    tauri.answer("set_terminal_history", () => null);
    const box = host.querySelector<HTMLInputElement>(
      'section[aria-label="Server"] input[role="switch"]',
    )!;
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    fixture.detectChanges();
    const total = host.querySelector<HTMLInputElement>(".history-total")!;
    total.value = "64";
    total.dispatchEvent(new Event("change"));
    fixture.detectChanges();
    expect(tauri.calls("set_terminal_history")).toEqual([
      { enabled: false, perTerminalMib: 8, totalMib: 256 },
      { enabled: false, perTerminalMib: 8, totalMib: 64 },
    ]);
  });

  it("refuses a limit that is not a whole number from 1 and restores the field", async () => {
    const { fixture, host } = serverPage();
    tauri.answer("set_terminal_history", () => null);
    const per = host.querySelector<HTMLInputElement>(".history-per-terminal")!;
    per.value = "0";
    per.dispatchEvent(new Event("change"));
    await fixture.whenStable();
    expect(tauri.calls("set_terminal_history")).toEqual([]);
    expect(per.value).toBe("8");
  });
});
