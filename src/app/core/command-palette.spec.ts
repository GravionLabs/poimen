import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CommandPalette } from "./command-palette";
import type { PluginAction, Snapshot } from "./herdr";
import { HerdrStore } from "./herdr-store";
import { Keymap } from "./keymap";
import { PluginActions } from "./plugin-actions";
import { PluginPanes } from "./plugin-panes";
import { UI_STORAGE, UiState } from "./ui-state";
import { SettingsStore } from "./settings";
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
  ],
};

describe("CommandPalette", () => {
  let palette: CommandPalette;

  beforeEach(async () => {
    tauri.reset();
    tauri.answer("snapshot", () => snapshot);
    tauri.answer("plugin_actions", () => [
      action("link-trim", "open", "Open link without trailing punctuation"),
      action("review-popup", "open", "Open the review popup"),
    ]);
    tauri.answer("plugin_pane_defs", () => [
      {
        plugin_id: "deliver-board",
        plugin_name: "deliver-board",
        entrypoint: "board",
        title: "Deliver board",
      },
    ]);
    const injector = Injector.create({
      providers: [
        { provide: UI_STORAGE, useValue: null },
        TauriBridge,
        HerdrStore,
        SettingsStore,
        Keymap,
        PluginActions,
        UiState,
        PluginPanes,
        CommandPalette,
      ],
    });
    await injector.get(HerdrStore).refresh();
    palette = injector.get(CommandPalette);
  });

  it("lists poimen commands first, then the plugin actions", async () => {
    await palette.open();
    const items = palette.items();
    expect(items[0]).toMatchObject({ kind: "command", command: "newTerminal" });
    expect(items.filter((i) => i.kind === "plugin").map((i) => i.title)).toEqual([
      "Open link without trailing punctuation",
      "Open the review popup",
    ]);
  });

  it("lists the panes plugins declare as tabs to open", async () => {
    await palette.open();
    const rows = palette.items().filter((i) => i.kind === "pane");
    expect(rows.map((i) => i.title)).toEqual(["Open Deliver board as tab"]);
    expect(rows[0]).toMatchObject({ origin: "deliver-board", def: { entrypoint: "board" } });
    palette.setQuery("deliver board");
    expect(palette.items()[0].kind).toBe("pane");
  });

  it("offers Close Space for the active space", async () => {
    await palette.open();
    palette.setQuery("close space");
    expect(palette.items()[0]).toMatchObject({ kind: "command", command: "closeSpace" });
  });

  it("shows the shortcut of a command and the plugin name of a plugin action", async () => {
    await palette.open();
    const toggle = palette.items().find((i) => i.key === "command:toggleSidebar");
    expect(toggle?.shortcut).not.toBe("");
    expect(toggle?.origin).toBe("");
    const plugin = palette.items().find((i) => i.key === "plugin:review-popup:open");
    expect(plugin).toMatchObject({ origin: "review-popup", shortcut: "" });
  });

  it("groups an empty query under Poimen and one heading per plugin", async () => {
    await palette.open();
    const headings = palette.rows().flatMap((r) => (r.type === "heading" ? [r.label] : []));
    expect(headings).toEqual(["Poimen", "link-trim", "review-popup", "deliver-board"]);
    expect(palette.rows()[0]).toMatchObject({ type: "heading", label: "Poimen" });
    expect(palette.items()).toHaveLength(palette.rows().filter((r) => r.type === "item").length);
  });

  it("shows one ranked list with title match positions for a query", async () => {
    await palette.open();
    palette.setQuery("zoom");
    expect(palette.rows().every((r) => r.type === "item")).toBe(true);
    const first = palette.rows()[0];
    expect(first.type === "item" && first.positions).toEqual([0, 1, 2, 3]);
  });

  it("lists the last run entries under Recent, newest first, without repeating them", async () => {
    await palette.open();
    for (const key of ["command:closeTab", "command:refresh", "plugin:review-popup:open"]) {
      palette.recordRun(palette.items().find((i) => i.key === key)!);
    }
    palette.recordRun(palette.items().find((i) => i.key === "command:closeTab")!);
    await palette.open();
    const rows = palette.rows();
    expect(rows[0]).toMatchObject({ type: "heading", label: "Recent" });
    expect(rows.slice(1, 4).map((r) => r.key)).toEqual([
      "command:closeTab",
      "plugin:review-popup:open",
      "command:refresh",
    ]);
    expect(rows.filter((r) => r.key === "command:closeTab")).toHaveLength(1);
    expect(rows[4]).toMatchObject({ type: "heading", label: "Poimen" });
  });

  it("keeps only the last five run entries and ignores ones that no longer exist", async () => {
    await palette.open();
    const keys = ["newTerminal", "closeTab", "closeAll", "refresh", "toggleSidebar", "splitRight"];
    for (const k of keys) palette.recordRun(palette.items().find((i) => i.key === `command:${k}`)!);
    palette.recordRun({ ...palette.items()[0], key: "gone" });
    await palette.open();
    const recent = palette
      .rows()
      .slice(1, 5)
      .map((r) => r.key);
    expect(recent).toEqual([
      "command:splitRight",
      "command:toggleSidebar",
      "command:refresh",
      "command:closeAll",
    ]);
  });

  it("filters fuzzily across commands and plugin actions", async () => {
    await palette.open();
    palette.setQuery("revpop");
    expect(palette.items().map((i) => i.title)).toEqual(["Open the review popup"]);
    palette.setQuery("zoom");
    expect(palette.items().every((i) => i.kind === "command")).toBe(true);
    palette.setQuery("qqqq");
    expect(palette.items()).toEqual([]);
    expect(palette.current()).toBeUndefined();
  });

  it("moves the highlight with wrap-around and resets it on a new query", async () => {
    await palette.open();
    const n = palette.items().length;
    palette.move(-1);
    expect(palette.selected()).toBe(n - 1);
    palette.move(1);
    expect(palette.selected()).toBe(0);
    palette.move(1);
    palette.setQuery("a");
    expect(palette.selected()).toBe(0);
  });

  it("keeps the commands usable when the plugin actions cannot be listed", async () => {
    tauri.answer("plugin_actions", () => {
      throw "herdr socket: refused";
    });
    await palette.open();
    expect(palette.loadError()).toBe("herdr socket: refused");
    expect(palette.items().length).toBeGreaterThan(0);
    expect(palette.items().some((i) => i.kind === "plugin")).toBe(false);
  });

  it("invokes an action with the focused pane and its workspace as context", async () => {
    expect(await palette.invoke(action("review-popup", "open", "Open"), "w1:p1")).toBeNull();
    expect(tauri.calls("plugin_invoke")).toEqual([
      {
        pluginId: "review-popup",
        actionId: "open",
        context: {
          focused_pane_id: "w1:p1",
          focused_pane_cwd: "/a",
          focused_pane_agent: null,
          tab_id: "w1:t1",
          tab_label: "main",
          workspace_id: "w1",
          workspace_label: "One",
        },
      },
    ]);
  });

  it("passes an empty context without a focused pane", async () => {
    await palette.invoke(action("x", "y", "Y"), null);
    const [call] = tauri.calls("plugin_invoke") as { context: Record<string, unknown> }[];
    expect(Object.values(call.context).every((v) => v === null)).toBe(true);
  });

  it("returns the error of a failed action", async () => {
    tauri.answer("plugin_invoke", () => {
      throw "herdr plugin_action_not_found: nope";
    });
    expect(await palette.invoke(action("x", "y", "Do Y"), null)).toBe(
      "Do Y: herdr plugin_action_not_found: nope",
    );
  });
});
