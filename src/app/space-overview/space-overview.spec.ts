// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { EditorTabs } from "../core/editor-tabs";
import type { PaneRow, SpaceNode } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { PaneActions } from "../core/pane-actions";
import { SpaceActions } from "../core/space-actions";
import { TauriBridge } from "../core/tauri";
import { LayoutTabsEditor } from "../settings-view/layout-tabs-editor";
import { ProjectLayouts } from "./project-layouts";
import { SpaceOverview } from "./space-overview";

const node = (rows: PaneRow[]): SpaceNode => ({
  workspace: { space_id: "w1", label: "One", number: 1, agent_status: "idle" },
  rows,
});
const mk = (id: string, agent?: string, extra: Partial<PaneRow> = {}): PaneRow => ({
  pane: { terminal_id: id, tab_id: "t", space_id: "w1", agent, agent_status: "idle" },
  tab: "t",
  name: id,
  worktree: false,
  ...extra,
});

function setup(rows: PaneRow[], spaceId = "w1", file?: string) {
  const spaces = signal<SpaceNode[]>([node(rows)]);
  const editor = { open: vi.fn(), openFlow: vi.fn() };
  const paneActions = {
    askRename: vi.fn(),
    askClose: vi.fn(),
    askNew: vi.fn(),
    askStartAgent: vi.fn(),
  };
  const files: { text: string; exists: boolean } = { text: "", exists: false };
  if (file !== undefined) {
    files.text = file;
    files.exists = true;
  }
  const saves: unknown[] = [];
  let imported: unknown = { tabs: [{ name: "imp" }] };
  const outcomes: unknown[] = [];
  const bridge = {
    openUrl: vi.fn(),
    pickSaveFile: vi.fn(async () => "/tmp/out.json" as string | null),
    pickJsonFile: vi.fn(async () => "/tmp/in.json" as string | null),
    invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
      if (command === "layout_export") return null;
      if (command === "layout_import") return imported;
      if (command === "project_file") return { root: "/work", ...files };
      if (command === "project_save") {
        saves.push(args);
        return outcomes.shift() ?? { saved: true, errors: [], trust: null, store: null };
      }
      throw new Error(command);
    }),
  };
  const spaceActions = { rename: vi.fn(async () => true), renameError: () => null };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: SpacesStore, useValue: { spaces } },
      { provide: EditorTabs, useValue: editor },
      { provide: PaneActions, useValue: paneActions },
      { provide: TauriBridge, useValue: bridge },
      { provide: SpaceActions, useValue: spaceActions },
    ],
  });
  TestBed.overrideComponent(LayoutTabsEditor, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      styleUrls: undefined,
      template: readFileSync("src/app/settings-view/layout-tabs-editor.html", "utf8"),
    },
  });
  TestBed.overrideComponent(ProjectLayouts, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/space-overview/project-layouts.html", "utf8"),
    },
  });
  TestBed.overrideComponent(SpaceOverview, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/space-overview/space-overview.html", "utf8"),
    },
  });
  const fixture = TestBed.createComponent(SpaceOverview);
  fixture.componentInstance.space = spaceId;
  const out = { output: [] as string[], explorer: [] as string[], del: [] as string[] };
  fixture.componentInstance.outputRequested.subscribe((v) => out.output.push(v));
  fixture.componentInstance.explorerRequested.subscribe((v) => out.explorer.push(v));
  fixture.componentInstance.deleteRequested.subscribe((v) => out.del.push(v));
  fixture.detectChanges();
  const root = fixture.nativeElement as HTMLElement;
  const click = (sel: string, at = 0) => {
    root.querySelectorAll<HTMLButtonElement>(sel)[at].click();
  };
  return {
    fixture,
    root,
    spaces,
    editor,
    paneActions,
    bridge,
    out,
    click,
    saves,
    outcomes,
    spaceActions,
    setImported: (v: unknown) => {
      imported = v;
    },
  };
}

