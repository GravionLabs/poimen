// @vitest-environment jsdom
import { createComponent, createEnvironmentInjector, EnvironmentInjector } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import { FIXTURES, findFixture } from "./registry";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized
  }
});

describe("fixture registry", () => {
  it("has unique ids of the form <component>/<state>", () => {
    const ids = FIXTURES.map((f) => f.id);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(id).toMatch(/^[a-z-]+\/[a-z-]+$/);
  });

  it("finds a fixture by id", () => {
    expect(findFixture(FIXTURES[0].id)).toBe(FIXTURES[0]);
    expect(findFixture("nope/none")).toBeUndefined();
  });

  // Specs run without the Angular compiler transform, so signal inputs (status bar) are not wired:
  // that fixture is checked by `pnpm fixtures:shot`, not here.
  it.each(FIXTURES.filter((f) => !f.id.startsWith("status-bar/")).map((f) => [f.id, f] as const))(
    "%s renders without a backend",
    (_id, f) => {
      TestBed.resetTestingModule();
      // specs do not load stylesheets (see vitest.config.mts)
      TestBed.overrideComponent(f.component, { set: { styleUrl: undefined } });
      const injector = createEnvironmentInjector(
        f.providers ?? [],
        TestBed.inject(EnvironmentInjector),
      );
      const c = createComponent(f.component, { environmentInjector: injector });
      for (const [k, v] of Object.entries(f.inputs ?? {})) c.setInput(k, v);
      c.changeDetectorRef.detectChanges();
      expect((c.location.nativeElement as HTMLElement).childElementCount).toBeGreaterThan(0);
    },
  );
});
