import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Snapshot } from "./herdr";
import { HerdrStore } from "./herdr-store";
import { PluginPanes, type PluginPaneDef } from "./plugin-panes";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const snapshot: Snapshot = { workspaces: [], tabs: [], panes: [] };

/** Wire shapes of `plugin.list` panes, as the deliver-board and review-popup manifests declare. */
const board: PluginPaneDef = {
  plugin_id: "deliver-board",
  plugin_name: "deliver-board",
  entrypoint: "board",
  title: "Deliver board",
};
const review: PluginPaneDef = {
  plugin_id: "review-popup",
  plugin_name: "review-popup",
  entrypoint: "review",
  title: "Review",
};

describe("PluginPanes", () => {
  let panes: PluginPanes;

  beforeEach(() => {
    tauri.reset();
    tauri.answer("snapshot", () => snapshot);
    tauri.answer("plugin_pane_defs", () => [board, review]);
    tauri.answer("plugin_pane_open", (args) => ({
      plugin_id: args?.["pluginId"],
      entrypoint: args?.["entrypoint"],
      pane_id: args?.["entrypoint"] === "board" ? "w1:p5" : "w1:p6",
    }));
    const injector = Injector.create({ providers: [TauriBridge, HerdrStore, PluginPanes] });
    panes = injector.get(PluginPanes);
  });

  it("lists the declared panes and clears the list when herdr refuses", async () => {
    await panes.load();
    expect(panes.defs().map((d) => d.entrypoint)).toEqual(["board", "review"]);
    tauri.answer("plugin_pane_defs", () => {
      throw "refused";
    });
    await panes.load();
    expect(panes.defs()).toEqual([]);
  });

  it("opens deliver-board and review-popup and names their tabs after the plugin", async () => {
    expect(await panes.open(board, "w1")).toEqual({ paneId: "w1:p5" });
    expect(await panes.open(review)).toEqual({ paneId: "w1:p6" });
    expect(tauri.calls("plugin_pane_open")[0]).toMatchObject({
      pluginId: "deliver-board",
      entrypoint: "board",
      workspaceId: "w1",
    });
    expect(panes.names().get("w1:p5")).toBe("deliver-board");
    expect(panes.names().get("w1:p6")).toBe("review-popup");
  });

  it("reports a refused open with the pane title", async () => {
    tauri.answer("plugin_pane_open", () => {
      throw "plugin_pane_open_failed";
    });
    expect(await panes.open(review)).toEqual({ error: "Review: plugin_pane_open_failed" });
    expect(panes.names().size).toBe(0);
  });

  it("focuses only plugin panes", async () => {
    await panes.open(board);
    await panes.focus("w1:p5");
    await panes.focus("w1:p1");
    expect(tauri.calls("plugin_pane_focus")).toEqual([{ paneId: "w1:p5" }]);
  });

  it("tolerates a focus on a pane that is gone", async () => {
    await panes.open(board);
    tauri.answer("plugin_pane_focus", () => {
      throw "plugin_pane_not_found";
    });
    await expect(panes.focus("w1:p5")).resolves.toBeUndefined();
  });

  it("closes a plugin pane through herdr and forgets it", async () => {
    await panes.open(board);
    expect(await panes.close("w1:p5")).toBeNull();
    expect(tauri.calls("plugin_pane_close")).toEqual([{ paneId: "w1:p5" }]);
    expect(panes.isPluginPane("w1:p5")).toBe(false);
  });

  it("does not close a pane that is not a plugin pane and keeps a pane herdr refused to close", async () => {
    expect(await panes.close("w1:p1")).toBeNull();
    expect(tauri.calls("plugin_pane_close")).toEqual([]);
    await panes.open(review);
    tauri.answer("plugin_pane_close", () => {
      throw "plugin_pane_not_found";
    });
    expect(await panes.close("w1:p6")).toBe("plugin_pane_not_found");
    expect(panes.isPluginPane("w1:p6")).toBe(true);
  });
});
