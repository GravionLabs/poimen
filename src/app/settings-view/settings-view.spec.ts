// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { HxTheme } from "@gravionlabs/helix-ui";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { bundled } from "../core/color-scheme";
import { ColorSchemeService } from "../core/color-scheme-service";
import { DEFAULT_SETTINGS, parseHarnesses, SettingsStore } from "../core/settings";
import { tauri } from "../core/tauri.mock";
import { TauriBridge } from "../core/tauri";
import {
  chosen,
  groupSchemes,
  moved,
  pairOf,
  selectedIn,
  SettingsView,
  withRoot,
} from "./settings-view";
import { SpaceLayoutEditor } from "./space-layout-editor";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

describe("scheme helpers", () => {
  it("groups the registry by appearance, sorted by label", () => {
    const { dark, light } = groupSchemes(bundled);
    expect(dark.map((s) => s.name)).toContain("dracula");
    expect(light.map((s) => s.name)).toContain("one-light");
    expect(dark.map((s) => s.name)).not.toContain("one-light");
    const labels = dark.map((s) => s.label);
    expect(labels).toEqual([...labels].sort((a, b) => a.localeCompare(b)));
  });

  it("selects a single scheme only in the list of its appearance", () => {
    expect(selectedIn("dracula", "dark", bundled)).toBe("dracula");
    expect(selectedIn("dracula", "light", bundled)).toBeNull();
    expect(selectedIn("unknown", "dark", bundled)).toBeNull();
    expect(selectedIn(null, "dark", bundled)).toBeNull();
  });

  it("selects each side of a pair", () => {
    const pair = { light: "one-light", dark: "one-dark" };
    expect(selectedIn(pair, "light", bundled)).toBe("one-light");
    expect(selectedIn(pair, "dark", bundled)).toBe("one-dark");
  });

  it("builds a pair that keeps a single scheme on its side", () => {
    expect(pairOf("dracula", "dark")).toEqual({ light: "poimen-light", dark: "dracula" });
    expect(pairOf("one-light", "light")).toEqual({ light: "one-light", dark: "poimen-dark" });
    expect(pairOf(null, "dark")).toEqual({ light: "poimen-light", dark: "poimen-dark" });
  });

  it("changes one side of a pair and replaces a single scheme", () => {
    const pair = { light: "one-light", dark: "one-dark" };
    expect(chosen(pair, "pair", "dark", "dracula")).toEqual({
      light: "one-light",
      dark: "dracula",
    });
    expect(chosen("dracula", "single", "light", "one-light")).toBe("one-light");
    expect(chosen(null, "pair", "light", "gruvbox-light")).toEqual({
      light: "gruvbox-light",
      dark: "poimen-dark",
    });
  });
});

describe("explorer root helpers", () => {
  it("moves an item and clamps at the ends", () => {
    expect(moved(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
    expect(moved(["a", "b", "c"], 2, -1)).toEqual(["a", "c", "b"]);
    expect(moved(["a", "b"], 0, -1)).toEqual(["a", "b"]);
  });

  it("appends a new root once", () => {
    expect(withRoot(["~"], "/srv")).toEqual(["~", "/srv"]);
    expect(withRoot(["~", "/srv"], "/srv")).toEqual(["~", "/srv"]);
  });

  it("treats the absolute and the ~ spelling of a root as the same", () => {
    const home = "/Users/x";
    expect(withRoot(["~/Source"], "/Users/x/Source", home)).toEqual(["~/Source"]);
    expect(withRoot(["~"], "/Users/x", home)).toEqual(["~"]);
    expect(withRoot(["~"], "/Users/x/", home)).toEqual(["~"]);
    expect(withRoot(["/Users/x/Source"], "/Users/x/Source/", home)).toEqual(["/Users/x/Source"]);
    expect(withRoot(["~"], "/Users/xy", home)).toEqual(["~", "/Users/xy"]);
  });

  it("matches Windows spellings of a root below the home directory", () => {
    const home = "C:\\Users\\x";
    expect(withRoot(["~/Source"], "C:\\Users\\x\\Source", home)).toEqual(["~/Source"]);
    expect(withRoot(["~"], "c:\\users\\X\\", home)).toEqual(["~"]);
    expect(withRoot(["~"], "C:\\Users\\xy", home)).toEqual(["~", "C:\\Users\\xy"]);
    expect(withRoot(["~"], "D:\\src", home)).toEqual(["~", "D:\\src"]);
  });
});

const setDark = vi.fn();

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

afterEach(() => {
  TestBed.resetTestingModule();
  document.documentElement.removeAttribute("style");
});

function setup(
  colorSchema: string | { light: string; dark: string } | null = "dracula",
  explorerRoots: string[] = ["~"],
) {
  tauri.reset();
  setDark.mockReset();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({ providers: [{ provide: HxTheme, useValue: { setDark } }] });
  TestBed.overrideComponent(SettingsView, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/settings-view/settings-view.html", "utf8"),
    },
  });
  TestBed.overrideComponent(SpaceLayoutEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/settings-view/space-layout-editor.html", "utf8"),
    },
  });
  const store = TestBed.inject(SettingsStore);
  store["_settings"].set({ ...DEFAULT_SETTINGS, colorSchema, explorerRoots });
  const schemes = TestBed.inject(ColorSchemeService);
  schemes.sync(typeof colorSchema === "string" ? colorSchema : null);
  const fixture = TestBed.createComponent(SettingsView);
  fixture.detectChanges();
  const host = fixture.nativeElement as HTMLElement;
  const option = (name: string) =>
    host.querySelector<HTMLElement>(`[role="option"][data-name="${name}"]`)!;
  return { fixture, host, store, schemes, option };
}

