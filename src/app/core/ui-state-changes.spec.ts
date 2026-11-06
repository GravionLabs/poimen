import { Injector } from "@angular/core";
import { describe, expect, it } from "vitest";
import { UI_STATE_KEY, UI_STORAGE, UiState, parseLayout, type UiStorage } from "./ui-state";

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
  Injector.create({ providers: [{ provide: UI_STORAGE, useValue: storage }, UiState] }).get(
    UiState,
  );

describe("changes layout", () => {
  it("is side by side until the inline layout is chosen, and is remembered", () => {
    const storage = fakeStorage();
    const state = stateOn(storage);
    expect(state.changesLayout()).toBe("split");
    state.setChangesLayout("inline");
    expect(stateOn(storage).changesLayout()).toBe("inline");
    state.setChangesLayout("split");
    expect(stateOn(storage).changesLayout()).toBe("split");
  });

  it("ignores a stored value that is not a layout", () => {
    expect(parseLayout(JSON.stringify({ changesLayout: "grid" })).changesLayout).toBeUndefined();
    expect(parseLayout(JSON.stringify({ changesLayout: "inline" })).changesLayout).toBe("inline");
  });
});
