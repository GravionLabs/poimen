import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_SETTINGS, SettingsStore, parseSettings } from "./settings";
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
        warnings: ["w"],
      }),
    ).toEqual({
      theme: "dark",
      fontFamily: "Hack",
      fontSize: 15,
      colors: { background: "#000000", foreground: null },
      warnings: ["w"],
    });
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
