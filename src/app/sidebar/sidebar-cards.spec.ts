// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { SpaceNode, Terminal } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { DEFAULT_SETTINGS, SettingsStore, type SidebarLayout } from "../core/settings";
import { UI_STATE_KEY, UI_STORAGE, UiState } from "../core/ui-state";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { Sidebar, type SidebarMenuAction } from "./sidebar";
import { SpaceCard } from "./space-card";

const mk = (id: string, label: string, panes: [string, string, boolean, string?][]): SpaceNode => ({
  workspace: { space_id: id, label, number: 1, agent_status: "idle" },
  rows: panes.map(([pid, name, agent, parent]) => ({
    pane: {
      terminal_id: pid,
      tab_id: `${id}:${name}`,
      space_id: id,
      agent_status: "idle",
    } as Terminal,
    tab: name,
    tabId: `${id}:${name}`,
    tabParent: parent ? `${id}:${parent}` : undefined,
    name,
    agent: agent ? "claude" : undefined,
    worktree: false,
  })),
});

const one = mk("w1", "One", [
  ["w1:p1", "agent-a", true],
  ["w1:p2", "shell-a", false, "agent-a"],
]);
const two = mk("w2", "Two", [["w2:p1", "shell-b", false]]);

async function setup(layout: SidebarLayout, stored?: string) {
  TestBed.resetTestingModule();
  const setSidebarLayout = vi.fn(async (layout: SidebarLayout) => void layout);
  const settings = signal({ ...DEFAULT_SETTINGS, sidebarLayout: layout });
  const storage = new Map<string, string>(stored ? [[UI_STATE_KEY, stored]] : []);
  TestBed.configureTestingModule({
    providers: [
      {
        provide: SpacesStore,
        useValue: {
          spaces: signal([one, two]),
          snapshot: signal({ spaces: [one.workspace, two.workspace] }),
          agentSections: signal(new Map()),
          agentChangedAt: signal(new Map()),
          disconnected: signal<string | null>(null),
          ensureBackend: vi.fn(),
        },
      },
      {
        provide: SettingsStore,
        useValue: { settings, writeError: signal(null), setSidebarLayout },
      },
      {
        provide: UI_STORAGE,
        useValue: {
          getItem: (k: string) => storage.get(k) ?? null,
          setItem: (k: string, v: string) => void storage.set(k, v),
        },
      },
    ],
  });
  const inline = (file: string) => ({
    templateUrl: undefined,
    styleUrl: undefined,
    template: readFileSync(`src/app/sidebar/${file}`, "utf8"),
  });
  TestBed.overrideComponent(Sidebar, { set: inline("sidebar.html") });
  TestBed.overrideComponent(Sidebar, {
    remove: { imports: [SplitPane] },
    add: { imports: [SplitPaneStub] },
  });
  TestBed.overrideComponent(SpaceCard, { set: inline("space-card.html") });
  const fixture = TestBed.createComponent(Sidebar);
  const selected: string[] = [];
  const actions: SidebarMenuAction[] = [];
  const configured: string[] = [];
  fixture.componentInstance.selected.subscribe((p) => selected.push(p.terminal_id));
  fixture.componentInstance.menuAction.subscribe((a) => actions.push(a));
  fixture.componentInstance.configOpened.subscribe((id) => configured.push(id));
  const root = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      fixture.detectChanges();
      await fixture.whenStable();
    }
  };
  await settle();
  return {
    fixture,
    root,
    settle,
    selected,
    actions,
    configured,
    storage,
    ui: TestBed.inject(UiState),
    setSidebarLayout,
    cards: () => [...root.querySelectorAll<HTMLElement>("app-space-card")],
  };
}

