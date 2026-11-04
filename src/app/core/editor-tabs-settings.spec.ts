// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";
import { EditorTabs, GITHUB_TAB, SETTINGS_TAB, parseEditor, tabKind } from "./editor-tabs";
import { UI_STORAGE, UI_STATE_KEY } from "./ui-state";

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

describe("restoring a layout of the removed plugin integration", () => {
  const layout = { kind: "group", id: 1 };
  const editor = (tabs: string[], activeId: string | null) => ({
    groups: [{ id: 1, tabs: tabs.map((paneId) => ({ paneId, pinned: true })), activeId }],
    layout,
    focusedId: 1,
  });
  const restore = (stored: object) => {
    const data = new Map<string, string>([[UI_STATE_KEY, JSON.stringify(stored)]]);
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        {
          provide: UI_STORAGE,
          useValue: {
            getItem: (k: string) => data.get(k) ?? null,
            setItem: (k: string, v: string) => void data.set(k, v),
          },
        },
      ],
    });
    return TestBed.inject(EditorTabs);
  };

  it("has no board tab kind any more", () => {
    expect(tabKind("poimen:deliver-board")).toBe("pane");
    expect(tabKind(SETTINGS_TAB)).toBe("settings");
  });

  it("opens the GitHub tab for a stored board tab, keeping it active", () => {
    const tabs = restore({ editor: editor(["a", "poimen:deliver-board"], "poimen:deliver-board") });
    expect(tabs.tabs().map((t) => t.paneId)).toEqual(["a", GITHUB_TAB]);
    expect(tabs.activeId()).toBe(GITHUB_TAB);
  });

  it("merges a stored board tab with an open GitHub tab", () => {
    const parsed = parseEditor(
      editor([GITHUB_TAB, "poimen:deliver-board"], "poimen:deliver-board"),
    );
    expect(parsed?.groups[0].tabs.map((t) => t.paneId)).toEqual([GITHUB_TAB]);
    expect(parsed?.groups[0].activeId).toBe(GITHUB_TAB);
  });

  it("drops the tabs of stored plugin panes and picks another active tab", () => {
    const tabs = restore({
      editor: editor(["a", "w1:p5", "b"], "w1:p5"),
      pluginPanes: { "w1:p5": "board" },
    });
    expect(tabs.tabs().map((t) => t.paneId)).toEqual(["a", "b"]);
    expect(tabs.activeId()).toBe("a");
  });

  it("restores an empty editor when only a plugin pane tab was open", () => {
    const tabs = restore({ editor: editor(["w1:p5"], "w1:p5"), pluginPanes: { "w1:p5": "x" } });
    expect(tabs.tabs()).toEqual([]);
    expect(tabs.activeId()).toBeNull();
  });

  it("still rejects an active tab that was never stored", () => {
    expect(parseEditor(editor(["a"], "zzz"))).toBeNull();
  });
});
