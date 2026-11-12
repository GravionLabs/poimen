// @vitest-environment jsdom
import { Injector } from "@angular/core";
import { describe, expect, it } from "vitest";
import { BottomPanel } from "./bottom-panel";
import { UI_STORAGE, UiState } from "../core/ui-state";

interface Cols {
  colStart(e: unknown, column: string, sign: 1 | -1): void;
  colMove(e: unknown): void;
  colEnd(): void;
  colKey(e: unknown, column: string, sign: 1 | -1): void;
}

function setup() {
  const ui = Injector.create({
    providers: [{ provide: UI_STORAGE, useValue: null }, UiState],
  }).get(UiState);
  const panel = Object.create(BottomPanel.prototype) as Cols & { ui: UiState };
  Object.defineProperty(panel, "ui", { value: ui });
  return { ui, panel };
}

const down = (x: number) => ({ clientX: x, pointerId: 1, target: document.createElement("i") });
const key = (k: string, shiftKey = false) => ({ key: k, shiftKey, preventDefault() {} });

describe("Events column resizing", () => {
  it("drags a border to widen and narrow a column", () => {
    const { ui, panel } = setup();
    panel.colStart(down(100), "task", 1);
    panel.colMove({ clientX: 160 });
    expect(ui.eventColumns().task).toBe(200);
    panel.colEnd();
    panel.colMove({ clientX: 0 });
    expect(ui.eventColumns().task).toBe(200);
  });

  it("grows the result column when its border moves left", () => {
    const { ui, panel } = setup();
    panel.colStart(down(100), "result", -1);
    panel.colMove({ clientX: 80 });
    expect(ui.eventColumns().result).toBe(110);
  });

  it("stops at the minimum width", () => {
    const { ui, panel } = setup();
    panel.colStart(down(500), "task", 1);
    panel.colMove({ clientX: 0 });
    expect(ui.eventColumns().task).toBe(40);
  });

  it("resizes with the arrow keys, Shift for a larger step, other keys ignored", () => {
    const { ui, panel } = setup();
    panel.colKey(key("ArrowRight"), "task", 1);
    expect(ui.eventColumns().task).toBe(150);
    panel.colKey(key("ArrowLeft", true), "task", 1);
    expect(ui.eventColumns().task).toBe(110);
    panel.colKey(key("ArrowLeft"), "result", -1);
    expect(ui.eventColumns().result).toBe(100);
    panel.colKey(key("a"), "task", 1);
    expect(ui.eventColumns().task).toBe(110);
  });
});
