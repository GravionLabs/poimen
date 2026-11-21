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
