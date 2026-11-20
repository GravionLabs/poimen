import { describe, expect, it } from "vitest";
import { UI_STATE_KEY, UI_STORAGE, UiState } from "../core/ui-state";
import { Injector } from "@angular/core";
import { TAIL_CHARS, middleParts } from "./space-card";

describe("middleParts (#1075)", () => {
  it("keeps a short text whole", () => {
    expect(middleParts("main")).toEqual({ head: "main", tail: "" });
  });

  it("keeps the end of a long branch visible so the cut is in the middle", () => {
    const branch = "fix/1047-a-very-long-branch-name-for-the-config";
    const { head, tail } = middleParts(branch);
    expect(head + tail).toBe(branch);
    expect(tail).toBe(branch.slice(-TAIL_CHARS));
    expect(tail.endsWith("-config")).toBe(true);
  });
});

describe("hidden details (#1075)", () => {
  function ui(stored?: string) {
    const storage = new Map<string, string>(stored ? [[UI_STATE_KEY, stored]] : []);
    const injector = Injector.create({
      providers: [
        {
          provide: UI_STORAGE,
          useValue: {
            getItem: (k: string) => storage.get(k) ?? null,
            setItem: (k: string, v: string) => void storage.set(k, v),
            removeItem: (k: string) => void storage.delete(k),
          },
        },
        { provide: UiState, useClass: UiState, deps: [] },
      ],
    });
    return { state: injector.get(UiState), storage };
  }

  it("is expanded by default, survives a restart and is pruned with closed spaces", () => {
    const { state, storage } = ui();
    expect(state.hiddenDetails()).toEqual([]);
    state.setDetailsHidden("w1", true);
    state.setDetailsHidden("w2", true);
    const again = ui(storage.get(UI_STATE_KEY));
    expect(again.state.hiddenDetails()).toEqual(["w1", "w2"]);
    again.state.pruneHiddenDetails(["w1"]);
    expect(again.state.hiddenDetails()).toEqual(["w1"]);
    again.state.setDetailsHidden("w1", false);
    expect(again.state.hiddenDetails()).toEqual([]);
  });
});
