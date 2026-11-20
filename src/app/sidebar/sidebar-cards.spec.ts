// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { SpaceNode, Terminal } from "../core/snapshot";
import { Attention } from "../core/attention";
import { PaneActions } from "../core/pane-actions";
import { SpaceActions } from "../core/space-actions";
import { SpaceRebuild, type RebuildMark } from "../core/space-rebuild";
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
  const spaces = signal<SpaceNode[]>([one, two]);
  const marks = signal<ReadonlyMap<string, RebuildMark>>(new Map());
  const rebuild = vi.fn(async () => undefined);
  const storage = new Map<string, string>(stored ? [[UI_STATE_KEY, stored]] : []);
  TestBed.configureTestingModule({
    providers: [
      {
        provide: SpacesStore,
        useValue: {
          spaces,
          pinnedSpaces: signal<string[]>([]),
          snapshot: signal({ spaces: [one.workspace, two.workspace] }),
          agentSections: signal(new Map()),
          agentChangedAt: signal(new Map()),
          disconnected: signal<string | null>(null),
          ensureBackend: vi.fn(),
        },
      },
      { provide: SpaceRebuild, useValue: { marks, rebuild } },
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
    spaces,
    marks,
    rebuild,
    store: TestBed.inject(SpacesStore) as unknown as {
      pinnedSpaces: ReturnType<typeof signal<string[]>>;
    },
    attention: TestBed.inject(Attention),
    spaceActions: TestBed.inject(SpaceActions),
    paneActions: TestBed.inject(PaneActions),
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
    expect(s.root.querySelector("hx-tree")).toBeNull();
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

  describe("states (#935)", () => {
    const statuses = (blocked: string[]) =>
      new Map(
        [...one.rows, ...two.rows].map((r) => [
          r.pane.terminal_id,
          blocked.includes(r.pane.terminal_id) ? ("blocked" as const) : ("idle" as const),
        ]),
      );

    it("rings the whole card of a pane that turned blocked while not viewed, until viewed", async () => {
      const s = await setup("cards");
      s.attention.observe(statuses([]), new Set());
      s.attention.observe(statuses(["w1:p1"]), new Set());
      await s.settle();
      const card = (i: number) => s.cards()[i].querySelector<HTMLElement>(".card")!;
      expect(card(0).classList.contains("attention")).toBe(true);
      expect(card(1).classList.contains("attention")).toBe(false);
      s.attention.observe(statuses(["w1:p1"]), new Set(["w1:p1"]));
      await s.settle();
      expect(card(0).classList.contains("attention")).toBe(false);
    });

    it("shows the unread count and the most urgent status in a collapsed header", async () => {
      const s = await setup("cards");
      s.spaces.set([
        {
          ...one,
          rows: [
            { ...one.rows[0], pane: { ...one.rows[0].pane, agent_status: "blocked" } },
            one.rows[1],
          ],
        },
        two,
      ]);
      s.attention.observe(statuses([]), new Set());
      s.attention.unread.set(new Set(["w1:p2"]));
      s.cards()[0].querySelector<HTMLButtonElement>(".fold")!.click();
      await s.settle();
      const head = s.cards()[0].querySelector<HTMLElement>(".head")!;
      expect(s.cards()[0].querySelector(".body")).toBeNull();
      expect(head.querySelector(".badge")?.getAttribute("aria-label")).toBe("1 unread");
      expect(head.querySelector(".dot")?.classList.contains("status-blocked")).toBe(true);
    });

    it("shows the silent mark on the terminal row and the header", async () => {
      const s = await setup("cards");
      const silentPane = {
        ...one.rows[0].pane,
        agent_silent_since: Date.now() - 5 * 60_000 - 30_000,
      };
      s.spaces.set([{ ...one, rows: [{ ...one.rows[0], pane: silentPane }, one.rows[1]] }, two]);
      await s.settle();
      const card = s.cards()[0];
      expect(card.querySelector(".head .silent")?.textContent).toBe("silent 5m");
      expect(card.querySelector(".row .silent")?.textContent).toBe("silent 5m");
      expect(s.cards()[1].querySelector(".silent")).toBeNull();
    });

    it("shows the pin mark in the header of a pinned space only", async () => {
      const s = await setup("cards");
      s.store.pinnedSpaces.set(["w2"]);
      await s.settle();
      expect(s.cards()[0].querySelector(".head .pin-mark")).toBeNull();
      expect(s.cards()[1].querySelector(".head .pin-mark")?.getAttribute("aria-label")).toBe(
        "Pinned",
      );
    });

    it("shows the rebuild marks and Closing… in the header of an empty space", async () => {
      const s = await setup("cards");
      s.spaces.set([{ ...one, rows: [] }, two]);
      const head = () => s.cards()[0].querySelector<HTMLElement>(".head")!;
      const mark = async (m: RebuildMark) => {
        s.marks.set(new Map([["w1", m]]));
        await s.settle();
        return head().textContent;
      };
      expect(await mark({ kind: "busy" })).toContain("Rebuilding…");
      expect(await mark({ kind: "missing", reason: "gone" })).toContain("Directory gone");
      expect(await mark({ kind: "failed", reason: "no" })).toContain("Not rebuilt");
      expect(await mark({ kind: "consent", reasons: ["agents start"] })).toContain("Rebuild");
      head().querySelector<HTMLButtonElement>(".rebuild")!.click();
      expect(s.rebuild).toHaveBeenCalledWith("w1");
      // A space with terminals shows no rebuild mark, whatever the mark says.
      s.marks.set(new Map([["w2", { kind: "busy" }]]));
      await s.settle();
      expect(s.cards()[1].querySelector(".head")?.textContent).not.toContain("Rebuilding");
    });

    it("renames a space inline: Enter saves, Escape cancels", async () => {
      const s = await setup("cards");
      const rename = vi.spyOn(s.spaceActions, "rename").mockResolvedValue(true);
      const edit = async () => {
        s.cards()[0]
          .querySelector<HTMLElement>(".head .name")!
          .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
        await s.settle();
        return s.cards()[0].querySelector<HTMLInputElement>(".head input.edit")!;
      };
      let input = await edit();
      expect(input.value).toBe("One");
      input.value = "Uno";
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      await s.settle();
      expect(rename).toHaveBeenCalledWith("w1", "Uno");
      expect(s.cards()[0].querySelector("input.edit")).toBeNull();

      input = await edit();
      input.value = "Zwei";
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      await s.settle();
      expect(rename).toHaveBeenCalledTimes(1);
      expect(s.cards()[0].querySelector("input.edit")).toBeNull();
      expect(s.cards()[0].querySelector(".head .name")?.textContent).toBe("One");
    });

    it("renames a terminal inline and keeps editing when the rename fails", async () => {
      const s = await setup("cards");
      const rename = vi
        .spyOn(s.paneActions, "rename")
        .mockResolvedValueOnce(false)
        .mockResolvedValueOnce(true);
      s.cards()[0]
        .querySelector<HTMLElement>(".row .name")!
        .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
      await s.settle();
      const input = s.cards()[0].querySelector<HTMLInputElement>(".row input.edit")!;
      expect(input.value).toBe("agent-a");
      input.value = "builder";
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      await s.settle();
      expect(s.cards()[0].querySelector(".row input.edit")).not.toBeNull();
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
      await s.settle();
      expect(rename).toHaveBeenNthCalledWith(1, "w1:p1", "builder");
      expect(s.cards()[0].querySelector(".row input.edit")).toBeNull();
      expect(s.selected).toEqual([]);
    });
  });
});

describe("Sidebar cards keyboard and ARIA (#937)", () => {
  const press = async (
    s: Awaited<ReturnType<typeof setup>>,
    el: Element,
    key: string,
    init: KeyboardEventInit = {},
  ) => {
    el.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...init }));
    await s.settle();
  };
  const nav = (s: Awaited<ReturnType<typeof setup>>, key: string) =>
    s.root.querySelector<HTMLElement>(`[data-nav="${key}"]`)!;

  it("is a tree of treeitems with level, expanded and selected", async () => {
    const s = await setup("cards");
    expect(s.root.querySelector('.cards[role="tree"]')).not.toBeNull();
    expect(s.root.querySelector('[role="listitem"]')).toBeNull();
    const head = nav(s, "w1");
    expect(head.getAttribute("role")).toBe("treeitem");
    expect(head.getAttribute("aria-level")).toBe("1");
    expect(head.getAttribute("aria-expanded")).toBe("true");
    expect(head.hasAttribute("aria-selected")).toBe(true);
    expect(nav(s, "w1:p1").getAttribute("aria-level")).toBe("2");
    expect(nav(s, "w1:p2").getAttribute("aria-level")).toBe("3");
    expect(nav(s, "w1:p1").getAttribute("aria-selected")).toBe("false");
  });

  it("has one tab stop; the header buttons are tabbable from the focused header", async () => {
    const s = await setup("cards");
    const stops = [...s.root.querySelectorAll<HTMLElement>(".cards [tabindex='0']")];
    expect(stops.map((e) => e.dataset["nav"] ?? e.getAttribute("aria-label"))).toEqual([
      "w1",
      "New terminal in One",
      "Configure space One",
      "Close space One",
      "More actions for One",
    ]);
    nav(s, "w1:p1").dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    await s.settle();
    const after = [...s.root.querySelectorAll<HTMLElement>(".cards [tabindex='0']")];
    expect(after.map((e) => e.dataset["nav"])).toEqual(["w1:p1"]);
  });

  it("moves with the arrow keys over headers and visible rows in display order", async () => {
    const s = await setup("cards");
    await press(s, nav(s, "w1"), "ArrowDown");
    expect(nav(s, "w1:p1").getAttribute("tabindex")).toBe("0");
    expect(document.activeElement).toBe(nav(s, "w1:p1"));
    await press(s, nav(s, "w1:p1"), "ArrowDown");
    await press(s, nav(s, "w1:p2"), "ArrowDown");
    expect(nav(s, "w2").getAttribute("tabindex")).toBe("0");
    await press(s, nav(s, "w2"), "End");
    expect(nav(s, "w2:p1").getAttribute("tabindex")).toBe("0");
    await press(s, nav(s, "w2:p1"), "Home");
    expect(nav(s, "w1").getAttribute("tabindex")).toBe("0");
  });

  it("collapses and expands with Left and Right, Enter toggles a header", async () => {
    const s = await setup("cards");
    await press(s, nav(s, "w1"), "ArrowLeft");
    expect(s.ui.collapsedCards()).toEqual(["w1"]);
    expect(s.root.querySelector('[data-nav="w1:p1"]')).toBeNull();
    expect(nav(s, "w1").getAttribute("aria-expanded")).toBe("false");
    await press(s, nav(s, "w1"), "ArrowRight");
    expect(s.ui.collapsedCards()).toEqual([]);
    await press(s, nav(s, "w1"), "Enter");
    expect(s.ui.collapsedCards()).toEqual(["w1"]);
    await press(s, nav(s, "w1"), "Enter");
    await press(s, nav(s, "w1"), "ArrowRight");
    expect(nav(s, "w1:p1").getAttribute("tabindex")).toBe("0");
    await press(s, nav(s, "w1:p1"), "ArrowLeft");
    expect(nav(s, "w1").getAttribute("tabindex")).toBe("0");
  });

  it("opens a terminal with Enter, once", async () => {
    const s = await setup("cards");
    await press(s, nav(s, "w1:p1"), "Enter");
    expect(s.selected).toEqual(["w1:p1"]);
  });

  it("keeps Space on a button inside a treeitem, and the stop on a card whose fold was clicked", async () => {
    const s = await setup("cards");
    const space = new KeyboardEvent("keydown", { key: " ", bubbles: true, cancelable: true });
    s.root.querySelector('[aria-label="Close space One"]')!.dispatchEvent(space);
    expect(space.defaultPrevented).toBe(false);
    nav(s, "w2:p1").dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    s.root
      .querySelectorAll<HTMLElement>(".fold")[1]
      .dispatchEvent(new FocusEvent("focusin", { bubbles: true }));
    await s.settle();
    expect(nav(s, "w2").getAttribute("tabindex")).toBe("0");
  });

  it("leaves keys with a modifier, buttons and fields to the app shortcuts", async () => {
    const s = await setup("cards");
    const head = nav(s, "w1");
    const alt = new KeyboardEvent("keydown", { key: "ArrowDown", altKey: true, bubbles: true });
    head.dispatchEvent(alt);
    expect(alt.defaultPrevented).toBe(false);
    const button = s.root.querySelector<HTMLElement>('[aria-label="Close space One"]')!;
    const enter = new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true });
    button.dispatchEvent(enter);
    expect(enter.defaultPrevented).toBe(false);
    expect(s.ui.collapsedCards()).toEqual([]);
  });
});
