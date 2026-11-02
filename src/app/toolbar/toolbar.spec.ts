// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { Component } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import { nextTheme, type ThemeSetting } from "../core/settings";
import { fixedLabel, themeLabel } from "./toolbar";

beforeAll(() => {
  try {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by the shared setup
  }
});

describe("themeLabel", () => {
  it("states the current theme and the next one", () => {
    expect(themeLabel("system")).toBe("Theme: system (click for light)");
    expect(themeLabel("light")).toBe("Theme: light (click for dark)");
    expect(themeLabel("dark")).toBe("Theme: dark (click for system)");
  });
});

describe("theme button", () => {
  const render = (theme: ThemeSetting, fixedBy: string | null = null) => {
    const emitted: ThemeSetting[] = [];
    @Component({
      selector: "app-host",
      template: readFileSync("src/app/toolbar/toolbar.html", "utf8"),
    })
    class Host {
      theme = () => theme;
      fixedBy = () => fixedBy;
      fixed = fixedLabel;
      themeChange = { emit: (t: ThemeSetting) => emitted.push(t) };
      label = themeLabel;
      next = nextTheme;
    }
    TestBed.resetTestingModule();
    const fixture = TestBed.createComponent(Host);
    fixture.detectChanges();
    const button = (fixture.nativeElement as HTMLElement).querySelector("button")!;
    return { button, emitted };
  };

  it.each([
    ["system", "auto", "light"],
    ["light", "sun", "dark"],
    ["dark", "moon", "system"],
  ] as const)("%s shows the %s icon and cycles to %s", (theme, icon, next) => {
    const { button, emitted } = render(theme);
    expect(button.tagName).toBe("BUTTON");
    expect(button.querySelector(`svg.${icon}`)).not.toBeNull();
    expect(button.getAttribute("aria-label")).toBe(themeLabel(theme));
    expect(button.title).toBe(themeLabel(theme));
    button.click();
    expect(emitted).toEqual([next]);
  });

  it("is disabled with a tooltip naming the color schema that fixes the theme", () => {
    const { button, emitted } = render("system", "dracula");
    expect(button.disabled).toBe(true);
    expect(button.title).toBe("Theme fixed by color schema dracula");
    expect(button.getAttribute("aria-label")).toBe("Theme fixed by color schema dracula");
    button.click();
    expect(emitted).toEqual([]);
  });

  it("is enabled without a fixing color schema", () => {
    expect(render("light").button.disabled).toBe(false);
  });
});
