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

describe("flow direction", () => {
  it("is left to right until top to bottom is chosen, and is remembered", () => {
    const storage = fakeStorage();
    const state = stateOn(storage);
    expect(state.flowDirection()).toBe("LR");
    state.setFlowDirection("TB");
    expect(stateOn(storage).flowDirection()).toBe("TB");
    state.setFlowDirection("LR");
    expect(stateOn(storage).flowDirection()).toBe("LR");
  });

  it("ignores a stored value that is not a direction", () => {
    expect(parseLayout(JSON.stringify({ flowDirection: "RL" })).flowDirection).toBeUndefined();
    expect(parseLayout(JSON.stringify({ flowDirection: "TB" })).flowDirection).toBe("TB");
  });
});
