// @vitest-environment jsdom
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { PRESENCE_MS, Presence } from "./presence";

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

describe("Presence", () => {
  let focus = true;
  beforeEach(() => {
    vi.useFakeTimers();
    focus = true;
    vi.spyOn(document, "hasFocus").mockImplementation(() => focus);
  });
  afterEach(() => {
    TestBed.resetTestingModule();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("is not present on focus alone, present after input, idle after 60 s", () => {
    const p = TestBed.inject(Presence);
    expect(p.present()).toBe(false);
    window.dispatchEvent(new Event("focus"));
    expect(p.present()).toBe(false);
    window.dispatchEvent(new Event("keydown"));
    expect(p.present()).toBe(true);
    vi.advanceTimersByTime(PRESENCE_MS - 1);
    expect(p.present()).toBe(true);
    vi.advanceTimersByTime(1);
    expect(p.present()).toBe(false);
  });

  it("sees keydown in the capture phase even when a target stops it", () => {
    const p = TestBed.inject(Presence);
    const el = document.createElement("div");
    el.addEventListener("keydown", (e) => e.stopPropagation());
    document.body.append(el);
    el.dispatchEvent(new Event("keydown", { bubbles: true }));
    expect(p.present()).toBe(true);
    el.remove();
  });

  it("is absent after blur and ignores input without the focus", () => {
    const p = TestBed.inject(Presence);
    window.dispatchEvent(new Event("wheel"));
    expect(p.present()).toBe(true);
    window.dispatchEvent(new Event("blur"));
    expect(p.present()).toBe(false);
    focus = false;
    window.dispatchEvent(new Event("pointermove"));
    expect(p.present()).toBe(false);
  });
});
