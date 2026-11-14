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

  it("parses showBaseBranch as a strict boolean, default false", () => {
    expect(parseSettings({}).sidebarShowBaseBranch).toBe(false);
    expect(parseSettings({ sidebarShowBaseBranch: true }).sidebarShowBaseBranch).toBe(true);
    expect(parseSettings({ sidebarShowBaseBranch: "yes" }).sidebarShowBaseBranch).toBe(false);
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
