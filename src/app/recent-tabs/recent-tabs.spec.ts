// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { readFileSync } from "node:fs";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";
import { EditorTabs } from "../core/editor-tabs";
import { UI_STORAGE } from "../core/ui-state";
import { RecentTabs } from "./recent-tabs";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized
  }
});

function press(type: "keydown" | "keyup", key: string, init: KeyboardEventInit = {}): void {
  window.dispatchEvent(new KeyboardEvent(type, { key, bubbles: true, cancelable: true, ...init }));
}

describe("RecentTabs", () => {
  let editor: EditorTabs;
  let host: HTMLElement;

  async function settle(fixture: { detectChanges(): void; whenStable(): Promise<void> }) {
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
  }

  beforeEach(() => {
    TestBed.resetTestingModule();
    TestBed.overrideComponent(RecentTabs, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/recent-tabs/recent-tabs.html", "utf8"),
      },
    });
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: null }] });
    editor = TestBed.inject(EditorTabs);
  });

  async function setup() {
    const fixture = TestBed.createComponent(RecentTabs);
    host = fixture.nativeElement as HTMLElement;
    for (const id of ["a", "b", "c"]) editor.open(id, true);
    await settle(fixture);
    editor.activate("a");
    await settle(fixture);
    editor.activate("c");
    await settle(fixture);
    return fixture;
  }

  it("cycles in most-recently-used order and selects on Ctrl release", async () => {
    const fixture = await setup();
    // MRU: c, a, b
    press("keydown", "Tab", { ctrlKey: true });
    await settle(fixture);
    const rows = [...host.querySelectorAll(".item")].map((e) => e.textContent?.trim());
    expect(rows).toEqual(["c", "a", "b"]);
    expect(host.querySelector(".item.selected")?.textContent?.trim()).toBe("a");
    press("keydown", "Tab", { ctrlKey: true });
    await settle(fixture);
    expect(host.querySelector(".item.selected")?.textContent?.trim()).toBe("b");
    expect(editor.activeId()).toBe("c");
    press("keyup", "Control");
    await settle(fixture);
    expect(editor.activeId()).toBe("b");
    expect(host.querySelector(".switcher")).toBeNull();
  });

  it("goes back with Ctrl+Shift+Tab", async () => {
    const fixture = await setup();
    press("keydown", "Tab", { ctrlKey: true, shiftKey: true });
    await settle(fixture);
    expect(host.querySelector(".item.selected")?.textContent?.trim()).toBe("b");
    press("keyup", "Control");
    expect(editor.activeId()).toBe("b");
  });

  it("cancels with Escape", async () => {
    const fixture = await setup();
    press("keydown", "Tab", { ctrlKey: true });
    press("keydown", "Escape");
    press("keyup", "Control");
    await settle(fixture);
    expect(editor.activeId()).toBe("c");
    expect(host.querySelector(".switcher")).toBeNull();
  });
});
