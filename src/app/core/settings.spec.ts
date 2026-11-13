import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_SETTINGS,
  SettingsStore,
  nextTheme,
  defaultTerminalFontFamily,
  defaultTerminalFontSize,
  parseSettings,
  parseTaskModes,
  windowsPtyOption,
} from "./settings";
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

describe("UI font settings", () => {
  beforeEach(() => {
    tauri.reset();
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
  });

  it("defaults to the default stack and 13px", () => {
    expect(DEFAULT_SETTINGS.uiFontFamily).toBeNull();
    expect(DEFAULT_SETTINGS.uiFontSize).toBe(13);
    const s = parseSettings({ uiFontFamily: 3, uiFontSize: "big" });
    expect([s.uiFontFamily, s.uiFontSize]).toEqual([null, 13]);
  });

  it("writes the four font settings through their commands", async () => {
    const store = create();
    expect(await store.setTerminalFontFamily(" Fira Code ")).toBe(true);
    expect(await store.setTerminalFontSize(16)).toBe(true);
    expect(await store.setUiFontFamily("Inter")).toBe(true);
    expect(await store.setUiFontSize(14)).toBe(true);
    expect(tauri.calls("set_terminal_font_family")).toEqual([{ family: "Fira Code" }]);
    expect(tauri.calls("set_terminal_font_size")).toEqual([{ size: 16 }]);
    expect(tauri.calls("set_ui_font_family")).toEqual([{ family: "Inter" }]);
    expect(tauri.calls("set_ui_font_size")).toEqual([{ size: 14 }]);
    const s = store.settings();
    expect([s.fontFamily, s.fontSize, s.uiFontFamily, s.uiFontSize]).toEqual([
      "Fira Code",
      16,
      "Inter",
      14,
    ]);
  });

  it("an empty family goes back to the default", async () => {
    const store = create();
    await store.setUiFontFamily("Inter");
    await store.setUiFontFamily("  ");
    expect(store.settings().uiFontFamily).toBeNull();
    expect(tauri.calls("set_ui_font_family").at(-1)).toEqual({ family: "" });
  });

  it("does not write a size out of range and says why", async () => {
    const store = create();
    expect(await store.setUiFontSize(9)).toBe(false);
    expect(await store.setUiFontSize(Number.NaN)).toBe(false);
    expect(await store.setTerminalFontSize(73)).toBe(false);
    expect(tauri.calls("set_ui_font_size")).toEqual([]);
    expect(tauri.calls("set_terminal_font_size")).toEqual([]);
    expect(store.writeError()).toBe("The terminal font size must be a number from 6 to 72");
    expect(store.settings().uiFontSize).toBe(13);
  });
});

