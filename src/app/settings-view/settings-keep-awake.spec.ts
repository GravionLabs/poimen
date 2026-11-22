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

/** The Agents page, with the templates of the components read from disk (jsdom has no loader). */
function agentsPage(keepAwake: boolean) {
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
  TestBed.inject(UiState).selectSettingsPage("agents");
  TestBed.inject(SettingsStore)["_settings"].set({ ...DEFAULT_SETTINGS, keepAwake });
  const fixture = TestBed.createComponent(SettingsView);
  fixture.detectChanges();
  return { fixture, host: fixture.nativeElement as HTMLElement };
}

describe("SettingsView keep awake switch", () => {
  const find = (host: HTMLElement) =>
    [
      ...host.querySelectorAll<HTMLInputElement>(
        'section[aria-label="Agents"] input[role="switch"]',
      ),
    ].find((box) => box.closest("label")?.textContent?.includes("Keep the machine awake"))!;

  it("shows the setting and writes agents.keepAwake when switched", () => {
    const { fixture, host } = agentsPage(false);
    tauri.answer("set_keep_awake", () => null);
    const box = find(host);
    expect(box.checked).toBe(false);
    box.checked = true;
    box.dispatchEvent(new Event("change"));
    fixture.detectChanges();
    expect(tauri.calls("set_keep_awake")).toEqual([{ on: true }]);
  });

  it("is on when the setting is", () => {
    expect(find(agentsPage(true).host).checked).toBe(true);
  });
});
