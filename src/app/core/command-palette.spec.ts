import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CommandPalette, POIMEN_COMMANDS } from "./command-palette";
import type { Snapshot } from "./herdr";
import { HerdrStore } from "./herdr-store";
import { Keymap } from "./keymap";
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

const snapshot: Snapshot = {
  spaces: [{ space_id: "w1", label: "One", number: 1, agent_status: "idle" }],
  tabs: [{ tab_id: "w1:t1", space_id: "w1", label: "main", number: 1, agent_status: "idle" }],
  terminals: [
    { terminal_id: "w1:p1", tab_id: "w1:t1", space_id: "w1", agent_status: "idle", cwd: "/a" },
  ],
};

describe("CommandPalette", () => {
  let palette: CommandPalette;

  beforeEach(async () => {
    tauri.reset();
    tauri.answer("snapshot", () => snapshot);
    const injector = Injector.create({
      providers: [
        { provide: UI_STORAGE, useValue: null },
        TauriBridge,
        HerdrStore,
        SettingsStore,
        Keymap,
        UiState,
        CommandPalette,
      ],
    });
    await injector.get(HerdrStore).refresh();
    palette = injector.get(CommandPalette);
  });

  it("lists the poimen commands in order and nothing else", () => {
    palette.open();
    const items = palette.items();
    expect(items[0]).toMatchObject({ command: "newTerminal" });
    expect(items.map((i) => i.command)).toEqual(POIMEN_COMMANDS.map((c) => c.id));
  });

  it("offers no plugin entries", () => {
    palette.open();
    palette.setQuery("plugin");
    expect(palette.items()).toEqual([]);
  });

  it("offers Close Space for the active space", async () => {
    await palette.open();
    palette.setQuery("close space");
    expect(palette.items()[0]).toMatchObject({ command: "closeSpace" });
  });

  it("shows the shortcut of a command", async () => {
    await palette.open();
    const toggle = palette.items().find((i) => i.key === "command:toggleSidebar");
    expect(toggle?.shortcut).not.toBe("");
  });

  it("groups an empty query under Poimen", async () => {
    await palette.open();
    const headings = palette.rows().flatMap((r) => (r.type === "heading" ? [r.label] : []));
    expect(headings).toEqual(["Poimen"]);
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
    for (const key of ["command:closeTab", "command:refresh", "command:zoomIn"]) {
      palette.recordRun(palette.items().find((i) => i.key === key)!);
    }
    palette.recordRun(palette.items().find((i) => i.key === "command:closeTab")!);
    await palette.open();
    const rows = palette.rows();
    expect(rows[0]).toMatchObject({ type: "heading", label: "Recent" });
    expect(rows.slice(1, 4).map((r) => r.key)).toEqual([
      "command:closeTab",
      "command:zoomIn",
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

  it("filters fuzzily across the commands", async () => {
    await palette.open();
    palette.setQuery("tglsb");
    expect(palette.items().map((i) => i.title)).toEqual(["Toggle Sidebar"]);
    palette.setQuery("zoom");
    expect(palette.items().map((i) => i.title)).toEqual(["Zoom In", "Zoom Out", "Reset Zoom"]);
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
});
