import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentPicker } from "./agent-picker";
import { AgentPrompts } from "./agent-prompt";
import { AgentActions } from "../github/agent-actions";
import { AgentRuns } from "../github/agent-runs";
import { GithubFilter } from "../github/github-filter";
import { GithubIssues } from "../github/issues";
import { GithubMilestones } from "../github/milestones";
import { GithubPulls } from "../github/pulls";
import { CommandPalette, POIMEN_COMMANDS } from "./command-palette";
import { ProjectEntries } from "./project-entries";
import { SpaceActions } from "./space-actions";
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
  let project: ProjectEntries;

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
        SpaceActions,
        AgentPrompts,
        AgentPicker,
        ProjectEntries,
        GithubFilter,
        GithubMilestones,
        GithubPulls,
        GithubIssues,
        AgentRuns,
        AgentActions,
        CommandPalette,
      ],
    });
    await injector.get(HerdrStore).refresh();
    palette = injector.get(CommandPalette);
    project = injector.get(ProjectEntries);
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

  it("offers Rename Space", async () => {
    await palette.open();
    palette.setQuery("rename space");
    expect(palette.items()[0]).toMatchObject({ command: "renameSpace" });
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

  describe("project entries", () => {
    const list = (locked: boolean) => ({
      commands: [
        { name: "test", target: "terminal" },
        { name: "review", target: "agent" },
      ],
      presets: ["duo"],
      locked,
    });

    it("lists Project and Preset entries after the poimen commands, in their own group", async () => {
      tauri.answer("project_entries", () => list(false));
      project.setDir("/a");
      await project.reload();
      palette.open();
      const items = palette.items();
      expect(items.slice(-3).map((i) => i.title)).toEqual([
        "Project: test",
        "Project: review",
        "Preset: duo",
      ]);
      expect(items.at(-1)).toMatchObject({
        command: "projectEntry",
        entry: { kind: "preset", name: "duo" },
        locked: false,
      });
      const headings = palette.rows().flatMap((r) => (r.type === "heading" ? [r.label] : []));
      expect(headings).toEqual(["Poimen", "Project"]);
    });

    it("marks every entry locked while the file is untrusted", async () => {
      tauri.answer("project_entries", () => list(true));
      project.setDir("/a");
      await project.reload();
      palette.open();
      expect(
        palette
          .items()
          .filter((i) => i.entry)
          .every((i) => i.locked),
      ).toBe(true);
    });

    it("finds an entry by its name and offers none without a project file", async () => {
      tauri.answer("project_entries", () => list(false));
      project.setDir("/a");
      await project.reload();
      palette.open();
      palette.setQuery("duo");
      expect(palette.items()[0]).toMatchObject({ title: "Preset: duo" });
      project.setDir(undefined);
      await project.reload();
      palette.setQuery("duo");
      expect(palette.items().some((i) => i.entry)).toBe(false);
    });

    it("reads the file again when the palette opens and when the directory changes", async () => {
      tauri.answer("project_entries", () => list(false));
      project.setDir("/a");
      project.setDir("/b");
      await project.reload();
      palette.open();
      expect(tauri.calls("project_entries").map((a) => (a as { dir: string }).dir)).toEqual([
        "/a",
        "/b",
        "/b",
        "/b",
      ]);
    });
  });
});
