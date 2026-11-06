// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import type { FlowNode, NodeTokens } from "../core/flow-graph";
import { FlowNodeTokens } from "./flow-node-tokens";

function node(over: Partial<FlowNode> = {}): FlowNode {
  return {
    id: "main",
    kind: "main",
    title: "claude",
    type: "",
    status: "running",
    startedMs: null,
    endedMs: null,
    tools: 0,
    agentId: null,
    tokens: null,
    model: null,
    context: null,
    ...over,
  };
}

const tokens: NodeTokens = {
  input: 12300,
  output: 842,
  rawInput: 12300,
  cacheRead: 0,
  cacheCreation: 0,
};

function render(n: FlowNode): HTMLElement {
  const fixture = TestBed.createComponent(FlowNodeTokens);
  fixture.componentRef.setInput("node", n);
  fixture.detectChanges();
  return fixture.nativeElement as HTMLElement;
}

describe("flow node token row", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("is absent without usage, with no zeros", () => {
    const el = render(node());
    expect(el.querySelector(".tokens")).toBeNull();
    expect(el.textContent?.trim()).toBe("");
  });

  it("shows the counts and the context fill when set", () => {
    const el = render(node({ tokens, context: 5000 }));
    const text = el.querySelector(".tokens")?.textContent ?? "";
    expect(text).toContain("12.3k in · 842 out");
    expect(text).toContain("context 5");
  });

  it("omits the context when unknown", () => {
    const el = render(node({ tokens }));
    expect(el.querySelectorAll(".tokens span")).toHaveLength(1);
  });
});
