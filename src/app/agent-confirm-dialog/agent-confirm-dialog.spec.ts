// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { AgentConfirm, type ConfirmRequest } from "../core/agent-confirm";
import { AgentConfirmDialog } from "./agent-confirm-dialog";

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
  const fixture = TestBed.createComponent(AgentConfirmDialog);
  fixture.detectChanges();
  return { confirm, root: fixture.nativeElement as HTMLElement };
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
    root.querySelector<HTMLButtonElement>(".backdrop")!.click();
    expect(confirm.answer).toHaveBeenLastCalledWith(false);
    root
      .querySelector<HTMLElement>(".dialog")!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(confirm.answer).toHaveBeenCalledTimes(2);
  });
});
