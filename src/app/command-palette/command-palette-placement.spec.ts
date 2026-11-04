// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { CommandPalette as Palette } from "../core/command-palette";
import { EditorTabs } from "../core/editor-tabs";
import { CommandPalette } from "./command-palette";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

afterEach(() => {
  TestBed.resetTestingModule();
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
});

function placeWith(openTabs: string[]) {
  vi.stubGlobal("innerWidth", 1000);
  vi.stubGlobal("innerHeight", 800);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  const area = document.createElement("div");
  area.className = "editor-area";
  area.getBoundingClientRect = () =>
    ({ left: 300, top: 100, width: 600, height: 500, right: 900, bottom: 600 }) as DOMRect;
  document.body.appendChild(area);

  TestBed.configureTestingModule({
    providers: [
      { provide: Palette, useValue: { isOpen: signal(true), selected: () => 0 } },
      { provide: EditorTabs, useValue: { openIds: () => new Set(openTabs) } },
    ],
  });
  TestBed.overrideComponent(CommandPalette, {
    set: { template: "", templateUrl: undefined, styleUrl: undefined, styles: [] },
  });
  const fixture = TestBed.createComponent(CommandPalette);
  fixture.detectChanges();
  TestBed.tick();
  return (fixture.componentInstance as unknown as { position: () => unknown }).position();
}

describe("CommandPalette placement", () => {
  it("is centred on the window when no tab is open", () => {
    // 1000x800 window, 480x320 popup: (1000-480)/2, (800-320)/2
    expect(placeWith([])).toEqual({ left: 260, top: 240 });
  });

  it("is centred on the editor area when a tab is open", () => {
    // area 300,100 600x500: 300+(600-480)/2, 100+(500-320)/2
    expect(placeWith(["p1"])).toEqual({ left: 360, top: 190 });
  });
});
