import { Injector } from "@angular/core";
import { describe, expect, it } from "vitest";
import {
  EXPANDED_RUNS_LIMIT,
  UI_STATE_KEY,
  UI_STORAGE,
  UiState,
  parseLayout,
  type UiStorage,
} from "./ui-state";

function fakeStorage(): UiStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
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

describe("expanded groups and runs of the Runs view (#584)", () => {
  it("are collapsed until expanded, and are remembered", () => {
    const storage = fakeStorage();
    const state = stateOn(storage);
    expect(state.expandedRunGroups().size).toBe(0);
    state.setRunGroupExpanded("/r\ngithub.milestone\no/r#1", true);
    state.setRunExpanded("/r\nrun-1", true);
    const again = stateOn(storage);
    expect([...again.expandedRunGroups()]).toEqual(["/r\ngithub.milestone\no/r#1"]);
    expect([...again.expandedRuns()]).toEqual(["/r\nrun-1"]);
    again.setRunGroupExpanded("/r\ngithub.milestone\no/r#1", false);
    again.setRunExpanded("/r\nrun-1", false);
    const stored = JSON.parse(storage.data.get(UI_STATE_KEY)!) as Record<string, unknown>;
    expect(stored["expandedRunGroups"]).toBeUndefined();
    expect(stored["expandedRuns"]).toBeUndefined();
  });

  it("keeps the newest keys only, so the list does not grow without end", () => {
    const state = stateOn(fakeStorage());
    for (let i = 0; i <= EXPANDED_RUNS_LIMIT; i++) state.setRunExpanded(`run-${i}`, true);
    const keys = [...state.expandedRuns()];
    expect(keys).toHaveLength(EXPANDED_RUNS_LIMIT);
    expect(keys[0]).toBe("run-1");
    expect(keys.at(-1)).toBe(`run-${EXPANDED_RUNS_LIMIT}`);
  });

  it("drops stored keys that are not strings, empty or repeated", () => {
    const layout = parseLayout(
      JSON.stringify({ expandedRunGroups: ["a", 1, "", "a", "b"], expandedRuns: "x" }),
    );
    expect(layout.expandedRunGroups).toEqual(["a", "b"]);
    expect(layout.expandedRuns).toBeUndefined();
  });
});
