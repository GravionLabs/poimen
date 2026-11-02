import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsStore } from "./settings";
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

describe("SettingsStore.setColorSchema", () => {
  beforeEach(() => tauri.reset());

  it("writes a name or a pair through set_color_schema and applies it at once", async () => {
    const store = create();
    await store.setColorSchema("dracula");
    expect(store.settings().colorSchema).toBe("dracula");
    const pair = { light: "one-light", dark: "one-dark" };
    await store.setColorSchema(pair);
    expect(tauri.calls("set_color_schema")).toEqual([{ schema: "dracula" }, { schema: pair }]);
    expect(store.settings().colorSchema).toEqual(pair);
  });

  it("goes back when the write fails", async () => {
    tauri.answer("set_color_schema", () => {
      throw new Error("disk full");
    });
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const store = create();
    await store.setColorSchema("dracula");
    expect(store.settings().colorSchema).toBeNull();
  });
});
