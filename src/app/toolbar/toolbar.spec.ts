// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { Component, signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it } from "vitest";
import type { PluginAction } from "../core/herdr";
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
      pluginsOpen = signal(false);
      plugins = () => [];
      pluginsError = () => null;
      togglePlugins = () => undefined;
      label = themeLabel;
      next = nextTheme;
    }
    TestBed.resetTestingModule();
    const fixture = TestBed.createComponent(Host);
    fixture.detectChanges();
    const button = (fixture.nativeElement as HTMLElement).querySelector(".theme-button")!;
    return { button: button as HTMLButtonElement, emitted };
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

describe("Plugins menu", () => {
  const act = (id: string, title: string): PluginAction => ({
    plugin_id: "p",
    action_id: id,
    title,
    description: null,
  });

  const render = (plugins: PluginAction[], error: string | null = null) => {
    const opened: number[] = [];
    const chosen: PluginAction[] = [];
    @Component({
      selector: "app-host",
      template: readFileSync("src/app/toolbar/toolbar.html", "utf8"),
    })
    class Host {
      theme = () => "system";
      fixedBy = () => null;
      fixed = fixedLabel;
      themeChange = { emit: () => undefined };
      label = themeLabel;
      next = nextTheme;
      pluginsOpen = signal(false);
      plugins = () => plugins;
      pluginsError = () => error;
      togglePlugins() {
        this.pluginsOpen.update((o) => !o);
        if (this.pluginsOpen()) opened.push(1);
      }
      choosePlugin(a: PluginAction) {
        this.pluginsOpen.set(false);
        chosen.push(a);
      }
    }
    TestBed.resetTestingModule();
    const fixture = TestBed.createComponent(Host);
    fixture.detectChanges();
    const root = fixture.nativeElement as HTMLElement;
    const button = root.querySelector<HTMLButtonElement>("button[aria-haspopup=menu]")!;
    return { fixture, root, button, opened, chosen };
  };

  it("lists the actions after opening and reports the pick", () => {
    const first = act("a", "First");
    const { fixture, root, button, opened, chosen } = render([first, act("b", "Second")]);
    expect(root.querySelector("[role=menu]")).toBeNull();
    button.click();
    fixture.detectChanges();
    const items = [...root.querySelectorAll<HTMLButtonElement>("[role=menuitem]")];
    expect(items.map((i) => i.textContent?.replace(/\s+/g, " ").trim())).toEqual([
      "First p",
      "Second p",
    ]);
    expect(opened).toHaveLength(1);
    items[0].click();
    fixture.detectChanges();
    expect(chosen).toEqual([first]);
    expect(root.querySelector("[role=menu]")).toBeNull();
  });

  it("says why nothing is listed", () => {
    const { fixture, root, button } = render([], "herdr socket: refused");
    button.click();
    fixture.detectChanges();
    expect(root.querySelector("[role=menu]")?.textContent).toContain("herdr socket: refused");
  });

  it("says there are no actions when herdr lists none", () => {
    const { fixture, root, button } = render([]);
    button.click();
    fixture.detectChanges();
    expect(root.querySelector("[role=menu]")?.textContent).toContain("No plugin actions");
  });
});
