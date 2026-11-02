// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { HxTheme } from "@gravionlabs/helix-ui";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { ColorSchemeService } from "./color-scheme-service";

const setDark = vi.fn();

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting(), {
    teardown: { destroyAfterEach: true },
  });
});

afterEach(() => {
  TestBed.resetTestingModule();
  setDark.mockReset();
  document.documentElement.removeAttribute("style");
});

function service(): ColorSchemeService {
  TestBed.configureTestingModule({ providers: [{ provide: HxTheme, useValue: { setDark } }] });
  return TestBed.inject(ColorSchemeService);
}

const bg = () => document.documentElement.style.getPropertyValue("--term-bg");

describe("ColorSchemeService preview", () => {
  it("shows a scheme without saving it and signals when the preview ends", () => {
    const s = service();
    s.sync("dracula");
    s.preview("one-light");
    expect(bg()).toBe("#fafafa");
    expect(setDark).toHaveBeenLastCalledWith(false);
    const before = s.restored();
    s.revert();
    expect(s.restored()).toBe(before + 1);
  });

  it("ignores an unknown name", () => {
    const s = service();
    s.sync("dracula");
    s.preview("nope");
    expect(bg()).toBe("#282a36");
  });
});
