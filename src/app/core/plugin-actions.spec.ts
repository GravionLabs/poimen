import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginAction, Snapshot } from "./herdr";
import { HerdrStore } from "./herdr-store";
import { PluginActions, parsePluginMenuId, pluginMenuId } from "./plugin-actions";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const action = (plugin_id: string, action_id: string, title: string): PluginAction => ({
  plugin_id,
  action_id,
  title,
  description: null,
});

const snapshot: Snapshot = {
  workspaces: [{ workspace_id: "w1", label: "One", number: 1, agent_status: "idle" }],
  tabs: [{ tab_id: "w1:t1", workspace_id: "w1", label: "main", number: 1, agent_status: "idle" }],
  panes: [
    { pane_id: "w1:p1", tab_id: "w1:t1", workspace_id: "w1", agent_status: "idle", cwd: "/a" },
    { pane_id: "w1:p2", tab_id: "w1:t1", workspace_id: "w1", agent_status: "idle", cwd: "/b" },
  ],
};

describe("plugin menu ids", () => {
  it("round-trips ids that contain separators", () => {
    const id = pluginMenuId(action("a/b:c", "d:e", "T"));
    expect(id.startsWith("plugin:")).toBe(true);
    expect(parsePluginMenuId(id)).toEqual({ pluginId: "a/b:c", actionId: "d:e" });
  });

  it.each(["split-right", "plugin:", "plugin:{", 'plugin:["a"]', "plugin:[1,2]"])(
    "rejects %s",
    (id) => {
      expect(parsePluginMenuId(id)).toBeNull();
    },
  );
});

describe("PluginActions", () => {
  let plugins: PluginActions;

  beforeEach(async () => {
    tauri.reset();
    tauri.answer("snapshot", () => snapshot);
    tauri.answer("plugin_actions", () => [action("link-trim", "open", "Open link")]);
    const injector = Injector.create({ providers: [TauriBridge, HerdrStore, PluginActions] });
    await injector.get(HerdrStore).refresh();
    plugins = injector.get(PluginActions);
  });

  it("lists the actions herdr offers and finds one by menu id", async () => {
    await plugins.load();
    expect(plugins.actions().map((a) => a.title)).toEqual(["Open link"]);
    expect(plugins.find(pluginMenuId(action("link-trim", "open", "")))?.title).toBe("Open link");
    expect(plugins.find(pluginMenuId(action("gone", "x", "")))).toBeUndefined();
    expect(plugins.find("split-right")).toBeUndefined();
  });

  it("picks up changed plugins on the next load", async () => {
    await plugins.load();
    tauri.answer("plugin_actions", () => [
      action("link-trim", "open", "Open link"),
      action("review-popup", "open", "Review"),
    ]);
    await plugins.load();
    expect(plugins.actions()).toHaveLength(2);
  });

  it("clears the list and reports why when herdr cannot list them", async () => {
    await plugins.load();
    tauri.answer("plugin_actions", () => {
      throw "herdr socket: refused";
    });
    await plugins.load();
    expect(plugins.actions()).toEqual([]);
    expect(plugins.error()).toBe("herdr socket: refused");
  });

  it("invokes with the pane the context menu was opened on", async () => {
    expect(await plugins.invoke(action("link-trim", "open", "Open link"), "w1:p2")).toBeNull();
    const [call] = tauri.calls("plugin_invoke") as { context: Record<string, unknown> }[];
    expect(call).toMatchObject({
      pluginId: "link-trim",
      actionId: "open",
      context: { focused_pane_id: "w1:p2", focused_pane_cwd: "/b", workspace_id: "w1" },
    });
  });

  it("returns the refusal text", async () => {
    tauri.answer("plugin_invoke", () => {
      throw "nope";
    });
    expect(await plugins.invoke(action("x", "y", "Do Y"), null)).toBe("Do Y: nope");
  });
});
