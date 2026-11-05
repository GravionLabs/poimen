// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { Component } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import { TabBar } from "./tab-bar";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

const tabs = [
  { id: "a", label: "terminal", group: "alpha" },
  { id: "b", label: "terminal", group: "beta" },
  { id: "c", label: "solo" },
];

describe("TabBar close button name", () => {
  it("includes the group so equal labels in two workspaces differ", () => {
    @Component({
      selector: "app-host",
      imports: [TabBar],
      template: `<app-tab-bar [tabs]="tabs" activeId="a" />`,
    })
    class Host {
      tabs = tabs;
    }
    TestBed.resetTestingModule();
    TestBed.overrideComponent(TabBar, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/ui/helix-candidates/tab-bar/tab-bar.html", "utf8"),
      },
    });
    const fixture = TestBed.createComponent(Host);
    fixture.detectChanges();
    const names = [...(fixture.nativeElement as HTMLElement).querySelectorAll(".close")].map((b) =>
      b.getAttribute("aria-label"),
    );
    expect(names).toEqual(["Close alpha / terminal", "Close beta / terminal", "Close solo"]);
  });
});