const bg = () => document.documentElement.style.getPropertyValue("--term-bg");

describe("SettingsView", () => {
  beforeEach(() => vi.spyOn(console, "warn").mockImplementation(() => undefined));

  it("lists the schemes in a dark and a light listbox with swatches", () => {
    const { host, option } = setup();
    const lists = host.querySelectorAll('[role="listbox"]');
    expect(lists).toHaveLength(2);
    expect(lists[0].contains(option("dracula"))).toBe(true);
    expect(lists[1].contains(option("one-light"))).toBe(true);
    expect(lists[0].getAttribute("aria-labelledby")).toBe("list-dark");
    expect(option("dracula").querySelectorAll(".sw")).toHaveLength(2 + 16 + 1);
    expect(option("dracula").getAttribute("aria-selected")).toBe("true");
    expect(option("one-light").getAttribute("aria-selected")).toBe("false");
  });

  it("previews a scheme on focus and restores the saved one when focus leaves the list", () => {
    const { fixture, host, schemes, option } = setup("dracula");
    const revert = vi.spyOn(schemes, "revert");
    option("one-light").focus();
    expect(bg()).toBe("#fafafa");
    expect(setDark).toHaveBeenLastCalledWith(false);
    option("one-light").dispatchEvent(
      new FocusEvent("focusout", { bubbles: true, relatedTarget: null }),
    );
    fixture.detectChanges();
    expect(revert).toHaveBeenCalledOnce();
    expect(host.querySelector('[aria-selected="true"]')!.getAttribute("data-name")).toBe("dracula");
    expect(tauri.calls("set_color_schema")).toEqual([]);
  });

  it("keeps the preview while focus moves inside the list", () => {
    const { schemes, option } = setup();
    const revert = vi.spyOn(schemes, "revert");
    option("dracula").dispatchEvent(
      new FocusEvent("focusout", { bubbles: true, relatedTarget: option("catppuccin") }),
    );
    expect(revert).not.toHaveBeenCalled();
  });

  it("previews on hover and reverts when the mouse leaves", () => {
    const { schemes, option } = setup("dracula");
    const revert = vi.spyOn(schemes, "revert");
    option("one-light").dispatchEvent(new MouseEvent("mouseenter"));
    expect(bg()).toBe("#fafafa");
    option("one-light").parentElement!.dispatchEvent(new MouseEvent("mouseleave"));
    expect(revert).toHaveBeenCalledOnce();
  });

  it("saves on click", () => {
    const { option } = setup();
    option("vesper").click();
    expect(tauri.calls("set_color_schema")).toEqual([{ schema: "vesper" }]);
  });

  it("saves on Enter and Space but not on other keys", () => {
    const { option } = setup();
    option("vesper").dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    option("vesper").dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
    option("vesper").dispatchEvent(new KeyboardEvent("keydown", { key: "a", bubbles: true }));
    expect(tauri.calls("set_color_schema")).toEqual([{ schema: "vesper" }, { schema: "vesper" }]);
  });

  it("moves between options with the arrow keys, Home and End", () => {
    const { host, option } = setup();
    const options = [
      ...host.querySelector('[role="listbox"]')!.querySelectorAll<HTMLElement>('[role="option"]'),
    ];
    const first = options[0];
    first.focus();
    first.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    expect(document.activeElement).toBe(options[1]);
    options[1].dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    expect(document.activeElement).toBe(options.at(-1));
    options
      .at(-1)!
      .dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
    expect(document.activeElement).toBe(options.at(-1));
    options.at(-1)!.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true }));
    expect(document.activeElement).toBe(first);
    expect(option("dracula").getAttribute("tabindex")).toBe("0");
  });

  it("saves one side of a pair", () => {
    const { option } = setup({ light: "one-light", dark: "one-dark" });
    option("dracula").click();
    expect(tauri.calls("set_color_schema")).toEqual([
      { schema: { light: "one-light", dark: "dracula" } },
    ]);
  });

  it("switches to a pair that keeps the current scheme, and back", () => {
    const { host, store } = setup("dracula");
    const radio = (text: string) =>
      [...host.querySelectorAll<HTMLElement>('[role="radio"]')].find((b) =>
        b.textContent?.includes(text),
      )!;
    radio("pair").click();
    expect(tauri.calls("set_color_schema")).toEqual([
      { schema: { light: "poimen-light", dark: "dracula" } },
    ]);
    store["_settings"].set({
      ...DEFAULT_SETTINGS,
      theme: "dark",
      colorSchema: { light: "one-light", dark: "dracula" },
    });
    radio("Single").click();
    expect(tauri.calls("set_color_schema")[1]).toEqual({ schema: "dracula" });
  });

  it("saves the appearance", () => {
    const { host } = setup();
    [...host.querySelectorAll<HTMLElement>('[role="radio"]')]
      .find((b) => b.textContent?.includes("Light"))!
      .click();
    expect(tauri.calls("set_theme")).toEqual([{ theme: "light" }]);
  });
});

