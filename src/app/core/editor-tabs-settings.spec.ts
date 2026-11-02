// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";
import { EditorTabs, SETTINGS_TAB, tabKind } from "./editor-tabs";
import { UI_STORAGE } from "./ui-state";

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

describe("settings tab", () => {
  let tabs: EditorTabs;
  const ids = () => tabs.tabs().map((t) => t.paneId);

  beforeEach(() => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: null }] });
    tabs = TestBed.inject(EditorTabs);
  });

  it("tells the settings tab from a pane", () => {
    expect(tabKind(SETTINGS_TAB)).toBe("settings");
    expect(tabKind("w1-p1")).toBe("pane");
  });

  it("opens one pinned settings tab and only activates it afterwards", () => {
    tabs.open("a", true);
    tabs.openSettings();
    expect(tabs.activeId()).toBe(SETTINGS_TAB);
    tabs.activate("a");
    tabs.openSettings();
    expect(ids()).toEqual(["a", SETTINGS_TAB]);
    expect(tabs.activeId()).toBe(SETTINGS_TAB);
    expect(tabs.tabs()[1].pinned).toBe(true);
  });

  it("is neither replaced by a preview pane nor replaces one", () => {
    tabs.open("a");
    tabs.openSettings();
    tabs.open("b");
    expect(ids()).toEqual(["b", SETTINGS_TAB]);
  });

  it("survives pruning of panes herdr no longer has", () => {
    tabs.open("a", true);
    tabs.openSettings();
    expect(tabs.prune(new Set())).toEqual(["a"]);
    expect(ids()).toEqual([SETTINGS_TAB]);
  });

  it("closes like any tab and opens again", () => {
    tabs.openSettings();
    tabs.close(SETTINGS_TAB);
    expect(tabs.openIds().size).toBe(0);
    tabs.openSettings();
    expect(tabs.activeId()).toBe(SETTINGS_TAB);
  });
});