describe("SpaceOverview", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("lists the panes of the space with kind and status", () => {
    const { root } = setup([mk("w1:p1"), mk("w1:p2", "claude")]);
    const kinds = [...root.querySelectorAll(".pane .kind")].map((e) => e.textContent?.trim());
    expect(kinds).toEqual(["Terminal", "Agent claude"]);
    expect(root.querySelector("h2")?.textContent).toContain("Space: One");
    expect(root.querySelector(".pane .status")?.textContent).toContain("idle");
  });

  it("follows the store when panes are added", () => {
    const { fixture, root, spaces } = setup([mk("w1:p1")]);
    expect(root.querySelectorAll(".pane")).toHaveLength(1);
    spaces.set([node([mk("w1:p1"), mk("w1:p2")])]);
    fixture.detectChanges();
    expect(root.querySelectorAll(".pane")).toHaveLength(2);
  });

  it("dispatches the pane actions", () => {
    const { root, editor, paneActions, out, click } = setup([mk("w1:p1"), mk("w1:p2", "claude")]);
    click(".act-focus", 1);
    expect(editor.open).toHaveBeenCalledWith("w1:p2", true);
    click(".act-flow");
    expect(editor.openFlow).toHaveBeenCalledWith("w1:p2");
    click(".act-output");
    expect(out.output).toEqual(["w1:p2"]);
    click(".act-restart");
    expect(paneActions.askStartAgent).toHaveBeenCalledWith("w1:p2");
    click(".act-rename", 0);
    expect(paneActions.askRename).toHaveBeenCalledWith("w1:p1");
    click(".act-close", 1);
    expect(paneActions.askClose).toHaveBeenCalledWith("w1:p2");
    // Flow, Output and Restart exist for the agent pane only.
    expect(root.querySelectorAll(".act-flow")).toHaveLength(1);
  });

  it("dispatches the space actions", () => {
    const pr = { number: 1, url: "https://github.com/a/b/pull/1" } as PaneRow["pr"];
    const { paneActions, bridge, out, click } = setup([
      mk("w1:p1", undefined, { dir: "/work", pr }),
    ]);
    click(".act-new");
    expect(paneActions.askNew).toHaveBeenCalledWith("w1");
    click(".act-explorer");
    expect(out.explorer).toEqual(["/work"]);
    click(".act-pr");
    expect(bridge.openUrl).toHaveBeenCalledWith("https://github.com/a/b/pull/1");
    click(".act-delete");
    expect(out.del).toEqual(["w1"]);
  });

  it("disables explorer and PR without a directory or pull request", () => {
    const { root } = setup([mk("w1:p1")]);
    expect(root.querySelector<HTMLButtonElement>(".act-explorer")!.disabled).toBe(true);
    expect(root.querySelector<HTMLButtonElement>(".act-pr")!.disabled).toBe(true);
  });

  it("says so when the space is gone", () => {
    const { root } = setup([mk("w1:p1")], "w9");
    expect(root.textContent).toContain("no longer exists");
  });

  describe("configuration section", () => {
    const withDir = () => [mk("w1:p1", undefined, { dir: "/work" })];
    const settle = async (fixture: { detectChanges(): void }) => {
      await new Promise((r) => setTimeout(r, 0));
      fixture.detectChanges();
    };
    const type = (root: HTMLElement, sel: string, value: string) => {
      const el = root.querySelector<HTMLInputElement>(sel)!;
      el.value = value;
      el.dispatchEvent(new Event("input"));
    };

    it("loads the fields of the project file and the directory", async () => {
      const file = JSON.stringify({
        setup: "pnpm i",
        teardown: "rm x",
        layout: { panes: [{ name: "a" }] },
      });
      const { fixture, root } = setup(withDir(), "w1", file);
      await settle(fixture);
      expect(root.querySelector<HTMLInputElement>(".cfg-setup")!.value).toBe("pnpm i");
      expect(root.querySelector<HTMLInputElement>(".cfg-teardown")!.value).toBe("rm x");
      expect(root.querySelector<HTMLTextAreaElement>(".cfg-layout")!.value).toContain(
        '"name": "a"',
      );
      expect(root.querySelector(".cfg-dir")?.textContent).toBe("/work");
      expect(root.querySelector<HTMLInputElement>(".cfg-name")!.value).toBe("One");
    });

    it("saves every field as a patch", async () => {
      const { fixture, root, saves } = setup(withDir(), "w1", "{}");
      await settle(fixture);
      type(root, ".cfg-setup", "make");
      root.querySelector<HTMLButtonElement>(".cfg-save")!.click();
      await settle(fixture);
      expect(saves).toEqual([
        {
          dir: "/work",
          patch: { setup: "make", teardown: "", layout: null, presets: null },
          decision: undefined,
        },
      ]);
      expect(root.querySelector(".status-note")?.textContent).toContain("Saved");
    });

    it("shows a layout that is not JSON inline and does not save", async () => {
      const { fixture, root, saves } = setup(withDir());
      await settle(fixture);
      type(root, ".cfg-layout", "{ nope");
      root.querySelector<HTMLButtonElement>(".cfg-save")!.click();
      await settle(fixture);
      expect(saves).toEqual([]);
      expect(root.querySelector(".config .error")?.textContent).toContain("Not valid JSON");
    });

    it("shows backend validation errors next to the field", async () => {
      const { fixture, root, outcomes } = setup(withDir());
      await settle(fixture);
      outcomes.push({
        saved: false,
        errors: [{ field: "layout", message: '"panes" must be a non-empty list' }],
        trust: null,
        store: null,
      });
      type(root, ".cfg-layout", '{"panes": []}');
      root.querySelector<HTMLButtonElement>(".cfg-save")!.click();
      await settle(fixture);
      expect(root.querySelector(".config .error")?.textContent).toContain("non-empty");
    });

    it("asks to trust new commands and saves with the hash on confirmation", async () => {
      const { fixture, root, saves, outcomes } = setup(withDir());
      await settle(fixture);
      outcomes.push({
        saved: false,
        errors: [],
        trust: {
          root: "/work",
          files: [],
          hash: "abc",
          commands: [{ what: "setup", command: "make" }],
        },
        store: null,
      });
      type(root, ".cfg-setup", "make");
      root.querySelector<HTMLButtonElement>(".cfg-save")!.click();
      await settle(fixture);
      expect(root.querySelector(".trust")?.textContent).toContain("make");
      root.querySelector<HTMLButtonElement>(".cfg-trust")!.click();
      await settle(fixture);
      expect((saves[1] as { decision: unknown }).decision).toEqual({ kind: "trust", hash: "abc" });
      expect(root.querySelector(".trust")).toBeNull();
    });

    const NESTED = {
      tabs: [
        {
          name: "agent",
          command: "claude",
          children: [
            {
              name: "tests",
              place: { direction: "down", of: "agent", ratio: 0.3 },
              root: {
                kind: "split",
                direction: "right",
                ratio: 0.4,
                children: [
                  { kind: "terminal", name: "unit", command: "pnpm test" },
                  { kind: "terminal", name: "e2e" },
                ],
              },
            },
          ],
        },
      ],
    };

    it("opens the project layout and each preset in the layout editor", async () => {
      const file = JSON.stringify({
        layout: NESTED,
        presets: [
          {
            name: "duo",
            panes: [{ name: "a" }, { name: "b" }].map((p, i) =>
              i ? { ...p, split: { direction: "right" } } : p,
            ),
          },
        ],
      });
      const { fixture, root } = setup(withDir(), "w1", file);
      await settle(fixture);
      const names = (scope: Element) =>
        [...scope.querySelectorAll<HTMLInputElement>("[data-tab]")].map((i) => i.value);
      expect(names(root.querySelector(".cfg-layout-editor")!)).toEqual(["agent", "tests"]);
      expect(names(root.querySelector('[data-preset="0"]')!)).toEqual(["a"]);
      expect(root.querySelector<HTMLInputElement>(".cfg-preset-name")!.value).toBe("duo");
      // The JSON stays available as Advanced.
      expect(root.querySelector<HTMLTextAreaElement>(".cfg-layout")!.value).toContain('"place"');
    });

    it("saves an unchanged nested layout and presets as they were read", async () => {
      const presets = [{ name: "p", tabs: NESTED.tabs }];
      const { fixture, root, saves } = setup(
        withDir(),
        "w1",
        JSON.stringify({ layout: NESTED, presets }),
      );
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-save")!.click();
      await settle(fixture);
      expect(saves).toEqual([
        {
          dir: "/work",
          patch: { setup: "", teardown: "", layout: NESTED, presets },
          decision: undefined,
        },
      ]);
    });

    it("keeps nested tabs and place when another tab is edited", async () => {
      const { fixture, root, saves } = setup(withDir(), "w1", JSON.stringify({ layout: NESTED }));
      await settle(fixture);
      root.querySelector<HTMLButtonElement>('[aria-label="Add child tab to agent"]')!.click();
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-save")!.click();
      await settle(fixture);
      const layout = (saves[0] as { patch: { layout: { tabs: Record<string, unknown>[] } } }).patch
        .layout;
      const children = layout.tabs[0]["children"] as Record<string, unknown>[];
      expect(children).toHaveLength(2);
      expect(children[0]).toEqual(NESTED.tabs[0].children[0]);
      expect(children[1]["name"]).toBe("tab");
    });

    it("adds a layout and a preset to a file without them", async () => {
      const { fixture, root, saves } = setup(withDir(), "w1", "{}");
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-layout-add")!.click();
      root.querySelector<HTMLButtonElement>(".cfg-preset-add")!.click();
      await settle(fixture);
      type(root, ".cfg-preset-name", "review");
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-save")!.click();
      await settle(fixture);
      const patch = (saves[0] as { patch: Record<string, unknown> }).patch;
      expect(patch["layout"]).toEqual({ tabs: [{ name: "shell" }] });
      expect(patch["presets"]).toEqual([{ name: "review", tabs: [{ name: "shell" }] }]);
    });

    it("does not save while a preset name repeats", async () => {
      const presets = [
        { name: "a", panes: [{ name: "x" }] },
        { name: "b", panes: [{ name: "x" }] },
      ];
      const { fixture, root, saves } = setup(withDir(), "w1", JSON.stringify({ presets }));
      await settle(fixture);
      const inputs = root.querySelectorAll<HTMLInputElement>(".cfg-preset-name");
      inputs[1].value = "a";
      inputs[1].dispatchEvent(new Event("input"));
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-save")!.click();
      await settle(fixture);
      expect(saves).toEqual([]);
      expect(root.querySelector(".config .error")?.textContent).toContain(
        'Two presets are called "a"',
      );
    });

    it("asks trust again after a layout change that adds a command", async () => {
      const { fixture, root, saves, outcomes } = setup(
        withDir(),
        "w1",
        JSON.stringify({ layout: { tabs: [{ name: "agent" }] } }),
      );
      await settle(fixture);
      outcomes.push({
        saved: false,
        errors: [],
        trust: {
          root: "/work",
          files: [],
          hash: "h2",
          commands: [{ what: 'layout: pane "agent"', command: "claude" }],
        },
        store: null,
      });
      type(root, 'input[aria-label="Command in agent"]', "claude");
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-save")!.click();
      await settle(fixture);
      expect((saves[0] as { patch: { layout: unknown } }).patch.layout).toEqual({
        tabs: [{ name: "agent", command: "claude" }],
      });
      expect(root.querySelector(".trust")?.textContent).toContain("claude");
      root.querySelector<HTMLButtonElement>(".cfg-trust")!.click();
      await settle(fixture);
      expect((saves[1] as { decision: unknown }).decision).toEqual({ kind: "trust", hash: "h2" });
    });

    it("shows a layout the editor cannot read under Advanced only", async () => {
      const { fixture, root } = setup(withDir(), "w1", JSON.stringify({ layout: { weird: 1 } }));
      await settle(fixture);
      expect(root.querySelector(".cfg-layout-advanced")).not.toBeNull();
      expect(root.querySelector(".cfg-layout-editor")).toBeNull();
      expect(root.querySelector<HTMLTextAreaElement>(".cfg-layout")!.value).toContain("weird");
    });

    it("exports the layout and a preset to a JSON file", async () => {
      const presets = [{ name: "p", tabs: NESTED.tabs }];
      const { fixture, root, bridge } = setup(
        withDir(),
        "w1",
        JSON.stringify({ layout: NESTED, presets }),
      );
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-layout-export")!.click();
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-preset-export")!.click();
      await settle(fixture);
      expect(bridge.pickSaveFile).toHaveBeenCalledWith("layout.json");
      expect(bridge.pickSaveFile).toHaveBeenCalledWith("p.json");
      const exports = bridge.invoke.mock.calls.filter((c) => c[0] === "layout_export");
      expect(exports.map((c) => c[1])).toEqual([
        { path: "/tmp/out.json", layout: NESTED },
        { path: "/tmp/out.json", layout: { tabs: NESTED.tabs } },
      ]);
    });

    it("imports a layout file into the layout and into a preset, and shows a refusal", async () => {
      const presets = [{ name: "p", panes: [{ name: "x" }] }];
      const { fixture, root, saves, bridge } = setup(withDir(), "w1", JSON.stringify({ presets }));
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-layout-import")!.click();
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-preset-import")!.click();
      await settle(fixture);
      root.querySelector<HTMLButtonElement>(".cfg-save")!.click();
      await settle(fixture);
      const patch = (saves[0] as { patch: Record<string, unknown> }).patch;
      expect(patch["layout"]).toEqual({ tabs: [{ name: "imp" }] });
      expect(patch["presets"]).toEqual([{ name: "p", tabs: [{ name: "imp" }] }]);
      bridge.invoke.mockImplementationOnce(async () => {
        throw new Error("not a layout");
      });
      root.querySelector<HTMLButtonElement>(".cfg-layout-import")!.click();
      await settle(fixture);
      expect(root.querySelector(".cfg-transfer-error")?.textContent).toContain("not a layout");
    });

    it("blocks saving a file that is not valid JSON", async () => {
      const { fixture, root } = setup(withDir(), "w1", "{ broken");
      await settle(fixture);
      expect(root.querySelector(".cfg-file-error")?.textContent).toContain("not valid JSON");
      expect(root.querySelector<HTMLButtonElement>(".cfg-save")!.disabled).toBe(true);
    });

    it("renames the space", async () => {
      const { fixture, root, spaceActions } = setup(withDir());
      await settle(fixture);
      type(root, ".cfg-name", "Renamed");
      root.querySelector<HTMLButtonElement>(".cfg-rename")!.click();
      await settle(fixture);
      expect(spaceActions.rename).toHaveBeenCalledWith("w1", "Renamed");
    });

    it("scrolls to the section when the nonce changes", async () => {
      const { fixture, root } = setup(withDir());
      const scroll = vi.fn();
      (root.querySelector("#config") as HTMLElement).scrollIntoView = scroll;
      fixture.componentInstance.configNonce = 1;
      fixture.detectChanges();
      await settle(fixture);
      expect(scroll).toHaveBeenCalled();
    });
  });
});
