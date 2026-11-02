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
        fontFamily: "Hack",
        fontSize: 15,
        colors: { background: "#000000", foreground: null },
        autoControl: false,
        warnings: ["w"],
      }),
    ).toEqual({
      theme: "dark",
      fontFamily: "Hack",
      fontSize: 15,
      colors: { background: "#000000", foreground: null },
      autoControl: false,
      shortcuts: DEFAULT_SETTINGS.shortcuts,
      sidebarRows: DEFAULT_SETTINGS.sidebarRows,
      warnings: ["w"],
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
