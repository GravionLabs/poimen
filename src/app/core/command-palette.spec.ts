import { Injector, signal } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AgentPicker } from "./agent-picker";
import { AgentPrompts } from "./agent-prompt";
import { AgentActions } from "../github/agent-actions";
import { IssueTasks } from "../github/issue-task";
import { AgentRuns } from "../github/agent-runs";
import { GithubFilter } from "../github/github-filter";
import { GithubIssues } from "../github/issues";
import { GithubMilestones } from "../github/milestones";
import { GithubPulls } from "../github/pulls";
import { CommandPalette, POIMEN_COMMANDS } from "./command-palette";
import { EditorTabs } from "./editor-tabs";
import { ProjectEntries } from "./project-entries";
import { SpaceActions } from "./space-actions";
import type { Snapshot } from "./snapshot";
import { SpacesStore } from "./spaces-store";
import { Keymap } from "./keymap";
import { UI_STORAGE, UiState } from "./ui-state";
import { DEFAULT_SETTINGS, SettingsStore } from "./settings";
import type { Snippet } from "./snippets-model";
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
    {
      terminal_id: "w1:p2",
      tab_id: "w1:t1",
      space_id: "w1",
      agent_status: "idle",
      cwd: "/a",
      agent: "claude",
    },
  ],
};

