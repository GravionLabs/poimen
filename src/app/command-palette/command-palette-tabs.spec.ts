// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { CommandPalette as Palette } from "../core/command-palette";
import { EditorTabs } from "../core/editor-tabs";
import { SnippetRunner } from "../core/snippets";
import { CommandPalette } from "./command-palette";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

describe("command palette keyboard", () => {
  function press(key: string, shiftKey = false) {
    const palette = { stepTab: vi.fn(), move: vi.fn(), selected: () => 0, isOpen: () => false };
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        { provide: Palette, useValue: palette },
        { provide: EditorTabs, useValue: { openIds: () => new Set() } },
        { provide: SnippetRunner, useValue: {} },
      ],
    });
    TestBed.overrideComponent(CommandPalette, {
      set: { template: "", templateUrl: undefined, styleUrl: undefined, styles: [] },
    });
    const component = TestBed.createComponent(CommandPalette).componentInstance as unknown as {
      key(event: KeyboardEvent): void;
    };
    const event = new KeyboardEvent("keydown", { key, shiftKey, cancelable: true });
    component.key(event);
    return { palette, event };
  }

  it("Tab steps to the next tab and Shift+Tab to the previous one, focus stays in the input", () => {
    const next = press("Tab");
    expect(next.palette.stepTab).toHaveBeenCalledWith(1);
    expect(next.event.defaultPrevented).toBe(true);
    expect(press("Tab", true).palette.stepTab).toHaveBeenCalledWith(-1);
  });
});
