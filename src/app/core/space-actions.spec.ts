import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HerdrStore } from "./herdr-store";
import { SpaceActions, dirName } from "./space-actions";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));
const pick = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: pick }));

describe("dirName", () => {
  it("is the last path segment", () => {
    expect(dirName("/work/app")).toBe("app");
    expect(dirName("C:\\work\\app\\")).toBe("app");
    expect(dirName("/work/app/ ")).toBe("app");
    expect(dirName("")).toBe("");
  });
});

describe("SpaceActions", () => {
  let actions: SpaceActions;

  beforeEach(() => {
    tauri.reset();
    pick.mockReset();
    tauri.answer("snapshot", () => ({ workspaces: [], tabs: [], panes: [] }));
    const injector = Injector.create({ providers: [TauriBridge, HerdrStore, SpaceActions] });
    actions = injector.get(SpaceActions);
  });

  it("opens and cancels the dialog", () => {
    actions.ask();
    expect(actions.open()).toBe(true);
    actions.cancel();
    expect(actions.open()).toBe(false);
  });

  it("creates the space with the trimmed directory and label, then closes and re-reads", async () => {
    tauri.answer("create_space", () => ({ workspace_id: "w2", created: true }));
    actions.ask();
    const outcome = await actions.create(" /work/app ", " App ");
    expect(outcome).toEqual({ workspace_id: "w2", created: true });
    expect(tauri.calls("create_space")).toEqual([{ cwd: "/work/app", label: "App" }]);
    expect(tauri.calls("snapshot")).toHaveLength(1);
    expect(actions.open()).toBe(false);
  });

  it("keeps a layout warning of the created space for the status bar", async () => {
    tauri.answer("create_space", () => ({
      workspace_id: "w2",
      created: true,
      warnings: ["/work/app/.agents/herdr-layout.json: invalid JSON"],
    }));
    vi.spyOn(console, "warn").mockImplementation(() => undefined);
    await actions.create("/work/app", "");
    expect(actions.warning()).toBe("/work/app/.agents/herdr-layout.json: invalid JSON");
    tauri.answer("create_space", () => ({ workspace_id: "w3", created: true }));
    await actions.create("/work/lib", "");
    expect(actions.warning()).toBeNull();
  });

  it("sends no label when it is blank", async () => {
    await actions.create("/work/app", "  ");
    expect(tauri.calls("create_space")).toEqual([{ cwd: "/work/app", label: null }]);
  });

  it("keeps the dialog open and shows the error when herdr or the directory fails", async () => {
    tauri.answer("create_space", () => {
      throw "/nope is not a directory.";
    });
    actions.ask();
    expect(await actions.create("/nope", "")).toBeUndefined();
    expect(actions.error()).toBe("/nope is not a directory.");
    expect(actions.open()).toBe(true);
    expect(actions.busy()).toBe(false);
  });

  it("lists matching directories for the typed path, empty on failure", async () => {
    tauri.answer("list_dirs", () => ["/work/app", "/work/apps"]);
    await actions.complete("/work/ap");
    expect(tauri.calls("list_dirs")).toEqual([{ input: "/work/ap" }]);
    expect(actions.dirs()).toEqual(["/work/app", "/work/apps"]);
    tauri.answer("list_dirs", () => {
      throw "boom";
    });
    await actions.complete("/x");
    expect(actions.dirs()).toEqual([]);
  });

  it("picks a directory with the native dialog; a cancel yields null", async () => {
    pick.mockResolvedValueOnce("/picked");
    expect(await actions.pick()).toBe("/picked");
    expect(pick).toHaveBeenCalledWith({ directory: true, multiple: false });
    pick.mockResolvedValueOnce(null);
    expect(await actions.pick()).toBeNull();
  });
});