describe("Sidebar in the cards layout (#934)", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("names the next layout on the toggle: tree, split, cards, tree", async () => {
    for (const [layout, next] of [
      ["tree", "split"],
      ["split", "cards"],
      ["cards", "tree"],
    ] as const) {
      const s = await setup(layout);
      const toggle = s.root.querySelector<HTMLButtonElement>(".layout-toggle")!;
      expect(toggle.getAttribute("aria-label")).toBe(`Show ${next} layout`);
      toggle.click();
      expect(s.setSidebarLayout).toHaveBeenCalledWith(next);
    }
  });

  it("renders one card per space and no tree", async () => {
    const s = await setup("cards");
    expect(s.cards()).toHaveLength(2);
    expect(s.root.querySelector('[role="tree"]')).toBeNull();
    expect(s.cards()[0].querySelector(".head")?.textContent).toContain("One");
    expect(s.cards()[1].querySelector(".head")?.textContent).toContain("Two");
  });

  it("shows new terminal, configure, close and more in the header and acts on them", async () => {
    const s = await setup("cards");
    const head = s.cards()[0].querySelector<HTMLElement>(".head")!;
    const label = (b: Element) => b.getAttribute("aria-label");
    expect([...head.querySelectorAll("button")].map(label)).toEqual([
      "Collapse One",
      "New terminal in One",
      "Configure space One",
      "Close space One",
      "More actions for One",
    ]);
    head.querySelector<HTMLButtonElement>('[aria-label="New terminal in One"]')!.click();
    head.querySelector<HTMLButtonElement>('[aria-label="Configure space One"]')!.click();
    head.querySelector<HTMLButtonElement>('[aria-label="Close space One"]')!.click();
    expect(s.actions).toEqual([
      { kind: "workspace", id: "w1", action: "new-terminal" },
      { kind: "workspace", id: "w1", action: "close-space" },
    ]);
    expect(s.configured).toEqual(["w1"]);
  });

  it("opens the context menu of the tree row from the more button", async () => {
    const s = await setup("cards");
    s.cards()[0].querySelector<HTMLButtonElement>(".more")!.click();
    await s.settle();
    const items = [...s.root.querySelectorAll('[role="menuitem"]')].map((e) =>
      e.textContent?.trim(),
    );
    expect(items).toContain("New terminal...");
    expect(items).toContain("Rename space...");
    expect(items).toContain("Close space");
  });

  it("lists the tabs and terminals, a child tab indented below its parent, and opens a click", async () => {
    const s = await setup("cards");
    const rows = [...s.cards()[0].querySelectorAll<HTMLElement>(".row")];
    expect(rows.map((r) => r.textContent)).toEqual([
      expect.stringContaining("agent-a"),
      expect.stringContaining("shell-a"),
    ]);
    expect(rows[1].style.paddingLeft).toBe("18px");
    expect(rows[0].style.paddingLeft).toBe("6px");
    rows[1].click();
    expect(s.selected).toEqual(["w1:p2"]);
  });

  it("collapses a card to its header, persists it and prunes closed spaces", async () => {
    const s = await setup("cards");
    s.cards()[0].querySelector<HTMLButtonElement>(".fold")!.click();
    await s.settle();
    expect(s.cards()[0].querySelector(".body")).toBeNull();
    expect(s.cards()[0].querySelector(".fold")!.getAttribute("aria-expanded")).toBe("false");
    expect(s.cards()[1].querySelector(".body")).not.toBeNull();
    expect(JSON.parse(s.storage.get(UI_STATE_KEY)!).collapsedCards).toEqual(["w1"]);

    const again = await setup("cards", s.storage.get(UI_STATE_KEY));
    expect(again.cards()[0].querySelector(".body")).toBeNull();

    again.ui.pruneCollapsedCards(["w2"]);
    expect(again.ui.collapsedCards()).toEqual([]);
    expect(JSON.parse(again.storage.get(UI_STATE_KEY)!).collapsedCards).toBeUndefined();
  });

  it("keeps the tree layout as before", async () => {
    const s = await setup("tree");
    expect(s.cards()).toHaveLength(0);
    expect(s.root.querySelector('[role="tree"]')).not.toBeNull();
  });
});
