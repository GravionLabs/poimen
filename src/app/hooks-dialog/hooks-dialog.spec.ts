// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { HooksOffer } from "../core/hooks-offer";
import { HooksDialog } from "./hooks-dialog";

function setup(missing: string[], error: string | null = null) {
  const offer = {
    missing: signal(missing),
    visible: signal(missing.length > 0),
    busy: signal(false),
    error: signal(error),
    install: vi.fn(),
    dismiss: vi.fn(),
  };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({ providers: [{ provide: HooksOffer, useValue: offer }] });
  TestBed.overrideComponent(HooksDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/hooks-dialog/hooks-dialog.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(HooksDialog);
  fixture.detectChanges();
  return { offer, root: fixture.nativeElement as HTMLElement };
}

describe("HooksDialog", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("renders nothing without an offer", () => {
    expect(setup([]).root.querySelector("[role=dialog]")).toBeNull();
  });

  it("names the agents and wires both buttons", () => {
    const { offer, root } = setup(["claude", "copilot"]);
    expect(root.querySelectorAll(".agents li").length).toBe(2);
    expect(root.textContent).toContain("Claude Code");
    root.querySelector<HTMLButtonElement>(".confirm")!.click();
    expect(offer.install).toHaveBeenCalled();
    root.querySelector<HTMLButtonElement>(".cancel")!.click();
    expect(offer.dismiss).toHaveBeenCalled();
  });

  it("shows an install error", () => {
    expect(setup(["claude"], "boom").root.querySelector(".error")?.textContent).toContain("boom");
  });
});
