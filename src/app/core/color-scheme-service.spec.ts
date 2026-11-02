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
  vi.restoreAllMocks();
});

function service(): ColorSchemeService {
  TestBed.configureTestingModule({ providers: [{ provide: HxTheme, useValue: { setDark } }] });
  return TestBed.inject(ColorSchemeService);
}

const root = () => document.documentElement.style;

describe("ColorSchemeService", () => {
  it("sets the derived tokens and the appearance of a scheme", () => {
    expect(service().sync("dracula")).toBe(true);
    expect(root().getPropertyValue("--term-bg")).toBe("#282a36");
    expect(setDark).toHaveBeenLastCalledWith(true);
  });

  it("uses light for a light scheme", () => {
    service().sync("one-light");
    expect(setDark).toHaveBeenLastCalledWith(false);
  });

  it("changes scheme without a restart", () => {
    const s = service();
    s.sync("dracula");
    s.sync("one-light");
    expect(root().getPropertyValue("--term-bg")).toBe("#fafafa");
  });

  it("changes nothing without a name or for poimen", () => {
    const s = service();
    expect(s.sync(null)).toBe(false);
    expect(s.sync("poimen")).toBe(false);
    expect(root().length).toBe(0);
    expect(setDark).not.toHaveBeenCalled();
  });

  it("removes the tokens again when the name goes away", () => {
    const s = service();
    s.sync("dracula");
    expect(s.sync(null)).toBe(false);
    expect(root().length).toBe(0);
  });

  it("warns once and falls back to poimen for an unknown name", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const s = service();
    expect(s.sync("nope")).toBe(false);
    expect(s.sync("nope")).toBe(false);
    expect(warn).toHaveBeenCalledOnce();
    expect(root().length).toBe(0);
  });
});
