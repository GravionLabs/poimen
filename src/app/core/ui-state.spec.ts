import { Injector } from "@angular/core";
import { describe, expect, it } from "vitest";
import {
  SIDEBAR_DEFAULT_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  UI_STATE_KEY,
  UI_STORAGE,
  UiState,
  clampSidebarWidth,
  parseLayout,
  type UiStorage,
  ZOOM_LIMIT,
} from "./ui-state";

function fakeStorage(initial?: string): UiStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  if (initial !== undefined) data.set(UI_STATE_KEY, initial);
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => void data.set(key, value),
  };
}

const stateOn = (storage: UiStorage | null) =>
  Injector.create({
    providers: [{ provide: UI_STORAGE, useValue: storage }, UiState],
  }).get(UiState);

describe("clampSidebarWidth", () => {
  it("keeps a width in range", () => expect(clampSidebarWidth(300)).toBe(300));
  it("raises to the minimum", () => expect(clampSidebarWidth(10)).toBe(SIDEBAR_MIN_WIDTH));
  it("lowers to the maximum", () => expect(clampSidebarWidth(9000)).toBe(SIDEBAR_MAX_WIDTH));
  it("falls back to the default for NaN", () =>
    expect(clampSidebarWidth(NaN)).toBe(SIDEBAR_DEFAULT_WIDTH));
});

describe("parseLayout", () => {
  it("returns defaults without stored state", () =>
    expect(parseLayout(null)).toEqual({ sidebarWidth: 260, sidebarCollapsed: false }));
  it("reads stored values", () =>
    expect(parseLayout('{"sidebarWidth":320,"sidebarCollapsed":true}')).toEqual({
      sidebarWidth: 320,
      sidebarCollapsed: true,
    }));
  it("clamps a stored width", () =>
    expect(parseLayout('{"sidebarWidth":5}').sidebarWidth).toBe(SIDEBAR_MIN_WIDTH));
  it("ignores corrupt json and wrong types", () => {
    const defaults = { sidebarWidth: 260, sidebarCollapsed: false };
    expect(parseLayout("{nope")).toEqual(defaults);
    expect(parseLayout("42")).toEqual(defaults);
    expect(parseLayout('{"sidebarWidth":"wide","sidebarCollapsed":1}')).toEqual(defaults);
  });
});

describe("UiState", () => {
  it("zooms in steps, clamps and resets without persisting", () => {
    const storage = fakeStorage();
    const state = stateOn(storage);
    state.zoomBy(2);
    state.zoomBy(-5);
    expect(state.zoom()).toBe(-3);
    state.zoomBy(100);
    expect(state.zoom()).toBe(ZOOM_LIMIT);
    state.zoomBy(null);
    expect(state.zoom()).toBe(0);
    expect(storage.data.size).toBe(0);
  });

  it("starts from defaults and persists width and collapse", () => {
    const storage = fakeStorage();
    const state = stateOn(storage);
    expect(state.sidebarWidth()).toBe(260);
    expect(state.sidebarCollapsed()).toBe(false);

    state.setSidebarWidth(340);
    state.toggleSidebar();

    expect(JSON.parse(storage.data.get(UI_STATE_KEY)!)).toEqual({
      sidebarWidth: 340,
      sidebarCollapsed: true,
    });
  });

  it("restores the persisted layout on the next start", () => {
    const state = stateOn(fakeStorage('{"sidebarWidth":400,"sidebarCollapsed":true}'));
    expect(state.sidebarWidth()).toBe(400);
    expect(state.sidebarCollapsed()).toBe(true);
  });

  it("toggles back to expanded and clamps dragged widths", () => {
    const state = stateOn(fakeStorage());
    state.toggleSidebar();
    state.toggleSidebar();
    expect(state.sidebarCollapsed()).toBe(false);
    state.setSidebarWidth(1);
    expect(state.sidebarWidth()).toBe(SIDEBAR_MIN_WIDTH);
  });

  it("works without storage", () => {
    const state = stateOn(null);
    state.setSidebarWidth(300);
    expect(state.sidebarWidth()).toBe(300);
  });

  it("survives a storage that throws", () => {
    const state = stateOn({
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("full");
      },
    });
    state.toggleSidebar();
    expect(state.sidebarCollapsed()).toBe(true);
  });
});

describe("editor state persistence", () => {
  it("round-trips the editor area next to the layout", () => {
    const storage = fakeStorage();
    const state = stateOn(storage);
    state.setSidebarWidth(300);
    state.saveEditor({ focusedId: 2 });
    expect(JSON.parse(storage.data.get(UI_STATE_KEY)!)).toEqual({
      sidebarWidth: 300,
      sidebarCollapsed: false,
      editor: { focusedId: 2 },
    });
    expect(stateOn(storage).storedEditor).toEqual({ focusedId: 2 });
  });

  it("keeps the editor area when the sidebar changes", () => {
    const storage = fakeStorage(JSON.stringify({ editor: { focusedId: 5 } }));
    stateOn(storage).toggleSidebar();
    expect(JSON.parse(storage.data.get(UI_STATE_KEY)!).editor).toEqual({ focusedId: 5 });
  });
});
