import { Injector } from "@angular/core";
import { describe, expect, it } from "vitest";
import {
  SPLIT_DEFAULT_SIZES,
  UI_STATE_KEY,
  UI_STORAGE,
  UiState,
  clampSplitSizes,
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
  Injector.create({
    providers: [{ provide: UI_STORAGE, useValue: storage }, UiState],
  }).get(UiState);

describe("split workspace UI state", () => {
  it("starts empty, persists the selection and restores it", () => {
    const storage = fakeStorage();
    const state = stateOn(storage);
    expect(state.splitWorkspace()).toBeNull();
    state.setSplitWorkspace("w2");
    expect(JSON.parse(storage.getItem(UI_STATE_KEY)!).splitWorkspace).toBe("w2");
    expect(stateOn(storage).splitWorkspace()).toBe("w2");
  });

  it("ignores a malformed stored value", () => {
    expect(parseLayout('{"splitWorkspace":5}').splitWorkspace).toBeUndefined();
    expect(parseLayout('{"splitWorkspace":""}').splitWorkspace).toBeUndefined();
  });
});

describe("split section sizes UI state", () => {
  it("defaults to 50/50, persists a resize and restores it", () => {
    const storage = fakeStorage();
    const state = stateOn(storage);
    expect(state.splitSizes()).toEqual(SPLIT_DEFAULT_SIZES);
    state.setSplitSizes([0.3, 0.7]);
    expect(JSON.parse(storage.getItem(UI_STATE_KEY)!).splitSizes).toEqual([0.3, 0.7]);
    expect(stateOn(storage).splitSizes()).toEqual([0.3, 0.7]);
  });

  it("keeps each section at the minimum share and ignores malformed sizes", () => {
    expect(clampSplitSizes([0.02, 0.98])).toEqual([0.15, 0.85]);
    expect(clampSplitSizes([1, 0])).toEqual([0.85, 0.15]);
    expect(clampSplitSizes([2, 2])).toEqual([0.5, 0.5]);
    expect(clampSplitSizes([0.5])).toBeNull();
    expect(clampSplitSizes(["a", 1])).toBeNull();
    expect(clampSplitSizes([0, 0])).toBeNull();
    expect(parseLayout('{"splitSizes":[0.5]}').splitSizes).toBeUndefined();
    expect(parseLayout('{"splitSizes":[0.25,0.75]}').splitSizes).toEqual([0.25, 0.75]);
    const state = stateOn(fakeStorage());
    state.setSplitSizes([Number.NaN, 1]);
    expect(state.splitSizes()).toEqual(SPLIT_DEFAULT_SIZES);
  });
});