describe("parseSettings", () => {
  it("reads the wire shape", () => {
    expect(
      parseSettings({
        theme: "dark",
        colorSchema: " dracula ",
        fontFamily: "Hack",
        fontSize: 15,
        uiFontFamily: "Inter",
        uiFontSize: 14,
        colors: {
          background: "#000000",
          foreground: null,
          cursor: "#ffffff",
          selectionBackground: "#222222",
          palette: ["#111111", null, 5],
        },
        autoControl: false,
        spacesLayout: { panes: [{ name: "solo", command: "nvim", split: null }] },
        spaceTemplates: [
          { name: "w", dir: "~/src", remote: null, agent: "claude", model: "opus", layout: null },
          {
            name: "az",
            remote: { provider: "azure-devops", organization: "o", project: "p" },
            agent: "copilot",
          },
          { name: 3 },
        ],
        home: "/Users/x",
        warnings: ["w"],
      }),
    ).toEqual({
      theme: "dark",
      colorSchema: "dracula",
      fontFamily: "Hack",
      fontSize: 15,
      uiFontFamily: "Inter",
      uiFontSize: 14,
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
      backend: "poimen",
      scrollback: 10000,
      herdrAutoStart: true,
      spacesLayout: { panes: [{ name: "solo", command: "nvim", split: null }] },
      spaceTemplates: [
        { name: "w", dir: "~/src", remote: null, agent: "claude", model: "opus", layout: null },
        {
          name: "az",
          dir: null,
          remote: { provider: "azure-devops", organization: "o", project: "p" },
          agent: "copilot",
          model: null,
          layout: null,
        },
      ],
      trustedProjects: {},
      tasks: DEFAULT_SETTINGS.tasks,
      machines: [],
      home: "/Users/x",
      warnings: ["w"],
    });
  });

  it("parses machines and skips entries without a name or an ssh target", () => {
    expect(
      parseSettings({
        machines: [
          { name: "box", ssh: "box", reposDir: "~/Source/{owner}/{repo}" },
          { name: "gpu", ssh: "me@gpu" },
          { name: "", ssh: "x" },
          { name: "nossh" },
          7,
        ],
      }).machines,
    ).toEqual([
      { name: "box", ssh: "box", reposDir: "~/Source/{owner}/{repo}" },
      { name: "gpu", ssh: "me@gpu", reposDir: "~/Source/{owner}/{repo}" },
    ]);
    expect(parseSettings({ machines: "box" }).machines).toEqual([]);
    expect(parseSettings({}).machines).toEqual([]);
  });

  it("reads the backend and falls back to poimen for anything else", () => {
    expect(parseSettings({ backend: "poimen" }).backend).toBe("poimen");
    expect(parseSettings({ backend: "herdr" }).backend).toBe("herdr");
    expect(parseSettings({ backend: "tmux" }).backend).toBe("poimen");
    expect(parseSettings({}).backend).toBe("poimen");
  });

  it("reads the scrollback and keeps 10000 for a value that is not a whole number from 0", () => {
    expect(parseSettings({ scrollback: 500 }).scrollback).toBe(500);
    expect(parseSettings({ scrollback: 0 }).scrollback).toBe(0);
    for (const bad of [-1, 1.5, "100", null]) {
      expect(parseSettings({ scrollback: bad }).scrollback).toBe(10000);
    }
  });

  it("parses trusted projects and skips entries that are not strings", () => {
    expect(parseSettings({ trustedProjects: { "/a": "h1", "/b": 3 } }).trustedProjects).toEqual({
      "/a": "h1",
    });
    expect(parseSettings({ trustedProjects: ["x"] }).trustedProjects).toEqual({});
    expect(parseSettings({}).trustedProjects).toEqual({});
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
      modes: [
        {
          name: "q",
          worktree: false,
          harness: null,
          model: null,
          prompt: null,
          command: "vim",
          branch: "b",
          cleanup: "c",
        },
      ],
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
  it("has no system notification setting any more (#676)", () => {
    const s = parseSettings({ notifyOnBlocked: true, notifyOnDone: true });
    expect(s).not.toHaveProperty("notifyOnBlocked");
    expect(s).not.toHaveProperty("notifyOnDone");
  });
});

describe("parseTaskModes", () => {
  it("keeps harness modes, argv commands and deprecated string commands", () => {
    const got = parseTaskModes({
      worktreeDir: "../{repo}-{nr}",
      modes: [
        { name: "agent", harness: "copilot", model: "auto", prompt: "Fix #{nr}" },
        {
          name: "loop",
          command: ["pwsh", "run.ps1", "{nr}"],
          cleanup: ["git", "branch", "-D", "{branch}"],
        },
        { name: "old", command: "claude {prompt}" },
      ],
    });
    expect(got.modes.map((m) => [m.harness, m.model, m.prompt, m.command])).toEqual([
      ["copilot", "auto", "Fix #{nr}", null],
      [null, null, null, ["pwsh", "run.ps1", "{nr}"]],
      [null, null, null, "claude {prompt}"],
    ]);
    expect(got.modes[1].cleanup).toEqual(["git", "branch", "-D", "{branch}"]);
  });
});

describe("windowsPtyOption", () => {
  it("marks a Windows PTY as ConPTY and leaves other platforms unset", () => {
    expect(windowsPtyOption(true)).toEqual({ backend: "conpty", buildNumber: 21376 });
    expect(windowsPtyOption(false)).toBeUndefined();
  });
});

describe("platform terminal font defaults", () => {
  it("uses 14 on Windows and 13 elsewhere", () => {
    expect(defaultTerminalFontSize(true)).toBe(14);
    expect(defaultTerminalFontSize(false)).toBe(13);
  });

  it("puts the Windows monospace fonts before the base stack only on Windows", () => {
    expect(defaultTerminalFontFamily("", true)).toBe(
      '"Cascadia Mono", "Cascadia Code", Consolas, Menlo, monospace',
    );
    expect(defaultTerminalFontFamily("JetBrains Mono, monospace", true)).toBe(
      '"Cascadia Mono", "Cascadia Code", Consolas, JetBrains Mono, monospace',
    );
    expect(defaultTerminalFontFamily("", false)).toBe("Menlo, monospace");
  });

  it("a configured size wins over the platform default", () => {
    expect(parseSettings({ fontSize: 16 }).fontSize).toBe(16);
    expect(parseSettings({ fontFamily: "Hack" }).fontFamily).toBe("Hack");
  });
});
