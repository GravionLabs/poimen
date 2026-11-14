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

describe("sidebar layout setting", () => {
  beforeEach(() => tauri.reset());

  it("parses split and falls back to tree", () => {
    expect(parseSettings({ sidebarLayout: "split" }).sidebarLayout).toBe("split");
    expect(parseSettings({ sidebarLayout: "grid" }).sidebarLayout).toBe("tree");
    expect(parseSettings({}).sidebarLayout).toBe("tree");
  });

  it("writes through set_sidebar_layout and applies it at once", async () => {
    const store = create();
    await store.setSidebarLayout("split");
    expect(tauri.calls("set_sidebar_layout")).toEqual([{ layout: "split" }]);
    expect(store.settings().sidebarLayout).toBe("split");
    expect(store.writeError()).toBeNull();
  });

  it("goes back and reports the error when the write fails", async () => {
    tauri.answer("set_sidebar_layout", () => {
      throw new Error("disk full");
    });
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    const store = create();
    await store.setSidebarLayout("split");
    expect(store.settings().sidebarLayout).toBe("tree");
    expect(store.writeError()).toContain("disk full");
  });
});

describe("sidebar sort setting", () => {
  beforeEach(() => tauri.reset());

  it("parses name and falls back to manual", () => {
    expect(parseSettings({ sidebarSort: "name" }).sidebarSort).toBe("name");
    expect(parseSettings({ sidebarSort: "recent" }).sidebarSort).toBe("manual");
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
