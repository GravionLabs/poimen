// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { Pane, SpaceNode } from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import { DEFAULT_SETTINGS, SettingsStore, type SidebarLayout } from "../core/settings";
import { UI_STORAGE } from "../core/ui-state";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { Sidebar, splitSections } from "./sidebar";
import { DEFAULT_SIDEBAR_ROWS, type SidebarRows } from "./spaces";

const mk = (id: string, label: string, panes: [string, string, boolean][]): SpaceNode => ({
  workspace: { workspace_id: id, label, number: 1, agent_status: "idle" },
  rows: panes.map(([pid, name, agent]) => ({
    pane: {
      pane_id: pid,
      tab_id: `${id}:t`,
      workspace_id: id,
      agent_status: "idle",
    } as Pane,
    tab: "t",
    name,
    agent: agent ? "claude" : undefined,
    worktree: false,
  })),
});

const one = mk("w1", "One", [
  ["w1:p1", "agent-a", true],
  ["w1:p2", "shell-a", false],
]);
const two = mk("w2", "Two", [
  ["w2:p1", "shell-b", false],
  ["w2:p2", "agent-b", true],
]);

describe("splitSections", () => {
  it("lists workspaces flat, the selected one with its terminal panes only", () => {
    const { spaces, selectedId } = splitSections([one, two], "w2");
    expect(selectedId).toBe("w2");
    expect(spaces.map((n) => n.key)).toEqual(["workspace:w1", "workspace:w2"]);
    expect(spaces[0].children).toBeUndefined();
    expect(spaces[0].leaf).toBe(true);
    expect(spaces[1].leaf).toBe(false);
    expect(spaces[1].children!.map((n) => n.key)).toEqual(["w2:p1"]);
    expect(spaces[1].data).toMatchObject({ selected: true });
    expect(spaces[0].data).toMatchObject({ selected: false });
  });

  it("selects the first workspace without or with an unknown selection", () => {
    expect(splitSections([one, two], null).selectedId).toBe("w1");
    expect(splitSections([one, two], "gone").selectedId).toBe("w1");
  });

  it("lists all agent panes across workspaces in herdr order with the workspace name", () => {
    const { agents } = splitSections([one, two], null);
    expect(agents.map((n) => n.key)).toEqual(["w1:p1", "w2:p2"]);
    const lines = (agents[1].data as { lines: { kind: string; text: string }[][] }).lines;
    expect(lines.at(-1)).toMatchObject([{ kind: "text", text: "Two" }]);
  });

  it("renders agent rows with sidebar.rows.agent", () => {
    const rows: SidebarRows = { ...DEFAULT_SIDEBAR_ROWS, agent: [[{ token: "tab" }]] };
    const { agents } = splitSections([one], null, rows);
    const lines = (agents[0].data as { lines: { text: string }[][] }).lines;
    expect(lines.map((l) => l.map((s) => s.text))).toEqual([["t"], ["One"]]);
  });

  it("is empty without spaces", () => {
    expect(splitSections([], null)).toEqual({ spaces: [], agents: [], selectedId: null });
  });
});

async function setup(layout: SidebarLayout) {
  TestBed.resetTestingModule();
  const setSidebarLayout = vi.fn(async (layout: SidebarLayout) => void layout);
  const settings = signal({ ...DEFAULT_SETTINGS, sidebarLayout: layout });
  const writeError = signal<string | null>(null);
  TestBed.configureTestingModule({
    providers: [
      {
        provide: HerdrStore,
        useValue: { spaces: signal([one, two]), connectionError: signal(null) },
      },
      { provide: SettingsStore, useValue: { settings, writeError, setSidebarLayout } },
      { provide: UI_STORAGE, useValue: null },
    ],
  });
  TestBed.overrideComponent(Sidebar, {
    set: {
      templateUrl: undefined,
      styleUrl: undefined,
      template: readFileSync("src/app/sidebar/sidebar.html", "utf8"),
    },
  });
  TestBed.overrideComponent(Sidebar, {
    remove: { imports: [SplitPane] },
    add: { imports: [SplitPaneStub] },
  });
  const fixture = TestBed.createComponent(Sidebar);
  const selected: string[] = [];
  const pinned: string[] = [];
  fixture.componentInstance.selected.subscribe((p) => selected.push(p.pane_id));
  fixture.componentInstance.pinned.subscribe((p) => pinned.push(p.pane_id));
  const root = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      fixture.detectChanges();
      await fixture.whenStable();
    }
  };
  await settle();
  const item = (name: string, treeNo?: number) => {
    const scope =
      treeNo === undefined ? root : root.querySelectorAll<HTMLElement>('[role="tree"]')[treeNo];
    // A parent item contains its children: the innermost match is the row itself.
    const found = [...scope.querySelectorAll<HTMLElement>('[role="treeitem"]')]
      .filter((e) => e.textContent?.includes(name))
      .at(-1);
    if (!found) throw new Error(`no row ${name}`);
    return found;
  };
  const click = async (name: string, treeNo?: number) => {
    const target = item(name, treeNo);
    (target.querySelector(".row, .space") as HTMLElement).click();
    await settle();
  };
  const toggle = () => root.querySelector<HTMLButtonElement>(".layout-toggle")!;
  return {
    fixture,
    root,
    settle,
    click,
    item,
    toggle,
    selected,
    pinned,
    settings,
    writeError,
    setSidebarLayout,
    labels: () =>
      [...root.querySelectorAll('[role="tree"]')].map((e) => e.getAttribute("aria-label")),
  };
}