describe("SettingsView explorer section", () => {
  beforeEach(() => vi.spyOn(console, "warn").mockImplementation(() => undefined));

  const button = (host: HTMLElement, label: string) =>
    host.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;

  it("lists the roots and keeps the last one", () => {
    const { host } = setup(null, ["~"]);
    expect(host.querySelector(".path")!.textContent).toBe("~");
    expect(button(host, "Remove ~").disabled).toBe(true);
    expect(button(host, "Move ~ up").disabled).toBe(true);
  });

  it("removes and reorders roots", () => {
    const { host } = setup(null, ["~", "/srv"]);
    button(host, "Move /srv up").click();
    expect(tauri.calls("set_explorer_roots")).toEqual([{ roots: ["/srv", "~"] }]);
  });

  it("removes a root", () => {
    const { host } = setup(null, ["~", "/srv"]);
    button(host, "Remove ~").click();
    expect(tauri.calls("set_explorer_roots")).toEqual([{ roots: ["/srv"] }]);
  });

  it("adds a folder from the picker and ignores a cancel", async () => {
    const { host } = setup(null, ["~"]);
    const pick = vi.spyOn(TestBed.inject(TauriBridge), "pickDirectory");
    pick.mockResolvedValueOnce(null).mockResolvedValueOnce("/srv/projects");
    const add = [...host.querySelectorAll<HTMLButtonElement>("button")].find((b) =>
      b.textContent?.includes("Add folder"),
    )!;
    add.click();
    await Promise.resolve();
    expect(tauri.calls("set_explorer_roots")).toEqual([]);
    add.click();
    await vi.waitFor(() =>
      expect(tauri.calls("set_explorer_roots")).toEqual([{ roots: ["~", "/srv/projects"] }]),
    );
  });

  it("saves the hidden folders option and shows a write error", async () => {
    tauri.reset();
    const { fixture, host } = setup(null, ["~"]);
    tauri.answer("set_explorer_show_hidden", () => {
      throw new Error("read-only");
    });
    const box = host.querySelector<HTMLInputElement>('input[type="checkbox"]')!;
    box.checked = true;
    box.dispatchEvent(new Event("change"));
    await vi.waitFor(() =>
      expect(tauri.calls("set_explorer_show_hidden")).toEqual([{ show: true }]),
    );
    await Promise.resolve();
    fixture.detectChanges();
    expect(host.querySelector(".error")?.textContent).toContain("read-only");
  });

  it("saves the notification switches", async () => {
    tauri.reset();
    const { host } = setup(null, ["~"]);
    const section = host.querySelector('section[aria-labelledby="notifications-h"]')!;
    const [blocked, done] = section.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    expect(blocked.checked).toBe(true);
    expect(done.checked).toBe(true);
    done.checked = false;
    done.dispatchEvent(new Event("change"));
    blocked.checked = false;
    blocked.dispatchEvent(new Event("change"));
    await vi.waitFor(() =>
      expect(tauri.calls("set_notification")).toEqual([
        { kind: "done", on: false },
        { kind: "blocked", on: false },
      ]),
    );
  });
});

describe("SettingsView agents section", () => {
  beforeEach(() => vi.spyOn(console, "warn").mockImplementation(() => undefined));

  it("lists the harnesses with their availability", async () => {
    const { fixture, host, store } = setup();
    tauri.answer("get_harnesses", () => [
      { id: "claude", command: ["claude"], models: ["opus"], defaultModel: null, available: true },
      { id: "copilot", command: ["copilot"], models: [], defaultModel: null, available: false },
    ]);
    // A settings change (config.json edited) reads the harnesses again.
    store["_settings"].set({ ...DEFAULT_SETTINGS, warnings: ["x"] });
    fixture.detectChanges();
    await new Promise((resolve) => setTimeout(resolve));
    fixture.detectChanges();
    const rows = [...host.querySelectorAll(".harnesses li")].map((li) => li.textContent);
    expect(rows).toHaveLength(2);
    expect(rows[0]).toContain("claude");
    expect(rows[0]).toContain("available");
    expect(rows[0]).toContain("Models: opus");
    expect(rows[1]).toContain("not found");
  });
});

describe("parseHarnesses", () => {
  it("reads harnesses and skips malformed entries", () => {
    expect(
      parseHarnesses([
        { id: "a", command: ["a", "x"], models: ["m", 1], defaultModel: "m", available: true },
        { command: ["no-id"] },
        null,
        { id: "b" },
      ]),
    ).toEqual([
      { id: "a", command: ["a", "x"], models: ["m"], defaultModel: "m", available: true },
      { id: "b", command: [], models: [], defaultModel: null, available: false },
    ]);
    expect(parseHarnesses("nope")).toEqual([]);
  });
});
