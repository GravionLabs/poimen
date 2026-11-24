// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { AgentGrants, type GrantRequest } from "../core/agent-grants";
import { expectFocusInside, focusOpener, press } from "../ui/dialog-shell/dialog-contract";
import { AgentGrantDialog } from "./agent-grant-dialog";

function setup(request: GrantRequest | null) {
  const grants = {
    current: signal(request),
    waiting: signal(request ? 1 : 0),
    error: signal<string | null>(null),
    answer: vi.fn(),
  };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({ providers: [{ provide: AgentGrants, useValue: grants }] });
  TestBed.overrideComponent(AgentGrantDialog, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/agent-grant-dialog/agent-grant-dialog.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(AgentGrantDialog);
  fixture.detectChanges();
  return { grants, fixture, root: fixture.nativeElement as HTMLElement };
}

const request: GrantRequest = {
  id: "g1",
  leadName: "lead",
  plan: "ship M016 in api and web",
  spaces: [
    { id: "s1", name: "api", path: "/w/api" },
    { id: "s2", name: "web" },
  ],
  tools: ["start_agent", "prompt_agent"],
  maxAgents: 3,
  durationS: 28800,
};

describe("AgentGrantDialog", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      teardown: { destroyAfterEach: true },
    });
  });

  it("shows nothing without a request", () => {
    expect(setup(null).root.querySelector(".dialog")).toBeNull();
  });

  it("shows the agent, the plan, the spaces with paths, the tools, the limit and the end", () => {
    const { root } = setup(request);
    expect(root.querySelector("h2")?.textContent).toContain("lead");
    expect(root.querySelector(".text")?.textContent).toBe("ship M016 in api and web");
    const spaces = [...root.querySelectorAll(".spaces li")].map((li) => li.textContent?.trim());
    expect(spaces[0]).toContain("/w/api");
    expect(spaces[1]).toBe("web");
    expect(root.textContent).toContain("start_agent, prompt_agent");
    expect(root.querySelector<HTMLInputElement>(".limit input")?.value).toBe("3");
    expect(root.textContent).toContain("8 hours");
  });

  it("allows the whole plan as asked", () => {
    const { root, grants } = setup(request);
    root.querySelector<HTMLButtonElement>(".allow")!.click();
    expect(grants.answer).toHaveBeenCalledWith(true, undefined, undefined);
  });

  it("narrows: a space left out and a lower limit, never a higher one", () => {
    const { root, grants, fixture } = setup(request);
    root.querySelectorAll<HTMLInputElement>(".spaces input")[1]!.click();
    const limit = root.querySelector<HTMLInputElement>(".limit input")!;
    limit.value = "2";
    limit.dispatchEvent(new Event("input"));
    fixture.detectChanges();
    root.querySelector<HTMLButtonElement>(".allow")!.click();
    expect(grants.answer).toHaveBeenLastCalledWith(true, ["s1"], 2);
    limit.value = "9";
    limit.dispatchEvent(new Event("input"));
    fixture.detectChanges();
    root.querySelector<HTMLButtonElement>(".allow")!.click();
    expect(grants.answer).toHaveBeenLastCalledWith(true, ["s1"], undefined);
  });

  it("cannot allow with no space left", () => {
    const { root, grants, fixture } = setup(request);
    for (const box of root.querySelectorAll<HTMLInputElement>(".spaces input")) box.click();
    fixture.detectChanges();
    const allow = root.querySelector<HTMLButtonElement>(".allow")!;
    expect(allow.disabled).toBe(true);
    allow.click();
    expect(grants.answer).not.toHaveBeenCalled();
  });

  it("denies on the Deny button, the backdrop and Escape", () => {
    const { root, grants } = setup(request);
    root.querySelector<HTMLButtonElement>(".deny")!.click();
    expect(grants.answer).toHaveBeenLastCalledWith(false);
    root.querySelector<HTMLElement>("dialog")!.click();
    expect(grants.answer).toHaveBeenCalledTimes(2);
  });

  it("takes the focus on Cancel, Escape denies, Enter does not allow, the focus goes back", async () => {
    const opener = focusOpener();
    const { grants, fixture, root } = setup(request);
    await fixture.whenStable();
    expectFocusInside(root);
    press("Enter", { key: "Enter" });
    expect(grants.answer).not.toHaveBeenCalledWith(true, expect.anything(), expect.anything());
    press("Escape");
    expect(grants.answer).toHaveBeenLastCalledWith(false);
    grants.current.set(null);
    fixture.detectChanges();
    expect(document.activeElement).toBe(opener);
  });
});
