// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { AgentPicker as PickerState } from "../core/agent-picker";
import { AgentPicker } from "./agent-picker";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});
afterEach(() => TestBed.resetTestingModule());

function setup() {
  // JIT in vitest does not load template files: give the real template inline.
  TestBed.overrideComponent(AgentPicker, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/agent-picker/agent-picker.html", "utf8"),
    },
  });
  const state = TestBed.inject(PickerState);
  const answer = state.pick([
    { paneId: "p1", label: "Claude" },
    { paneId: "p2", label: "Codex" },
    { paneId: "p3", label: "Copilot" },
  ]);
  const fixture = TestBed.createComponent(AgentPicker);
  document.body.appendChild(fixture.nativeElement);
  fixture.detectChanges();
  const el = fixture.nativeElement as HTMLElement;
  const rows = [...el.querySelectorAll<HTMLElement>("hx-list button[hx-list-row]")];
  const press = (key: string) => {
    const keyCode = { ArrowDown: 40, ArrowUp: 38, Home: 36, End: 35, Escape: 27 }[key];
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", { key, keyCode, bubbles: true, cancelable: true }),
    );
    fixture.detectChanges();
  };
  return { answer, rows, press };
}

describe("AgentPicker", () => {
  it("has one tab stop and moves with the arrows, Home and End", () => {
    const { rows, press } = setup();
    expect(rows.map((r) => r.textContent?.trim())).toEqual(["Claude", "Codex", "Copilot"]);
    expect(rows.map((r) => r.tabIndex)).toEqual([0, -1, -1]);
    rows[0].focus();
    press("ArrowDown");
    expect(document.activeElement).toBe(rows[1]);
    press("End");
    expect(document.activeElement).toBe(rows[2]);
    press("ArrowUp");
    expect(document.activeElement).toBe(rows[1]);
    press("Home");
    expect(document.activeElement).toBe(rows[0]);
  });

  it("answers with the chosen row (a native button, so Enter clicks it)", async () => {
    const { answer, rows } = setup();
    expect(rows[1].tagName).toBe("BUTTON");
    rows[1].click();
    await expect(answer).resolves.toBe("p2");
  });

  it("cancels with Escape", async () => {
    const { answer, rows, press } = setup();
    rows[0].focus();
    press("Escape");
    await expect(answer).resolves.toBeNull();
  });
});
