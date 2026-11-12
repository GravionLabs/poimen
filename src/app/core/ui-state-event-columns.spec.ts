import { Injector } from "@angular/core";
import { describe, expect, it } from "vitest";
import {
  EVENT_COLUMN_DEFAULTS,
  EVENT_COLUMN_MIN,
  UI_STATE_KEY,
  UI_STORAGE,
  UiState,
  parseLayout,
  type UiStorage,
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

const stateOn = (storage: UiStorage) =>
  Injector.create({ providers: [{ provide: UI_STORAGE, useValue: storage }, UiState] }).get(
    UiState,
  );

describe("Events tab column widths", () => {
  it("gives the task column more room than the old 4em by default", () => {
    const widths = stateOn(fakeStorage()).eventColumns();
    expect(widths).toEqual({ time: 66, task: 140, result: 90 });
    expect(EVENT_COLUMN_DEFAULTS.task).toBeGreaterThan(48);
  });

  it("changes a width and keeps it across a restart", () => {
    const storage = fakeStorage();
    stateOn(storage).setEventColumn("task", 200);
    expect(stateOn(storage).eventColumns()).toEqual({ time: 66, task: 200, result: 90 });
  });

  it("keeps a width at the minimum", () => {
    const state = stateOn(fakeStorage());
    state.setEventColumn("time", 5);
    expect(state.eventColumns().time).toBe(EVENT_COLUMN_MIN);
    state.setEventColumn("time", Number.NaN);
    expect(state.eventColumns().time).toBe(EVENT_COLUMN_MIN);
  });

  it("restores stored widths, clamped, and ignores malformed ones", () => {
    const stored = parseLayout(
      JSON.stringify({ eventColumns: { task: 10, time: "x", result: 120 } }),
    );
    expect(stored.eventColumns).toEqual({ task: EVENT_COLUMN_MIN, result: 120 });
    expect(parseLayout(JSON.stringify({ eventColumns: [1] })).eventColumns).toBeUndefined();
  });

  it("stores nothing while every width is the default", () => {
    const storage = fakeStorage();
    const state = stateOn(storage);
    state.setEventColumn("task", 200);
    state.setEventColumn("task", EVENT_COLUMN_DEFAULTS.task);
    expect(JSON.parse(storage.data.get(UI_STATE_KEY)!).eventColumns).toBeUndefined();
  });
});
