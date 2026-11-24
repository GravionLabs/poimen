// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { AgentConfirm, type ConfirmRequest } from "../core/agent-confirm";
import { AgentGrantDialog } from "../agent-grant-dialog/agent-grant-dialog";
import { AgentConfirmDialog } from "./agent-confirm-dialog";
import { expectFocusInside, focusOpener, press } from "../ui/dialog-shell/dialog-contract";

function setup(request: ConfirmRequest | null) {
  const confirm = {
    current: signal(request),
    waiting: signal(request ? 1 : 0),
    error: signal<string | null>(null),
    answer: vi.fn(),
  };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({ providers: [{ provide: AgentConfirm, useValue: confirm }] });
  TestBed.overrideComponent(AgentConfirmDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/agent-confirm-dialog/agent-confirm-dialog.html", "utf8"),
    },
  });
  // The grant dialog is its own spec; here it only has to exist.
  TestBed.overrideComponent(AgentGrantDialog, {
    set: { templateUrl: undefined, styleUrl: undefined, template: "" },
  });
  const fixture = TestBed.createComponent(AgentConfirmDialog);
  fixture.detectChanges();
  return { confirm, fixture, root: fixture.nativeElement as HTMLElement };
}

const request: ConfirmRequest = {
  id: "c1",
  callerName: "lead",
  action: "prompt_agent",
  spaceName: "api",
  text: "run the tests",
};

describe("AgentConfirmDialog", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("shows nothing without a question", () => {
    expect(setup(null).root.querySelector(".dialog")).toBeNull();
  });

  it("names who asks, what and where, and answers per button", () => {
    const { root, confirm } = setup(request);
    expect(root.querySelector("h2")?.textContent).toMatch(
      /lead\s+wants to prompt an agent\s+in\s+api/,
    );
    expect(root.querySelector(".text")?.textContent).toBe("run the tests");
    root.querySelector<HTMLButtonElement>(".allow")!.click();
    expect(confirm.answer).toHaveBeenLastCalledWith(true);
    root.querySelector<HTMLButtonElement>(".decline")!.click();
    expect(confirm.answer).toHaveBeenLastCalledWith(false);
  });

  it("declines on the backdrop and on Escape", () => {
    const { root, confirm } = setup({ ...request, action: "start_agent", text: "claude" });
    expect(root.querySelector("h2")?.textContent).toContain("start an agent");
    root.querySelector<HTMLElement>("dialog")!.click();
    expect(confirm.answer).toHaveBeenLastCalledWith(false);
    root
      .querySelector<HTMLElement>("dialog")!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    expect(confirm.answer).toHaveBeenCalledTimes(2);
  });

  it("takes the focus on Decline, Escape declines, Enter does not allow, the focus goes back", async () => {
    const opener = focusOpener();
    const { confirm, fixture, root } = setup(request);
    await fixture.whenStable();
    expectFocusInside(root);
    expect(document.activeElement?.textContent).toContain("Decline");
    press("Enter", { key: "Enter" });
    expect(confirm.answer).not.toHaveBeenCalledWith(true);
    press("Escape");
    expect(confirm.answer).toHaveBeenLastCalledWith(false);
    confirm.current.set(null);
    fixture.detectChanges();
    expect(document.activeElement).toBe(opener);
  });
});
