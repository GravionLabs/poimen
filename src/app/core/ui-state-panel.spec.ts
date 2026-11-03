import { Injector } from "@angular/core";
import { describe, expect, it } from "vitest";
import {
  PANEL_DEFAULT_HEIGHT,
  PANEL_MAX_HEIGHT,
  PANEL_MIN_HEIGHT,
  UI_STATE_KEY,
  UI_STORAGE,
  UiState,
  clampPanelHeight,
  parseLayout,
  type UiStorage,
} from "./ui-state";

function fakeStorage(): UiStorage {
  const data = new Map<string, string>();
  return {
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => void data.set(key, value),
  };
}

const stateOn = (storage: UiStorage) =>
  Injector.create({ providers: [{ provide: UI_STORAGE, useValue: storage }, UiState] }).get(
    UiState,
  );

describe("bottom panel UI state", () => {
  it("starts hidden at the default height on the output tab", () => {
    const state = stateOn(fakeStorage());
    expect(state.panelOpen()).toBe(false);
    expect(state.panelHeight()).toBe(PANEL_DEFAULT_HEIGHT);
    expect(state.panelTab()).toBe("output");
  });

  it("remembers visibility and height across a restart", () => {
    const storage = fakeStorage();
    const first = stateOn(storage);
    first.togglePanel();
    first.setPanelHeight(333);
    const second = stateOn(storage);
    expect(second.panelOpen()).toBe(true);
    expect(second.panelHeight()).toBe(333);
    second.togglePanel();
    expect(stateOn(storage).panelOpen()).toBe(false);
  });

  it("clamps the height and rejects non-finite values", () => {
    expect(clampPanelHeight(5)).toBe(PANEL_MIN_HEIGHT);
    expect(clampPanelHeight(99999)).toBe(PANEL_MAX_HEIGHT);
    expect(clampPanelHeight(200.4)).toBe(200);
    expect(clampPanelHeight(NaN)).toBe(PANEL_DEFAULT_HEIGHT);
  });

  it("remembers the flow tab across a restart", () => {
    const storage = fakeStorage();
    stateOn(storage).selectPanelTab("flow");
    expect(stateOn(storage).panelTab()).toBe("flow");
  });

  it("selecting a tab shows the panel; an unknown tab is ignored", () => {
    const state = stateOn(fakeStorage());
    state.selectPanelTab("nope");
    expect(state.panelOpen()).toBe(false);
    state.selectPanelTab("output");
    expect(state.panelOpen()).toBe(true);
  });

  it("falls back to defaults for malformed stored values", () => {
    const layout = parseLayout(
      JSON.stringify({ panelOpen: "yes", panelHeight: "tall", panelTab: "gantt" }),
    );
    expect(layout.panelOpen).toBe(false);
    expect(layout.panelHeight).toBe(PANEL_DEFAULT_HEIGHT);
    expect(layout.panelTab).toBe("output");
  });

  it("is written to the same storage key as the rest of the layout", () => {
    const storage = fakeStorage();
    stateOn(storage).togglePanel();
    expect(JSON.parse(storage.getItem(UI_STATE_KEY)!)).toMatchObject({ panelOpen: true });
  });
});
