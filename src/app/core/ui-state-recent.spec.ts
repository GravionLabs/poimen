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

describe("recent palette commands", () => {
  it("puts the newest first, moves a repeat up and keeps five", () => {
    const state = stateOn(null);
    for (const k of ["a", "b", "c", "d", "e", "f"]) state.addRecentCommand(k);
    state.addRecentCommand("d");
    expect(state.recentCommands()).toEqual(["d", "f", "e", "c", "b"]);
  });

  it("ignores an empty key", () => {
    const state = stateOn(null);
    state.addRecentCommand("");
    expect(state.recentCommands()).toEqual([]);
  });

  it("persists across a restart", () => {
    const storage = fakeStorage();
    stateOn(storage).addRecentCommand("command:refresh");
    expect(stateOn(storage).recentCommands()).toEqual(["command:refresh"]);
  });

  it("drops malformed stored entries, duplicates and the excess", () => {
    const layout = parseLayout(
      JSON.stringify({ recentCommands: ["a", 3, "", "a", "b", "c", "d", "e", "f"] }),
    );
    expect(layout.recentCommands).toEqual(["a", "b", "c", "d", "e"]);
    expect(parseLayout(JSON.stringify({ recentCommands: "x" })).recentCommands).toBeUndefined();
  });
});
