import { Injector } from "@angular/core";
import { describe, expect, it } from "vitest";
import { UI_STATE_KEY, UI_STORAGE, UiState, parseLayout, type UiStorage } from "./ui-state";

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
