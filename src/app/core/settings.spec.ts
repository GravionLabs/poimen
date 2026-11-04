import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, SettingsStore, nextTheme, parseSettings } from "./settings";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

const create = () =>
  Injector.create({ providers: [TauriBridge, SettingsStore] }).get(SettingsStore);

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

describe("parseSettings", () => {
  it("reads the wire shape", () => {
    expect(
      parseSettings({
        theme: "dark",
        colorSchema: " dracula ",
        fontFamily: "Hack",
        fontSize: 15,
        colors: {
          background: "#000000",
          foreground: null,
          cursor: "#ffffff",
          selectionBackground: "#222222",
          palette: ["#111111", null, 5],
        },
        autoControl: false,
        notifyOnBlocked: false,
        spacesLayout: { panes: [{ name: "solo", command: "nvim", split: null }] },
        home: "/Users/x",
        warnings: ["w"],
      }),
    ).toEqual({
      theme: "dark",
      colorSchema: "dracula",
      fontFamily: "Hack",
      fontSize: 15,
      colors: {
        background: "#000000",
        foreground: null,
        cursor: "#ffffff",
        selectionBackground: "#222222",
        selectionForeground: null,
        palette: ["#111111", null, null],
      },
      autoControl: false,
      shortcuts: DEFAULT_SETTINGS.shortcuts,
      sidebarRows: DEFAULT_SETTINGS.sidebarRows,
      sidebarLayout: "tree",
      explorerRoots: ["~"],
      explorerShowHidden: false,
      herdrAutoStart: true,
      notifyOnBlocked: false,
      notifyOnDone: true,
      spacesLayout: { panes: [{ name: "solo", command: "nvim", split: null }] },
      tasks: DEFAULT_SETTINGS.tasks,
      home: "/Users/x",
      warnings: ["w"],
    });
  });

  it("parses tasks and falls back to the default agent mode", () => {
    expect(parseSettings({}).tasks.modes[0].name).toBe("agent");
    const t = parseSettings({
      tasks: {
        worktreeDir: "../w",
        modes: [{ name: "q", worktree: false, command: "vim", branch: "b", cleanup: "c" }, 3],
      },
    }).tasks;
    expect(t).toEqual({
      worktreeDir: "../w",
      modes: [{ name: "q", worktree: false, command: "vim", branch: "b", cleanup: "c" }],
    });
    expect(parseSettings({ tasks: { modes: [], worktreeDir: "x" } }).tasks).toEqual(
      DEFAULT_SETTINGS.tasks,
    );
  });

  it("keeps colorSchema only as a non-empty string or a complete pair", () => {
    expect(parseSettings({}).colorSchema).toBeNull();
    for (const bad of [3, "", "  ", { light: "a" }, { light: "a", dark: 1 }, null])
      expect(parseSettings({ colorSchema: bad }).colorSchema).toBeNull();
    expect(parseSettings({ colorSchema: { light: " a ", dark: "b" } }).colorSchema).toEqual({
      light: "a",
      dark: "b",
    });
  });

  it("merges the shortcuts over the defaults and ignores bad entries", () => {
    const parsed = parseSettings({
      shortcuts: { toggleSidebar: "Ctrl+Alt+B", closeTab: 5, x: "y" },
    });
    expect(parsed.shortcuts.toggleSidebar).toBe("Ctrl+Alt+B");
    expect(parsed.shortcuts.closeTab).toBe("CmdOrCtrl+W");
    expect(parseSettings({ shortcuts: "no" }).shortcuts).toEqual(DEFAULT_SETTINGS.shortcuts);
  });

  it("takes valid sidebarRows per row type and defaults the rest", () => {
    const parsed = parseSettings({
      sidebarRows: {
        workspace: [[{ token: "name", bold: true }, "$step"]],
        agent: [["nope"]],
        terminal: [],
      },
    });
    expect(parsed.sidebarRows.workspace).toEqual([
      [{ token: "name", bold: true, dim: false }, { token: "$step" }],
    ]);
    expect(parsed.sidebarRows.agent).toEqual(DEFAULT_SETTINGS.sidebarRows.agent);
    expect(parsed.sidebarRows.terminal).toEqual(DEFAULT_SETTINGS.sidebarRows.terminal);
  });

  it("falls back to the defaults on malformed input", () => {
    expect(parseSettings(null)).toEqual(DEFAULT_SETTINGS);
    expect(parseSettings({ theme: "blue", fontSize: "x", fontFamily: 1 })).toEqual(
      DEFAULT_SETTINGS,
    );
  });
});

describe("SettingsStore", () => {
  beforeEach(() => {
    tauri.reset();
  });

  it("loads the settings and applies later changes", async () => {
    tauri.answer("get_settings", () => ({ theme: "light", fontSize: 14 }));
    const store = create();
    await store.start();
    expect(store.settings().theme).toBe("light");
    expect(store.settings().fontSize).toBe(14);

    tauri.emit("settings", { theme: "dark", fontSize: 16 });
    expect(store.settings().theme).toBe("dark");
    expect(store.settings().fontSize).toBe(16);
  });

  it("keeps the defaults when the read fails", async () => {
    tauri.answer("get_settings", () => {
      throw new Error("boom");
    });
    const store = create();
    await store.start();
    expect(store.settings()).toEqual(DEFAULT_SETTINGS);
  });
});

describe("theme cycle and persistence", () => {
  beforeEach(() => tauri.reset());

  it("cycles system, light, dark, system", () => {
    expect(nextTheme("system")).toBe("light");
    expect(nextTheme("light")).toBe("dark");
    expect(nextTheme("dark")).toBe("system");
  });

  it("writes the theme through set_theme and applies it at once", async () => {
    const store = create();
    await store.setTheme("dark");
    expect(tauri.calls("set_theme")).toEqual([{ theme: "dark" }]);
    expect(store.settings().theme).toBe("dark");
  });

  it("goes back when the write fails", async () => {
    tauri.answer("set_theme", () => {
      throw new Error("disk full");
    });
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const store = create();
    await store.setTheme("light");
    expect(store.settings().theme).toBe("system");
  });

  it("follows an external edit of the file", async () => {
    const store = create();
    await store.start();
    tauri.emit("settings", { ...DEFAULT_SETTINGS, theme: "light" });
    expect(store.settings().theme).toBe("light");
  });
});

describe("herdrAutoStart", () => {
  it("is on unless the backend says false", () => {
    expect(parseSettings({}).herdrAutoStart).toBe(true);
    expect(parseSettings({ herdrAutoStart: false }).herdrAutoStart).toBe(false);
    expect(parseSettings({ herdrAutoStart: "no" }).herdrAutoStart).toBe(true);
  });
});

describe("notifications", () => {
  it("default on and parse per kind", () => {
    expect(parseSettings({}).notifyOnBlocked).toBe(true);
    expect(parseSettings({}).notifyOnDone).toBe(true);
    const s = parseSettings({ notifyOnBlocked: false, notifyOnDone: "no" });
    expect(s.notifyOnBlocked).toBe(false);
    expect(s.notifyOnDone).toBe(true);
  });
});
