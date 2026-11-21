// @vitest-environment jsdom
import { getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { Injector, runInInjectionContext, signal } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { BUSY_DELAY_MS, SKELETON_DELAY_MS, delayed, delayedBusy, delayedSkeleton } from "./delayed";

describe("delayed", () => {
  beforeAll(() => {
    try {
      getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
    } catch {
      // already initialized
    }
  });
  beforeEach(() => TestBed.resetTestingModule());
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  function make(fn: (src: () => boolean) => () => boolean) {
    const source = signal(false);
    const shown = runInInjectionContext(TestBed.inject(Injector), () => fn(source));
    return { source, shown };
  }
  const flush = () => TestBed.tick();

  it("shows nothing before the delay and the flag after it", () => {
    const { source, shown } = make((s) => delayed(s, 200));
    source.set(true);
    flush();
    vi.advanceTimersByTime(199);
    expect(shown()).toBe(false);
    vi.advanceTimersByTime(1);
    expect(shown()).toBe(true);
  });

  it("never shows for a load that ends within the delay", () => {
    const { source, shown } = make(delayedSkeleton);
    source.set(true);
    flush();
    vi.advanceTimersByTime(SKELETON_DELAY_MS - 1);
    source.set(false);
    flush();
    vi.advanceTimersByTime(1000);
    expect(shown()).toBe(false);
  });

  it("hides at once when the source ends", () => {
    const { source, shown } = make(delayedSkeleton);
    source.set(true);
    flush();
    vi.advanceTimersByTime(SKELETON_DELAY_MS);
    expect(shown()).toBe(true);
    source.set(false);
    flush();
    expect(shown()).toBe(false);
  });

  it("uses 1 s for a started action", () => {
    const { source, shown } = make(delayedBusy);
    source.set(true);
    flush();
    vi.advanceTimersByTime(BUSY_DELAY_MS - 1);
    expect(shown()).toBe(false);
    vi.advanceTimersByTime(1);
    expect(shown()).toBe(true);
  });

  it("starts the delay again for a second load", () => {
    const { source, shown } = make(delayedSkeleton);
    source.set(true);
    flush();
    vi.advanceTimersByTime(SKELETON_DELAY_MS);
    source.set(false);
    flush();
    source.set(true);
    flush();
    expect(shown()).toBe(false);
    vi.advanceTimersByTime(SKELETON_DELAY_MS);
    expect(shown()).toBe(true);
  });
});
