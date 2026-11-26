// @vitest-environment jsdom
import { Component } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { Icon, type IconName } from "./icon";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});
afterEach(() => TestBed.resetTestingModule());

const KINDS: IconName[] = [
  "github",
  "runs",
  "space",
  "changes",
  "flow",
  "settings",
  "readOnly",
  "terminal",
];

function render(template: string): HTMLElement {
  @Component({ selector: "app-host", imports: [Icon], template })
  class Host {}
  const fixture = TestBed.createComponent(Host);
  fixture.detectChanges();
  return fixture.nativeElement as HTMLElement;
}

describe("Icon", () => {
  it.each(KINDS)("draws %s as stroked paths", (name) => {
    const el = render(`<app-icon name="${name}" />`);
    expect(el.querySelectorAll("svg path").length).toBeGreaterThan(0);
    expect(el.querySelector("svg")?.getAttribute("stroke")).toBe("currentColor");
  });

  it("draws a monogram badge with its letters and a label (#1215)", () => {
    const el = render(`<app-icon name="agent:CC" label="Claude Code" [size]="16" />`);
    expect(el.querySelector("svg rect")).not.toBeNull();
    expect(el.querySelector("svg text")?.textContent?.trim()).toBe("CC");
    // Wide capitals ("GM", "MW") stay inside the frame in every font.
    expect(el.querySelector("svg text")?.getAttribute("textLength")).toBe("14");
    expect(el.querySelector("app-icon")?.getAttribute("aria-label")).toBe("Claude Code");
  });

  it("draws the shell in the frame of the agent badges (#1314)", () => {
    const paths = render(`<app-icon name="terminal" />`).querySelectorAll("svg path");
    expect(paths[0].getAttribute("d")).toMatch(/^M7 2h10a5 5/);
    expect(paths.length).toBe(3);
  });

  it("draws the lock", () => {
    expect(render(`<app-icon name="lock" />`).querySelectorAll("svg path").length).toBe(2);
  });

  it("is hidden from assistive technology without a label", () => {
    const icon = render(`<app-icon name="github" />`).querySelector("app-icon")!;
    expect(icon.getAttribute("aria-hidden")).toBe("true");
    expect(icon.getAttribute("role")).toBeNull();
  });

  it("with a label is an image with that name and tooltip, at the given size", () => {
    const icon = render(`<app-icon name="runs" label="Runs" [size]="16" />`).querySelector(
      "app-icon",
    ) as HTMLElement;
    expect(icon.getAttribute("role")).toBe("img");
    expect(icon.getAttribute("aria-label")).toBe("Runs");
    expect(icon.getAttribute("title")).toBe("Runs");
    expect(icon.getAttribute("aria-hidden")).toBeNull();
    expect(icon.style.width).toBe("16px");
  });

  it("still draws the filled Material icons", () => {
    const el = render(`<app-icon name="close" />`);
    expect(el.querySelector("svg")?.getAttribute("fill")).toBe("currentColor");
  });
});
