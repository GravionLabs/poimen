// @vitest-environment jsdom
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { EditorTabs, SETTINGS_TAB, type EditorGroup } from "../core/editor-tabs";
import { HIDDEN_VIEW_LIMIT, HIDDEN_VIEW_MS, HiddenViews } from "./hidden-views";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialised
  }
});

const group = (active: string | null, tabs: string[], id = 1): EditorGroup => ({
  id,
  tabs: tabs.map((paneId) => ({ paneId, pinned: true })),
  activeId: active,
});

describe("HiddenViews (#978)", () => {
  let groups: ReturnType<typeof signal<readonly EditorGroup[]>>;
  let views: HiddenViews;

  const show = (next: EditorGroup[]) => {
    groups.set(next);
    TestBed.tick();
  };

  beforeEach(() => {
    TestBed.resetTestingModule();
    vi.useFakeTimers();
    groups = signal<readonly EditorGroup[]>([group("w1:p1", ["w1:p1", "w1:p2"])]);
    TestBed.configureTestingModule({ providers: [{ provide: EditorTabs, useValue: { groups } }] });
    views = TestBed.inject(HiddenViews);
    TestBed.tick();
  });

  afterEach(() => vi.useRealTimers());

  it("mounts the active pane only at first", () => {
    expect(views.of(1)).toEqual(["w1:p1"]);
  });

  it("keeps the view of a tab that was hidden, then closes it after the time", () => {
    show([group("w1:p2", ["w1:p1", "w1:p2"])]);
    expect(views.of(1)).toEqual(["w1:p1", "w1:p2"]);
    vi.advanceTimersByTime(HIDDEN_VIEW_MS - 1);
    expect(views.of(1)).toEqual(["w1:p1", "w1:p2"]);
    vi.advanceTimersByTime(1);
    expect(views.of(1)).toEqual(["w1:p2"]);
  });

  it("keeps the order and the instance when the tab comes back, and restarts its time", () => {
    show([group("w1:p2", ["w1:p1", "w1:p2"])]);
    vi.advanceTimersByTime(HIDDEN_VIEW_MS - 1000);
    show([group("w1:p1", ["w1:p1", "w1:p2"])]);
    expect(views.of(1)).toEqual(["w1:p1", "w1:p2"]);
    vi.advanceTimersByTime(HIDDEN_VIEW_MS * 2);
    // p1 is active and never expires; p2 was hidden by the switch back and expired.
    expect(views.of(1)).toEqual(["w1:p1"]);
  });

  it("keeps at most the limit of hidden views, the oldest goes first", () => {
    const tabs = ["a", "b", "c", "d", "e", "f"];
    show([group("a", tabs)]);
    for (const id of tabs.slice(1)) show([group(id, tabs)]);
    // f is active; a..e were hidden in this order, only the last three stay.
    expect(HIDDEN_VIEW_LIMIT).toBe(3);
    expect(views.of(1)).toEqual(["c", "d", "e", "f"]);
  });

  it("parks the pane when a non-terminal tab becomes active", () => {
    show([group(SETTINGS_TAB, ["w1:p1", SETTINGS_TAB])]);
    expect(views.of(1)).toEqual(["w1:p1"]);
    vi.advanceTimersByTime(HIDDEN_VIEW_MS);
    expect(views.of(1)).toEqual([]);
  });

  it("drops a closed tab at once", () => {
    show([group("w1:p2", ["w1:p1", "w1:p2"])]);
    show([group("w1:p2", ["w1:p2"])]);
    expect(views.of(1)).toEqual(["w1:p2"]);
  });
});
