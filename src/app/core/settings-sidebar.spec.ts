import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsStore, parseSettings } from "./settings";
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

describe("sidebar settings", () => {
  beforeEach(() => tauri.reset());

  it("ignores an old sidebarLayout value (#1111)", () => {
    for (const sidebarLayout of ["tree", "split", "cards", "grid"]) {
      expect("sidebarLayout" in parseSettings({ sidebarLayout })).toBe(false);
    }
  });

  it("parses showBaseBranch as a strict boolean, default false", () => {
    expect(parseSettings({}).sidebarShowBaseBranch).toBe(false);
    expect(parseSettings({ sidebarShowBaseBranch: true }).sidebarShowBaseBranch).toBe(true);
    expect(parseSettings({ sidebarShowBaseBranch: "yes" }).sidebarShowBaseBranch).toBe(false);
  });
});

describe("sidebar sort setting", () => {
  beforeEach(() => tauri.reset());

  it("parses name and falls back to manual", () => {
    expect(parseSettings({ sidebarSort: "name" }).sidebarSort).toBe("name");
    expect(parseSettings({ sidebarSort: "attention" }).sidebarSort).toBe("attention");
    expect(parseSettings({ sidebarSort: "recent" }).sidebarSort).toBe("recent");
    expect(parseSettings({ sidebarSort: "newest" }).sidebarSort).toBe("manual");
    expect(parseSettings({}).sidebarSort).toBe("manual");
  });

  it("writes through set_sidebar_sort and goes back when the write fails", async () => {
    const store = create();
    await store.setSidebarSort("name");
    expect(tauri.calls("set_sidebar_sort")).toEqual([{ sort: "name" }]);
    expect(store.settings().sidebarSort).toBe("name");
    tauri.answer("set_sidebar_sort", () => {
      throw new Error("disk full");
    });
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    await store.setSidebarSort("manual");
    expect(store.settings().sidebarSort).toBe("name");
    expect(store.writeError()).toContain("disk full");
  });
});
