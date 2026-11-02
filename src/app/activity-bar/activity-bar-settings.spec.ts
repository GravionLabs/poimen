// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { ActivityBar } from "./activity-bar";

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

describe("ActivityBar settings gear", () => {
  it("is an inline SVG button after the views that opens the settings", () => {
    TestBed.resetTestingModule();
    TestBed.overrideComponent(ActivityBar, {
      set: {
        templateUrl: undefined,
        styleUrl: undefined,
        template: readFileSync("src/app/activity-bar/activity-bar.html", "utf8"),
      },
    });
    const fixture = TestBed.createComponent(ActivityBar);
    const opened = vi.fn();
    fixture.componentInstance.settingsOpened.subscribe(opened);
    fixture.detectChanges();
    const host = fixture.nativeElement as HTMLElement;
    const gear = host.querySelector<HTMLButtonElement>("button.settings")!;
    expect(gear.getAttribute("aria-label")).toBe("Settings");
    expect(gear.querySelector("svg [stroke='currentColor']")).not.toBeNull();
    expect(gear.textContent?.trim()).toBe("");
    expect(host.querySelector("nav")!.nextElementSibling).toBe(gear);
    gear.click();
    expect(opened).toHaveBeenCalledOnce();
  });
});
