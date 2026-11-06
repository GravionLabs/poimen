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
import { HooksOffer } from "../core/hooks-offer";
import { TauriBridge } from "../core/tauri";
import { UiState, UI_STORAGE, UI_STATE_KEY, type UiStorage } from "../core/ui-state";
import {
  chosen,
  groupSchemes,
  moved,
  pairOf,
  selectedIn,
  SettingsView,
  withRoot,
} from "./settings-view";
import { allPages, SETTINGS_GROUPS } from "./settings-categories";
import { SpaceLayoutEditor } from "./space-layout-editor";
import { SpaceTemplatesEditor } from "./space-templates-editor";

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

let storage: UiStorage | null = null;

function setup(
  colorSchema: string | { light: string; dark: string } | null = "dracula",
  explorerRoots: string[] = ["~"],
  page = "appearance",
) {
  tauri.reset();
  setDark.mockReset();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: HxTheme, useValue: { setDark } },
      { provide: UI_STORAGE, useValue: storage },
    ],
  });
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
  TestBed.overrideComponent(SpaceTemplatesEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/settings-view/space-templates-editor.html", "utf8"),
    },
  });
  TestBed.inject(UiState).selectSettingsPage(page);
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
    expect(
      host.querySelector('[role="option"][aria-selected="true"]')!.getAttribute("data-name"),
    ).toBe("dracula");
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
    const { host } = setup(undefined, undefined, "appearance");
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
    const { host } = setup(null, ["~"], "explorer");
    expect(host.querySelector(".path")!.textContent).toBe("~");
    expect(button(host, "Remove ~").disabled).toBe(true);
    expect(button(host, "Move ~ up").disabled).toBe(true);
  });

  it("removes and reorders roots", () => {
    const { host } = setup(null, ["~", "/srv"], "explorer");
    button(host, "Move /srv up").click();
    expect(tauri.calls("set_explorer_roots")).toEqual([{ roots: ["/srv", "~"] }]);
  });

  it("removes a root", () => {
    const { host } = setup(null, ["~", "/srv"], "explorer");
    button(host, "Remove ~").click();
    expect(tauri.calls("set_explorer_roots")).toEqual([{ roots: ["/srv"] }]);
  });

  it("adds a folder from the picker and ignores a cancel", async () => {
    const { host } = setup(null, ["~"], "explorer");
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
    const { fixture, host } = setup(null, ["~"], "explorer");
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

  it("stacks the notification checkboxes instead of sharing a row (#360)", () => {
    const { host } = setup(null, ["~"], "notifications");
    const style = document.createElement("style");
    style.textContent = readFileSync("src/app/settings-view/settings-view.css", "utf8");
    document.head.append(style);
    try {
      const labels = host.querySelectorAll<HTMLElement>(
        'section[aria-label="Notifications"] .check',
      );
      expect(labels.length).toBe(2);
      expect(getComputedStyle(labels[0]).display).toBe("flex");
      expect(getComputedStyle(labels[1]).marginTop).toBe("8px");
    } finally {
      style.remove();
    }
  });

  it("saves the notification switches", async () => {
    tauri.reset();
    const { host } = setup(null, ["~"], "notifications");
    const section = host.querySelector('section[aria-label="Notifications"]')!;
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

describe("SettingsView fonts", () => {
  beforeEach(() => vi.spyOn(console, "warn").mockImplementation(() => undefined));

  const field = (host: HTMLElement, label: string) =>
    [...host.querySelectorAll<HTMLLabelElement>("section[aria-label='Fonts'] label.field")]
      .find((l) => l.textContent?.trim().startsWith(label))!
      .querySelector("input")!;

  it("writes the four values", async () => {
    const { host } = setup();
    const entries: [string, string, string, Record<string, unknown>][] = [
      ["Terminal font family", "Fira Code", "set_terminal_font_family", { family: "Fira Code" }],
      ["Terminal font size", "15", "set_terminal_font_size", { size: 15 }],
      ["UI font family", "Inter", "set_ui_font_family", { family: "Inter" }],
      ["UI font size", "14", "set_ui_font_size", { size: 14 }],
    ];
    for (const [label, value, command, args] of entries) {
      const input = field(host, label);
      input.value = value;
      input.dispatchEvent(new Event("change"));
      await vi.waitFor(() => expect(tauri.calls(command)).toEqual([args]));
    }
  });

  it("removes the key when the terminal font size is cleared", async () => {
    const { host } = setup();
    const input = field(host, "Terminal font size");
    input.value = "";
    input.dispatchEvent(new Event("change"));
    await vi.waitFor(() => expect(tauri.calls("clear_terminal_font_size")).toEqual([{}]));
    expect(tauri.calls("set_terminal_font_size")).toEqual([]);
  });

  it("shows the validation message and writes nothing for a bad size", async () => {
    const { fixture, host } = setup();
    const input = field(host, "UI font size");
    input.value = "40";
    input.dispatchEvent(new Event("change"));
    await vi.waitFor(() => {
      fixture.detectChanges();
      expect(host.querySelector(".error")?.textContent).toContain("from 10 to 20");
    });
    expect(tauri.calls("set_ui_font_size")).toEqual([]);
    expect(input.value).toBe("13");
  });

  it("suggests common families and is found by the search labels", () => {
    const { host } = setup();
    expect(host.querySelectorAll("datalist#mono-fonts option").length).toBeGreaterThan(3);
    const labels = allPages().find((p) => p.id === "appearance")!.labels;
    expect(labels).toEqual(
      expect.arrayContaining([
        "Terminal font family",
        "Terminal font size",
        "UI font family",
        "UI font size",
      ]),
    );
  });
});

describe("SettingsView agents section", () => {
  beforeEach(() => vi.spyOn(console, "warn").mockImplementation(() => undefined));

  it("lists the harnesses with their availability", async () => {
    const { fixture, host, store } = setup(undefined, undefined, "agents");
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

describe("SettingsView trusted projects section", () => {
  beforeEach(() => vi.spyOn(console, "warn").mockImplementation(() => undefined));

  const revokeButtons = (host: HTMLElement) => [
    ...host.querySelectorAll<HTMLButtonElement>('ul[aria-label="Trusted projects"] button'),
  ];

  it("says so when no project is trusted", () => {
    const { host } = setup(undefined, undefined, "trusted-projects");
    expect(revokeButtons(host)).toHaveLength(0);
    expect(host.textContent).toContain("No project is trusted.");
  });

  it("lists the roots and revokes one", async () => {
    const { fixture, host, store } = setup(undefined, undefined, "trusted-projects");
    tauri.answer("revoke_project", () => undefined);
    store["_settings"].set({
      ...DEFAULT_SETTINGS,
      trustedProjects: { "/work/b": "h2", "/work/a": "h1" },
    });
    fixture.detectChanges();
    const rows = [...host.querySelectorAll('ul[aria-label="Trusted projects"] .path')];
    expect(rows.map((r) => r.textContent)).toEqual(["/work/a", "/work/b"]);
    revokeButtons(host)[0].click();
    await new Promise((resolve) => setTimeout(resolve));
    fixture.detectChanges();
    expect(tauri.calls("revoke_project")).toEqual([{ root: "/work/a" }]);
    expect(store.settings().trustedProjects).toEqual({ "/work/b": "h2" });
    expect(revokeButtons(host)).toHaveLength(1);
  });

  it("keeps the root and reports the error when the revoke fails", async () => {
    const { fixture, store } = setup(undefined, undefined, "trusted-projects");
    tauri.answer("revoke_project", () => {
      throw "disk full";
    });
    store["_settings"].set({ ...DEFAULT_SETTINGS, trustedProjects: { "/work/a": "h1" } });
    await store.revokeProject("/work/a");
    fixture.detectChanges();
    expect(store.settings().trustedProjects).toEqual({ "/work/a": "h1" });
    expect(store.writeError()).toContain("disk full");
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

function memoryStorage(
  initial: Record<string, string> = {},
): UiStorage & { data: Map<string, string> } {
  const data = new Map(Object.entries(initial));
  return {
    data,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => void data.set(k, v),
  };
}

describe("SettingsView categories", () => {
  beforeEach(() => {
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    storage = null;
  });
  afterEach(() => {
    storage = null;
  });

  const item = (host: HTMLElement, node: string) =>
    host.querySelector<HTMLElement>(`[role="treeitem"][data-node="${node}"]`)!;
  const titles = (host: HTMLElement) =>
    [...host.querySelectorAll('[role="treeitem"]')].map((e) =>
      e.textContent!.replace(/[▾▸]/, "").trim(),
    );
  const press = (el: HTMLElement, key: string, init: KeyboardEventInit = {}) =>
    el.dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init }),
    );
  const search = (host: HTMLElement) =>
    host.querySelector<HTMLInputElement>('input[type="search"]')!;
  const type = (fixture: { detectChanges(): void }, host: HTMLElement, text: string) => {
    search(host).value = text;
    search(host).dispatchEvent(new Event("input"));
    fixture.detectChanges();
  };
  const crumb = (host: HTMLElement) =>
    host.querySelector("h1")!.textContent!.replace(/\s+/g, " ").trim();

  it("shows the groups and pages as a tree with a breadcrumb", () => {
    const { host } = setup(undefined, undefined, "explorer");
    expect(host.querySelector('[role="tree"]')).not.toBeNull();
    expect(titles(host)).toEqual([
      "Appearance & Behavior",
      "Appearance",
      "Notifications",
      "Workspace",
      "Explorer",
      "Spaces",
      "New space layout",
      "Trusted projects",
      "Backend",
      "Server",
      "Agents",
    ]);
    expect(crumb(host)).toBe("Workspace › Explorer");
    expect(item(host, "p:explorer").getAttribute("aria-selected")).toBe("true");
  });

  it("falls back to the first page when the stored one no longer exists", () => {
    const { host } = setup(undefined, undefined, "gone");
    expect(crumb(host)).toBe("Appearance & Behavior › Appearance");
  });

  it("selects a page on click and persists it with the collapsed groups", () => {
    const mem = memoryStorage();
    storage = mem;
    const { fixture, host } = setup(undefined, undefined, "appearance");
    item(host, "p:herdr").click();
    item(host, "g:workspace").click();
    fixture.detectChanges();
    expect(crumb(host)).toBe("Backend › Server");
    expect(host.textContent).toContain("Start the backend server");
    expect(item(host, "p:explorer")).toBeNull();
    const saved = JSON.parse(mem.data.get(UI_STATE_KEY)!);
    expect(saved.settingsPage).toBe("herdr");
    expect(saved.settingsCollapsed).toEqual(["workspace"]);
  });

  it("shows the hooks state per agent on the Server page with an install button", async () => {
    const { fixture, host } = setup(undefined, undefined, "herdr");
    tauri.answer("hooks_status", () => [
      { agent: "claude", state: "missing" },
      { agent: "copilot", state: "installed" },
    ]);
    await TestBed.inject(HooksOffer).refresh();
    fixture.detectChanges();
    const rows = [...host.querySelectorAll("ul.harnesses li")].map((li) =>
      li.textContent!.replace(/\s+/g, " ").trim(),
    );
    expect(rows).toEqual(["Claude Codemissing Install hooks", "GitHub Copilot CLIinstalled"]);
    const buttons = [...host.querySelectorAll("button")].filter(
      (b) => b.textContent!.trim() === "Install hooks",
    );
    expect(buttons).toHaveLength(1);
    buttons[0].click();
    await fixture.whenStable();
    expect(tauri.calls("hooks_install")).toEqual([{ agent: "claude" }]);
  });

  it("restores the collapsed groups from the stored state", () => {
    storage = memoryStorage({
      [UI_STATE_KEY]: JSON.stringify({ settingsPage: "agents", settingsCollapsed: ["backend"] }),
    });
    const { host } = setup(undefined, undefined, "agents");
    expect(item(host, "g:backend").getAttribute("aria-expanded")).toBe("false");
    expect(item(host, "p:agents")).toBeNull();
    expect(crumb(host)).toBe("Backend › Agents");
  });

  it("filters the tree by page title and by setting label, keeping the group", () => {
    const { fixture, host } = setup();
    type(fixture, host, "notif");
    expect(titles(host)).toEqual(["Appearance & Behavior", "Notifications"]);
    type(fixture, host, "hidden folders");
    expect(titles(host)).toEqual(["Workspace", "Explorer"]);
    type(fixture, host, "zzz");
    expect(titles(host)).toEqual([]);
    expect(host.textContent).toContain("No settings match.");
    type(fixture, host, "");
    expect(titles(host)).toHaveLength(11);
  });

  it("opens a retired color-scheme page as Appearance, with the scheme picker", () => {
    const { host } = setup(undefined, undefined, "color-scheme");
    expect(crumb(host)).toBe("Appearance & Behavior › Appearance");
    expect(item(host, "p:appearance").getAttribute("aria-selected")).toBe("true");
    expect(
      host.querySelector('section[aria-label="Color scheme"] [role="listbox"]'),
    ).not.toBeNull();
  });

  it("follows a deep link to a retired color-scheme page made while the view is open", () => {
    const { fixture, host } = setup(undefined, undefined, "notifications");
    TestBed.inject(UiState).selectSettingsPage("color-scheme");
    fixture.detectChanges();
    expect(crumb(host)).toBe("Appearance & Behavior › Appearance");
    expect(host.querySelector('[role="listbox"]')).not.toBeNull();
  });

  it("expands collapsed groups while searching", () => {
    storage = memoryStorage({
      [UI_STATE_KEY]: JSON.stringify({ settingsCollapsed: ["workspace"] }),
    });
    const { fixture, host } = setup();
    expect(item(host, "p:explorer")).toBeNull();
    type(fixture, host, "explorer");
    expect(item(host, "p:explorer")).not.toBeNull();
  });

  it("highlights the setting whose label matches", () => {
    const { fixture, host } = setup(undefined, undefined, "explorer");
    expect(host.querySelector(".hit")).toBeNull();
    type(fixture, host, "hidden");
    const hits = [...host.querySelectorAll(".hit")].map((e) => e.textContent!.trim());
    expect(hits).toEqual(["Show hidden folders"]);
  });

  it("has every label of the category list in its page template", () => {
    const html = readFileSync("src/app/settings-view/settings-view.html", "utf8");
    const editor = readFileSync("src/app/settings-view/space-layout-editor.html", "utf8");
    const templates = readFileSync("src/app/settings-view/space-templates-editor.html", "utf8");
    for (const page of allPages()) {
      const source = page.id === "space-layout" ? editor : page.id === "spaces" ? templates : html;
      for (const label of page.labels) {
        expect(source.toLowerCase(), `${page.id}: ${label}`).toContain(label.toLowerCase());
      }
    }
    expect(SETTINGS_GROUPS.length).toBeGreaterThan(0);
  });

  it("moves with the arrow keys, expands and collapses with Left and Right", () => {
    const { fixture, host } = setup(undefined, undefined, "appearance");
    document.body.append(host);
    const node = (n: string) => item(host, n);
    press(node("p:appearance"), "ArrowDown");
    expect(document.activeElement).toBe(node("p:notifications"));
    press(node("p:notifications"), "ArrowUp");
    press(node("p:appearance"), "ArrowLeft");
    expect(document.activeElement).toBe(node("g:appearance-behavior"));
    press(node("g:appearance-behavior"), "ArrowLeft");
    fixture.detectChanges();
    expect(node("g:appearance-behavior").getAttribute("aria-expanded")).toBe("false");
    expect(node("p:appearance")).toBeNull();
    press(node("g:appearance-behavior"), "ArrowDown");
    expect(document.activeElement).toBe(node("g:workspace"));
    press(node("g:appearance-behavior"), "ArrowRight");
    fixture.detectChanges();
    expect(node("g:appearance-behavior").getAttribute("aria-expanded")).toBe("true");
    press(node("g:appearance-behavior"), "ArrowRight");
    expect(document.activeElement).toBe(node("p:appearance"));
    press(node("p:appearance"), "End");
    expect(document.activeElement).toBe(node("p:agents"));
    press(node("p:agents"), "Home");
    expect(document.activeElement).toBe(node("g:appearance-behavior"));
    host.remove();
  });

  it("opens a page with Enter or Space and keeps one tab stop", () => {
    const { fixture, host } = setup(undefined, undefined, "appearance");
    press(item(host, "p:herdr"), "Enter");
    fixture.detectChanges();
    expect(crumb(host)).toBe("Backend › Server");
    press(item(host, "p:agents"), " ");
    fixture.detectChanges();
    expect(crumb(host)).toBe("Backend › Agents");
    const stops = [...host.querySelectorAll('[role="treeitem"][tabindex="0"]')];
    expect(stops).toHaveLength(1);
    expect(stops[0].getAttribute("data-node")).toBe("p:agents");
  });

  it("focuses the search field with Cmd/Ctrl+F and clears it with Escape", () => {
    const { fixture, host } = setup();
    document.body.append(host);
    press(item(host, "p:appearance"), "f", { ctrlKey: true });
    expect(document.activeElement).toBe(search(host));
    type(fixture, host, "x");
    press(search(host), "Escape");
    fixture.detectChanges();
    expect(search(host).value).toBe("");
    host.remove();
  });

  it("follows a deep link made while the view is open", () => {
    const { fixture, host } = setup();
    TestBed.inject(UiState).selectSettingsPage("notifications");
    fixture.detectChanges();
    expect(crumb(host)).toBe("Appearance & Behavior › Notifications");
  });
});
