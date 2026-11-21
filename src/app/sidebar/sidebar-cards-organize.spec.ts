// @vitest-environment jsdom
import { readFileSync } from "node:fs";
import { signal } from "@angular/core";
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, describe, expect, it, vi } from "vitest";
import type { SpaceNode } from "../core/snapshot";
import { SpacesStore } from "../core/spaces-store";
import { DEFAULT_SETTINGS, SettingsStore, type SidebarSort } from "../core/settings";
import { SpaceActions } from "../core/space-actions";
import { UI_STATE_KEY, UI_STORAGE } from "../core/ui-state";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { Sidebar } from "./sidebar";
import { SpaceCard } from "./space-card";

const node = (id: string, n: number, parent?: string): SpaceNode => ({
  workspace: { space_id: id, label: `Space ${id}`, number: n, agent_status: "idle" },
  parent,
  rows: [1, 2].map((t) => ({
    pane: {
      terminal_id: `${id}:p${t}`,
      tab_id: `${id}:t${t}`,
      space_id: id,
      agent_status: "idle" as const,
    },
    tab: `tab ${id}.${t}`,
    tabId: `${id}:t${t}`,
    name: `shell ${id}.${t}`,
    worktree: false,
  })),
});

async function setup(sort: SidebarSort = "manual", nodes = [node("w1", 1), node("w2", 2)]) {
  TestBed.resetTestingModule();
  const stored = JSON.stringify({ collapsedCards: ["w1"] });
  TestBed.configureTestingModule({
    providers: [
      {
        provide: SettingsStore,
        useValue: {
          settings: signal({ ...DEFAULT_SETTINGS, sidebarSort: sort }),
          writeError: signal(null),
        },
      },
      {
        provide: SpacesStore,
        useValue: {
          spaces: signal(nodes),
          pinnedSpaces: signal<string[]>([]),
          disconnected: signal<string | null>(null),
        },
      },
      {
        provide: UI_STORAGE,
        useValue: {
          getItem: (k: string) => (k === UI_STATE_KEY ? stored : null),
          setItem: () => {},
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
  const actions = TestBed.inject(SpaceActions);
  const reorder = vi.spyOn(actions, "reorder").mockImplementation(() => {});
  const reorderTabs = vi.spyOn(actions, "reorderTabs").mockImplementation(() => {});
  const fixture = TestBed.createComponent(Sidebar);
  const root = fixture.nativeElement as HTMLElement;
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      fixture.detectChanges();
      await fixture.whenStable();
    }
  };
  await settle();
  const card = (id: string) =>
    [...root.querySelectorAll<HTMLElement>("section.card")].find(
      (e) => e.getAttribute("aria-label") === `Space ${id}`,
    )!;
  const fire = async (el: Element, type: string, clientY = 0) => {
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clientY", { value: clientY });
    el.dispatchEvent(event);
    await settle();
    return event;
  };
  const filter = async (text: string) => {
    const input = root.querySelector<HTMLInputElement>("input.filter")!;
    input.value = text;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
  };
  return { root, card, fire, filter, reorder, reorderTabs };
}

describe("Sidebar organization in the cards layout (#936)", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("marks the card of a linked worktree as a member of its main checkout", async () => {
    const { card } = await setup("manual", [node("w1", 1), node("w2", 2, "w1")]);
    expect(card("w1").classList).not.toContain("member");
    expect(card("w2").classList).toContain("member");
  });

  it("drags a card by its header and reorders on drop", async () => {
    const { card, fire, reorder } = await setup();
    const head = card("w1").querySelector("header")!;
    expect(head.getAttribute("draggable")).toBe("true");
    await fire(head, "dragstart");
    expect(card("w1").classList).toContain("dragging");
    await fire(card("w2"), "dragover", 5);
    expect(card("w2").classList).toContain("drop-after");
    await fire(card("w2"), "drop");
    expect(reorder).toHaveBeenCalledExactlyOnceWith("w1", "w2", true);
    expect(card("w2").classList).not.toContain("drop-after");
  });

  it("reorders tabs inside a card by their rows", async () => {
    const { card, fire, reorderTabs } = await setup();
    const rows = card("w2").querySelectorAll(".row");
    expect(rows[0].getAttribute("draggable")).toBe("true");
    await fire(rows[0], "dragstart");
    await fire(rows[1], "dragover", 5);
    expect(rows[1].classList).toContain("drop-after");
    await fire(rows[1], "drop");
    expect(reorderTabs).toHaveBeenCalledExactlyOnceWith("w2", "w2:t1", "w2:t2", true);
  });

  it("turns drag and drop off outside manual and while a filter is active", async () => {
    const off = await setup("name");
    expect(off.card("w2").querySelector("header")!.getAttribute("draggable")).toBeNull();
    const { card, fire, filter, reorder, reorderTabs } = await setup();
    await filter("shell");
    expect(card("w2").querySelector("header")!.getAttribute("draggable")).toBeNull();
    expect(card("w2").querySelector(".row")!.getAttribute("draggable")).toBeNull();
    const start = await fire(card("w2").querySelector("header")!, "dragstart");
    expect(start.defaultPrevented).toBe(true);
    expect(reorder).not.toHaveBeenCalled();
    expect(reorderTabs).not.toHaveBeenCalled();
  });

  it("hides cards without a match and expands a matching collapsed card, keeping the state", async () => {
    const { root, card, filter } = await setup();
    expect(card("w1").classList).toContain("collapsed");
    await filter("shell w1.2");
    expect(root.querySelectorAll("section.card")).toHaveLength(1);
    expect(card("w1").classList).not.toContain("collapsed");
    await filter("");
    expect(card("w1").classList).toContain("collapsed");
    expect(card("w2")).toBeDefined();
  });
});
