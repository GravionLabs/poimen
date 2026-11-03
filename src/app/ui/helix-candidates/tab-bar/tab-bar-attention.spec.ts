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

const tabs = [
  { id: "a", label: "blocked", attention: true },
  { id: "b", label: "finished", unread: true },
  { id: "c", label: "calm" },
];

describe("TabBar attention and unread marks", () => {
  it("rings a blocked tab and badges an unread one", () => {
    @Component({
      selector: "app-host",
      imports: [NgTemplateOutlet],
      template: readFileSync("src/app/ui/helix-candidates/tab-bar/tab-bar.html", "utf8"),
    })
    class Host {
      tabs = () => tabs;
      activeId = () => "c";
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
    const root = fixture.nativeElement as HTMLElement;
    const rows = [...root.querySelectorAll(".tab")];
    expect(rows.map((t) => t.classList.contains("attention"))).toEqual([true, false, false]);
    expect(rows.map((t) => t.querySelector(".unread") !== null)).toEqual([false, true, false]);
  });
});
