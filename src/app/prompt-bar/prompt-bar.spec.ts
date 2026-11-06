// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { getTestBed, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { AgentPrompts, type PromptOutcome } from "../core/agent-prompt";
import type { Terminal } from "../core/herdr";
import { PromptBar } from "./prompt-bar";

const pane: Terminal = {
  terminal_id: "w1:p2",
  tab_id: "w1:t1",
  space_id: "w1",
  agent: "claude",
  agent_status: "idle",
  cwd: "/work/app",
};

async function mount(outcome: PromptOutcome) {
  const send = vi.fn(async () => outcome);
  TestBed.configureTestingModule({ providers: [{ provide: AgentPrompts, useValue: { send } }] });
  TestBed.overrideComponent(PromptBar, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      imports: [],
      template: readFileSync("src/app/prompt-bar/prompt-bar.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(PromptBar);
  // JIT without the Angular compiler transform cannot resolve signal inputs.
  Object.assign(fixture.componentInstance, { pane: signal(pane) });
  fixture.detectChanges();
  const el: HTMLElement = fixture.nativeElement;
  const input = el.querySelector("input")!;
  const submit = async (text: string) => {
    input.value = text;
    input.dispatchEvent(new Event("input"));
    fixture.detectChanges();
    el.querySelector("form")!.dispatchEvent(new Event("submit"));
    await fixture.whenStable();
    fixture.detectChanges();
  };
  return { send, el, input, submit };
}

describe("PromptBar", () => {
  afterEach(() => TestBed.resetTestingModule());
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
      errorOnUnknownElements: true,
    });
  });

  it("sends the trimmed text to the pane and clears the input", async () => {
    const { send, el, input, submit } = await mount({ kind: "started", status: "working" });
    await submit("  fix the test ");
    expect(send).toHaveBeenCalledWith("w1:p2", "fix the test");
    expect(input.value).toBe("");
    expect(el.querySelector(".result")!.textContent).toContain("working");
  });

  it("shows a stalled result", async () => {
    const { el, submit } = await mount({ kind: "stalled" });
    await submit("go");
    expect(el.querySelector(".result")!.textContent).toContain("stalled");
  });

  it("shows a timeout and keeps the text for a retry", async () => {
    const { el, input, submit } = await mount({ kind: "timeout" });
    await submit("go");
    expect(el.querySelector(".result")!.textContent).toContain("Timed out");
    expect(el.querySelector(".result")!.classList).toContain("failed");
    expect(input.value).toBe("go");
  });

  it("sends nothing for blank text", async () => {
    const { send, submit } = await mount({ kind: "stalled" });
    await submit("   ");
    expect(send).not.toHaveBeenCalled();
  });
});
