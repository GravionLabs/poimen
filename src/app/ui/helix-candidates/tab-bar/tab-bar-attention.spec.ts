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
  { id: "a", label: "blocked", attention: true },
  { id: "b", label: "finished", unread: true },
  { id: "c", label: "calm" },
  { id: "d", label: "old host", note: "Runs on poimen-pty-host 0.15.0; bundled is 0.16.0" },
];

describe("TabBar attention and unread marks", () => {
  it("rings a blocked tab and badges an unread one", () => {
    @Component({
      selector: "app-host",
      imports: [TabBar],
      template: `<app-tab-bar [tabs]="tabs" activeId="c" />`,
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
    const root = fixture.nativeElement as HTMLElement;
    const rows = [...root.querySelectorAll(".tab")];
    expect(rows.map((t) => t.classList.contains("attention"))).toEqual([true, false, false, false]);
    expect(rows.map((t) => t.querySelector(".unread") !== null)).toEqual([
      false,
      true,
      false,
      false,
    ]);
    expect(rows.map((t) => t.querySelector(".note") !== null)).toEqual([false, false, false, true]);
    expect(rows[3].querySelector<HTMLElement>(".note")!.title).toContain("0.15.0");
  });
});