describe("CommandPalette", () => {
  let palette: CommandPalette;
  let project: ProjectEntries;
  let settings: SettingsStore;
  const active = signal<string | null>("w1:p1");

  beforeEach(async () => {
    tauri.reset();
    active.set("w1:p1");
    tauri.answer("snapshot", () => snapshot);
    const injector = Injector.create({
      providers: [
        { provide: UI_STORAGE, useValue: null },
        TauriBridge,
        SpacesStore,
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
        { provide: EditorTabs, useValue: { activeId: active } },
        { provide: IssueTasks, useValue: { load: async () => {}, label: () => "Start task…" } },
        CommandPalette,
      ],
    });
    await injector.get(SpacesStore).refresh();
    palette = injector.get(CommandPalette);
    project = injector.get(ProjectEntries);
    settings = injector.get(SettingsStore);
  });

  it("lists snippets as Snippet: <label>, an agent one only while the active pane is an agent", () => {
    const snippets: Snippet[] = [
      { label: "tests", text: "cargo test", enter: true, target: "terminal" },
      { label: "deliver", text: "/deliver-loop", enter: true, target: "agent" },
    ];
    settings["_settings"].set({ ...DEFAULT_SETTINGS, snippets });
    palette.open();
    const titles = () => palette.items().flatMap((i) => (i.snippet ? [i.title] : []));
    expect(titles()).toEqual(["Snippet: tests"]);
    expect(palette.items().find((i) => i.snippet)).toMatchObject({ command: "snippet" });
    active.set("w1:p2");
    expect(titles()).toEqual(["Snippet: tests", "Snippet: deliver"]);
    active.set(null);
    expect(titles()).toEqual([]);
  });

  describe("tabs", () => {
    const entries = (commands: string[], presets: string[] = []) => {
      tauri.answer("project_entries", () => ({
        commands: commands.map((name) => ({ name })),
        presets,
        locked: false,
      }));
      project.setDir("/a");
    };
    const ids = () => palette.sourceTabs().map((t) => t.id);

    it("hides a tab without entries; All is always there", async () => {
      entries([]);
      await project.reload();
      palette.open();
      expect(ids()).toEqual(["all", "commands"]);
      entries(["build"]);
      await project.reload();
      expect(ids()).toEqual(["all", "commands", "project"]);
    });

    it("filters the list by tab and keeps the query when the tab changes", async () => {
      entries(["build"], ["duo"]);
      await project.reload();
      palette.open();
      palette.selectTab("project");
      expect(palette.items().map((i) => i.title)).toEqual(["Project: build", "Preset: duo"]);
      palette.setQuery("duo");
      expect(palette.items().map((i) => i.title)).toEqual(["Preset: duo"]);
      palette.stepTab(-1);
      expect(palette.tab()).toBe("commands");
      expect(palette.query()).toBe("duo");
      expect(palette.items().every((i) => i.source !== "project")).toBe(true);
    });

    it("cycles with step, wrapping around", async () => {
      entries(["build"]);
      await project.reload();
      palette.open();
      const seen = [palette.tab()];
      for (let n = 0; n < 3; n++) {
        palette.stepTab(1);
        seen.push(palette.tab());
      }
      expect(seen).toEqual(["all", "commands", "project", "all"]);
      palette.stepTab(-1);
      expect(palette.tab()).toBe("project");
    });

    it("remembers the last tab, and opens on All while it is hidden", async () => {
      entries(["build"]);
      await project.reload();
      palette.open();
      palette.selectTab("project");
      palette.close();
      palette.open();
      expect(palette.tab()).toBe("project");
      palette.close();
      entries([]);
      await project.reload();
      palette.open();
      expect(palette.tab()).toBe("all");
    });

    it("labels each entry with its source", async () => {
      entries(["build"]);
      await project.reload();
      palette.open();
      const source = (title: string) => palette.items().find((i) => i.title === title);
      expect(palette.sourceLabel(source("Project: build")!)).toBe("Project");
      expect(palette.sourceLabel(source("Open Settings")!)).toBe("Poimen");
    });
  });

  it("lists the poimen commands in order and nothing else", () => {
    palette.open();
    const items = palette.items();
    expect(items[0]).toMatchObject({ command: "newTerminal" });
    expect(items.map((i) => i.command)).toEqual(POIMEN_COMMANDS.map((c) => c.id));
  });

  it("offers the sort commands, and Move Space only in manual mode", async () => {
    palette.open();
    expect(palette.items().map((i) => i.command)).toEqual(
      expect.arrayContaining([
        "sortSpacesManual",
        "sortSpacesByName",
        "sortSpacesByAttention",
        "sortSpacesRecent",
        "sortSpacesNow",
        "moveSpaceUp",
      ]),
    );
    await settings.setSidebarSort("name");
    const commands = palette.items().map((i) => i.command);
    expect(commands).toContain("sortSpacesNow");
    expect(commands).not.toContain("moveSpaceUp");
    expect(commands).not.toContain("moveSpaceDown");
  });

  it("offers no plugin entries", () => {
    palette.open();
    palette.setQuery("plugin");
    expect(palette.items()).toEqual([]);
  });

  it("offers Close Space for the active space", async () => {
    palette.open();
    palette.setQuery("close space");
    expect(palette.items()[0]).toMatchObject({ command: "closeSpace" });
  });

  it("offers Rename Space", async () => {
    palette.open();
    palette.setQuery("rename space");
    expect(palette.items()[0]).toMatchObject({ command: "renameSpace" });
  });

  it("shows the shortcut of a command", async () => {
    palette.open();
    const toggle = palette.items().find((i) => i.key === "command:toggleSidebar");
    expect(toggle?.shortcut).not.toBe("");
  });

  it("groups an empty query under Poimen", async () => {
    palette.open();
    const headings = palette.rows().flatMap((r) => (r.type === "heading" ? [r.label] : []));
    expect(headings).toEqual(["Poimen"]);
    expect(palette.rows()[0]).toMatchObject({ type: "heading", label: "Poimen" });
    expect(palette.items()).toHaveLength(palette.rows().filter((r) => r.type === "item").length);
  });

  it("shows one ranked list with title match positions for a query", async () => {
    palette.open();
    palette.setQuery("zoom");
    expect(palette.rows().every((r) => r.type === "item")).toBe(true);
    const first = palette.rows()[0];
    expect(first.type === "item" && first.positions).toEqual([0, 1, 2, 3]);
  });

  it("lists the last run entries under Recent, newest first, without repeating them", async () => {
    palette.open();
    for (const key of ["command:closeTab", "command:refresh", "command:zoomIn"]) {
      palette.recordRun(palette.items().find((i) => i.key === key)!);
    }
    palette.recordRun(palette.items().find((i) => i.key === "command:closeTab")!);
    palette.open();
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
    palette.open();
    const keys = ["newTerminal", "closeTab", "closeAll", "refresh", "toggleSidebar", "splitRight"];
    for (const k of keys) palette.recordRun(palette.items().find((i) => i.key === `command:${k}`)!);
    palette.recordRun({ ...palette.items()[0], key: "gone" });
    palette.open();
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
    palette.open();
    palette.setQuery("tglsb");
    expect(palette.items().map((i) => i.title)).toEqual(["Toggle Sidebar"]);
    palette.setQuery("zoom");
    expect(palette.items().map((i) => i.title)).toEqual(["Zoom In", "Zoom Out", "Reset Zoom"]);
    palette.setQuery("qqqq");
    expect(palette.items()).toEqual([]);
    expect(palette.current()).toBeUndefined();
  });

  it("moves the highlight with wrap-around and resets it on a new query", async () => {
    palette.open();
    const n = palette.items().length;
    palette.move(-1);
    expect(palette.selected()).toBe(n - 1);
    palette.move(1);
    expect(palette.selected()).toBe(0);
    palette.move(1);
    palette.setQuery("a");
    expect(palette.selected()).toBe(0);
  });

  describe("detected scripts", () => {
    const scripts = [
      { source: "npm", name: "build", description: "tsc -p ." },
      { source: "npm", name: "test", description: "vitest" },
    ];
    async function load(commands: string[] = []) {
      tauri.answer("project_entries", () => ({
        commands: commands.map((name) => ({ name, target: "terminal" })),
        presets: [],
        locked: false,
      }));
      tauri.answer("detected_entries", () => scripts);
      project.setDir("/a");
      await project.reload();
      palette.open();
    }

    it("shows a Scripts tab with the command line of each script as its description", async () => {
      await load();
      expect(palette.sourceTabs().map((t) => t.id)).toContain("scripts");
      palette.selectTab("scripts");
      expect(palette.items().map((i) => [i.title, i.description])).toEqual([
        ["Script: build", "tsc -p ."],
        ["Script: test", "vitest"],
      ]);
      expect(palette.items()[0].command).toBe("detectedEntry");
      expect(palette.items()[0].detected).toEqual(scripts[0]);
    });

    it("has no Scripts tab without detected scripts", async () => {
      tauri.answer("detected_entries", () => []);
      project.setDir("/a");
      await project.reload();
      expect(palette.sourceTabs().map((t) => t.id)).not.toContain("scripts");
    });

    it("hides a script whose name a commands[] entry of the project file has", async () => {
      await load(["test"]);
      palette.selectTab("scripts");
      expect(palette.items().map((i) => i.title)).toEqual(["Script: build"]);
      palette.selectTab("all");
      const titles = palette.items().map((i) => i.title);
      expect(titles).toContain("Project: test");
      expect(titles).not.toContain("Script: test");
    });
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
