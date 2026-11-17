// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { ResultDialog as ResultState } from "../core/result-dialog";
import { expectFocusInside, focusOpener, press } from "../ui/dialog-shell/dialog-contract";
import { ResultDialog } from "./result-dialog";

function setup(action?: { label: string; run: () => void }) {
  TestBed.resetTestingModule();
  TestBed.overrideComponent(ResultDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/result-dialog/result-dialog.html", "utf8"),
    },
  });
  const state = TestBed.inject(ResultState);
  const fixture = TestBed.createComponent(ResultDialog);
  fixture.detectChanges();
  state.show({ title: "Teardown", summary: "It ran.", text: "out", action });
  fixture.detectChanges();
  return { state, fixture, root: fixture.nativeElement as HTMLElement };
}

describe("ResultDialog", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("shows nothing until a result is shown", () => {
    const { state, fixture, root } = setup();
    state.close();
    fixture.detectChanges();
    expect(root.querySelector("dialog")).toBeNull();
  });

  it("takes the focus on Close, Escape closes and the focus goes back", async () => {
    const opener = focusOpener();
    const { state, fixture, root } = setup();
    await fixture.whenStable();
    expectFocusInside(root);
    expect(root.querySelector("h2")?.textContent).toBe("Teardown");
    expect(document.activeElement?.textContent).toContain("Close");
    press("Escape");
    expect(state.content()).toBeNull();
    fixture.detectChanges();
    expect(document.activeElement).toBe(opener);
  });

  it("with an action the action is the primary button on the right and has the focus", async () => {
    const run = vi.fn();
    const { fixture, root } = setup({ label: "Run again", run });
    await fixture.whenStable();
    const buttons = [...root.querySelectorAll<HTMLButtonElement>(".dialog-buttons button")];
    expect(buttons.map((b) => b.textContent?.trim())).toEqual(["Close", "Run again"]);
    expect(document.activeElement).toBe(buttons[1]);
  });
});
