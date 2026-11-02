// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { NgTemplateOutlet } from "@angular/common";
import { Component } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

// JIT in vitest does not wire signal inputs, so the real template is rendered by a stand-in host
// that provides the members the template reads.
const tabs = [
  { id: "a", label: "terminal", group: "alpha" },
  { id: "b", label: "terminal", group: "beta" },
  { id: "c", label: "solo" },
];

describe("TabBar close button name", () => {
  it("includes the group so equal labels in two workspaces differ", () => {
    @Component({
      selector: "app-host",
      imports: [NgTemplateOutlet],
      template: readFileSync("src/app/ui/helix-candidates/tab-bar/tab-bar.html", "utf8"),
    })
    class Host {
      tabs = () => tabs;
      activeId = () => "a";
      tabContent = () => undefined;
      dragging = () => null;
      dropAt = () => null;
      foreignAt = () => null;
      menuAt = () => null;
      menu = () => [];
    }
    TestBed.resetTestingModule();
    const fixture = TestBed.createComponent(Host);
    fixture.detectChanges();
    const names = [...(fixture.nativeElement as HTMLElement).querySelectorAll(".close")].map((b) =>
      b.getAttribute("aria-label"),
    );
    expect(names).toEqual(["Close alpha / terminal", "Close beta / terminal", "Close solo"]);
  });
});