describe("Sidebar toggle", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("shows the other layout in its name and asks for it on a click", async () => {
    const tree = await setup("tree");
    expect(tree.toggle().getAttribute("aria-label")).toBe("Show split layout");
    expect(tree.toggle().title).toBe("Show split layout");
    expect(tree.toggle().querySelector("svg")).not.toBeNull();
    tree.toggle().click();
    expect(tree.setSidebarLayout).toHaveBeenCalledWith("split");

    const split = await setup("split");
    expect(split.toggle().getAttribute("aria-label")).toBe("Show tree layout");
    split.toggle().click();
    expect(split.setSidebarLayout).toHaveBeenCalledWith("tree");
  });

  it("switches the structure when the setting changes", async () => {
    const s = await setup("tree");
    expect(s.labels()).toEqual(["herdr spaces"]);
    s.settings.update((v) => ({ ...v, sidebarLayout: "split" }));
    await s.settle();
    expect(s.labels()).toEqual(["Spaces", "Agents"]);
  });

  it("surfaces a failed write", async () => {
    const s = await setup("tree");
    s.writeError.set("Cannot save the sidebar layout: disk full");
    await s.settle();
    expect(s.root.textContent).toContain("Cannot save the sidebar layout: disk full");
  });
});

describe("Sidebar in the split layout", () => {
  it("separates Spaces and Agents with a resize splitter, the tree layout has none", async () => {
    const s = await setup("split");
    const dividers = s.root.querySelectorAll<HTMLElement>('app-split-pane [role="separator"]');
    expect(dividers).toHaveLength(1);
    const panes = s.root.querySelectorAll("app-split-pane > .pane");
    expect(panes[0].querySelector('[aria-label="Spaces"]')).not.toBeNull();
    expect(panes[1].querySelector('[aria-label="Agents"]')).not.toBeNull();
    expect(s.root.classList.contains("split")).toBe(true);

    dividers[0].dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", shiftKey: true }));
    await s.settle();
    expect(panes[0].getAttribute("style")).toContain("flex-grow: 0.6");

    const tree = await setup("tree");
    expect(tree.root.querySelector("app-split-pane")).toBeNull();
    expect(tree.root.classList.contains("split")).toBe(false);
  });

  it("has two labelled lists, Spaces with the first workspace's terminals and Agents", async () => {
    const s = await setup("split");
    expect(s.labels()).toEqual(["Spaces", "Agents"]);
    expect(s.root.querySelector("#split-spaces")?.textContent).toBe("Spaces");
    const [spaces, agents] = [...s.root.querySelectorAll<HTMLElement>('[role="tree"]')];
    expect(spaces.textContent).toContain("shell-a");
    expect(spaces.textContent).not.toContain("agent-a");
    expect(agents.textContent).toContain("agent-a");
    expect(agents.textContent).toContain("agent-b");
    expect(agents.textContent).toContain("Two");
    expect(s.root.querySelectorAll(".chevron")).toHaveLength(0);
  });

  it("selects a workspace by click and lists its terminals", async () => {
    const s = await setup("split");
    await s.click("Two", 0);
    const spaces = s.root.querySelector<HTMLElement>('[role="tree"]')!;
    expect(spaces.textContent).toContain("shell-b");
    expect(spaces.textContent).not.toContain("shell-a");
    expect(s.selected).toEqual([]);
  });

  it("previews an agent or terminal on a click and pins it on a double click", async () => {
    const s = await setup("split");
    await s.click("agent-b");
    expect(s.selected).toEqual(["w2:p2"]);
    await s.click("agent-b");
    expect(s.pinned).toEqual(["w2:p2"]);
    await s.click("shell-a");
    expect(s.selected).toEqual(["w2:p2", "w1:p2"]);
  });

  it("filters both lists", async () => {
    const s = await setup("split");
    const input = s.root.querySelector<HTMLInputElement>("input.filter")!;
    input.value = "agent-b";
    input.dispatchEvent(new Event("input"));
    await s.settle();
    const [spaces, agents] = [...s.root.querySelectorAll<HTMLElement>('[role="tree"]')];
    expect(agents.textContent).toContain("agent-b");
    expect(agents.textContent).not.toContain("agent-a");
    expect(spaces.textContent).toContain("Two");
    expect(spaces.textContent).not.toContain("One");
  });

  it.each<SidebarLayout>(["tree", "split"])(
    "keeps the agent and terminal icons (%s)",
    async (layout) => {
      // The open/active marks come from the shared row template and `paneMarks` (spaces.spec.ts).
      const s = await setup(layout);
      expect(s.item("agent-a").querySelector(".icon")?.getAttribute("aria-label")).toBe(
        "Agent claude",
      );
      expect(s.item("shell-a").querySelector(".icon")?.getAttribute("aria-label")).toBe("Terminal");
    },
  );
});
