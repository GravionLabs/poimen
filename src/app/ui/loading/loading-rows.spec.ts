// @vitest-environment jsdom
import { getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { TestBed } from "@angular/core/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { LoadingRows } from "./loading-rows";

describe("LoadingRows", () => {
  beforeAll(() => {
    try {
      getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
    } catch {
      // already initialized
    }
  });
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("renders skeleton rows only after 200 ms", () => {
    const fixture = TestBed.createComponent(LoadingRows);
    fixture.componentInstance.rows = 3;
    fixture.componentInstance.active = true;
    fixture.detectChanges();
    vi.advanceTimersByTime(199);
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector("hx-skeleton")).toBeNull();
    vi.advanceTimersByTime(1);
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelectorAll("hx-skeleton").length).toBe(3);
    fixture.componentInstance.active = false;
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector("hx-skeleton")).toBeNull();
  });
});
