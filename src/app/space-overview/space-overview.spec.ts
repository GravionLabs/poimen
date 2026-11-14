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
  const outcomes: unknown[] = [];
  const bridge = {
    openUrl: vi.fn(),
    invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
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
        { dir: "/work", patch: { setup: "make", teardown: "", layout: null }, decision: undefined },
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
