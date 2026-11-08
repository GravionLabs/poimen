// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { EditorTabs } from "../core/editor-tabs";
import type { PaneRow, SpaceNode } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { PaneActions } from "../core/pane-actions";
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

function setup(rows: PaneRow[], spaceId = "w1") {
  const spaces = signal<SpaceNode[]>([node(rows)]);
  const editor = { open: vi.fn(), openFlow: vi.fn() };
  const paneActions = {
    askRename: vi.fn(),
    askClose: vi.fn(),
    askNew: vi.fn(),
    askStartAgent: vi.fn(),
  };
  const bridge = { openUrl: vi.fn() };
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      { provide: HerdrStore, useValue: { spaces } },
      { provide: EditorTabs, useValue: editor },
      { provide: PaneActions, useValue: paneActions },
      { provide: TauriBridge, useValue: bridge },
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
  return { fixture, root, spaces, editor, paneActions, bridge, out, click };
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
});
