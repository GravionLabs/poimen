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
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";
import { SplitPaneStub } from "../ui/helix-candidates/split-pane/split-pane.testing";
import { Sidebar } from "./sidebar";

const node = (id: string, n: number): SpaceNode => ({
  workspace: { space_id: id, label: `Space ${id}`, number: n, agent_status: "idle" },
  rows: [1, 2].map((t) => ({
    pane: {
      terminal_id: `${id}:p${t}`,
      tab_id: `${id}:t${t}`,
      space_id: id,
      agent_status: "idle" as const,
    },
    tab: "main",
    name: `shell ${id}.${t}`,
    worktree: false,
  })),
});

async function setup(
  sort: SidebarSort = "manual",
  nodes: SpaceNode[] = [node("w1", 1), node("w2", 2), node("w3", 3)],
) {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      {
        provide: SettingsStore,
        useValue: {
          settings: signal({ ...DEFAULT_SETTINGS, sidebarSort: sort }),
          writeError: signal(null),
          setSidebarSort: vi.fn(async () => {}),
        },
      },
      {
        provide: SpacesStore,
        useValue: {
          spaces: signal(nodes),
          disconnected: signal<string | null>(null),
        },
      },
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
  const reorder = vi.spyOn(TestBed.inject(SpaceActions), "reorder").mockImplementation(() => {});
  const reorderTabs = vi
    .spyOn(TestBed.inject(SpaceActions), "reorderTabs")
    .mockImplementation(() => {});
  const fixture = TestBed.createComponent(Sidebar);
  const settle = async () => {
    for (let i = 0; i < 3; i++) {
      fixture.detectChanges();
      await fixture.whenStable();
    }
  };
  await settle();
  const root = fixture.nativeElement as HTMLElement;
  const rowOf = (id: string) =>
    [...root.querySelectorAll<HTMLElement>(".space")].find((e) =>
      e.textContent?.includes(`Space ${id}`),
    )!;
  const fire = async (el: HTMLElement, type: string, clientY = 0) => {
    const event = new Event(type, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clientY", { value: clientY });
    el.dispatchEvent(event);
    await settle();
    return event;
  };
  const sortNow = vi.spyOn(TestBed.inject(SpaceActions), "sortNow").mockImplementation(() => {});
  return { fixture, root, sortNow, reorder, reorderTabs, rowOf, fire, settle };
}

describe("Sidebar space drag and drop", () => {
  beforeAll(() => {
    getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  });

  it("makes space rows draggable and shows a marker at the target", async () => {
    const { rowOf, fire } = await setup();
    expect(rowOf("w1").getAttribute("draggable")).toBe("true");
    await fire(rowOf("w1"), "dragstart");
    expect(rowOf("w1").classList).toContain("dragging");
    await fire(rowOf("w3"), "dragover", 0);
    expect(rowOf("w3").classList).toContain("drop-before");
    await fire(rowOf("w3"), "dragover", 5);
    expect(rowOf("w3").classList).toContain("drop-after");
  });

  it("reorders on a drop and clears the marker", async () => {
    const { rowOf, fire, reorder } = await setup();
    await fire(rowOf("w1"), "dragstart");
    await fire(rowOf("w3"), "dragover", 5);
    await fire(rowOf("w3"), "drop");
    expect(reorder).toHaveBeenCalledExactlyOnceWith("w1", "w3", true);
    expect(rowOf("w3").classList).not.toContain("drop-after");
    expect(rowOf("w1").classList).not.toContain("dragging");
  });

  it("changes nothing when the drag ends outside the list or over itself", async () => {
    const { rowOf, fire, reorder } = await setup();
    await fire(rowOf("w1"), "dragstart");
    await fire(rowOf("w1"), "dragover");
    await fire(rowOf("w1"), "drop");
    await fire(rowOf("w2"), "dragstart");
    await fire(rowOf("w2"), "dragend");
    expect(reorder).not.toHaveBeenCalled();
    expect(rowOf("w2").classList).not.toContain("dragging");
  });

  it("does not start a drag while a filter is active", async () => {
    const { root, rowOf, fire, reorder, settle } = await setup();
    const input = root.querySelector<HTMLInputElement>("input.filter")!;
    input.value = "space";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    expect(rowOf("w1").getAttribute("draggable")).toBeNull();
    const start = await fire(rowOf("w1"), "dragstart");
    expect(start.defaultPrevented).toBe(true);
    await fire(rowOf("w2"), "dragover", 5);
    await fire(rowOf("w2"), "drop");
    expect(reorder).not.toHaveBeenCalled();
  });
});

describe("Sidebar sort", () => {
  it("turns drag and drop of spaces off outside manual", async () => {
    const { rowOf, fire, reorder } = await setup("name");
    expect(rowOf("w1").getAttribute("draggable")).toBeNull();
    const start = await fire(rowOf("w1"), "dragstart");
    expect(start.defaultPrevented).toBe(true);
    await fire(rowOf("w2"), "drop");
    expect(reorder).not.toHaveBeenCalled();
  });

  it("opens the sort menu with the active mode checked", async () => {
    const { root, fire } = await setup("name");
    await fire(root.querySelector<HTMLElement>(".sort-toggle")!, "click");
    const items = [...root.querySelectorAll<HTMLElement>(".sort-menu [role^=menuitem]")];
    expect(items.map((e) => e.textContent?.trim())).toEqual([
      "Manual",
      "✓ By name",
      "Sort alphabetically now",
    ]);
    expect(items.map((e) => e.getAttribute("aria-checked"))).toEqual(["false", "true", null]);
  });

  it("sorts alphabetically now from the menu and closes it", async () => {
    const { root, fire, sortNow } = await setup();
    await fire(root.querySelector<HTMLElement>(".sort-toggle")!, "click");
    await fire(root.querySelectorAll<HTMLElement>(".sort-menu .menu-item")[2], "click");
    expect(sortNow).toHaveBeenCalledOnce();
    expect(root.querySelector(".sort-menu")).toBeNull();
  });

  it("writes the chosen mode", async () => {
    const { root, fire } = await setup();
    await fire(root.querySelector<HTMLElement>(".sort-toggle")!, "click");
    await fire(root.querySelectorAll<HTMLElement>(".sort-menu .menu-item")[1], "click");
    expect(TestBed.inject(SettingsStore).setSidebarSort).toHaveBeenCalledWith("name");
  });
});

describe("Sidebar tab drag and drop", () => {
  const tabRow = (root: HTMLElement, id: string, t: number) =>
    [...root.querySelectorAll<HTMLElement>(".row")].find((e) =>
      e.textContent?.includes(`shell ${id}.${t}`),
    )!;

  it("shows a marker and reorders inside the same space", async () => {
    const { root, fire, reorderTabs } = await setup();
    expect(tabRow(root, "w1", 1).getAttribute("draggable")).toBe("true");
    await fire(tabRow(root, "w1", 1), "dragstart");
    await fire(tabRow(root, "w1", 2), "dragover", 5);
    expect(tabRow(root, "w1", 2).classList).toContain("drop-after");
    await fire(tabRow(root, "w1", 2), "drop");
    expect(reorderTabs).toHaveBeenCalledExactlyOnceWith("w1", "w1:t1", "w1:t2", true);
    expect(tabRow(root, "w1", 2).classList).not.toContain("drop-after");
  });

  it("refuses a drop on a tab of another space: no marker, nothing changes", async () => {
    const { root, fire, reorderTabs } = await setup();
    await fire(tabRow(root, "w1", 1), "dragstart");
    const over = await fire(tabRow(root, "w2", 1), "dragover", 5);
    expect(over.defaultPrevented).toBe(false);
    expect(tabRow(root, "w2", 1).classList).not.toContain("drop-after");
    await fire(tabRow(root, "w2", 1), "drop");
    expect(reorderTabs).not.toHaveBeenCalled();
  });

  it("clears the marker when the drag moves over a refused row", async () => {
    const { root, fire } = await setup();
    await fire(tabRow(root, "w1", 1), "dragstart");
    await fire(tabRow(root, "w1", 2), "dragover", 5);
    expect(tabRow(root, "w1", 2).classList).toContain("drop-after");
    await fire(tabRow(root, "w2", 1), "dragover", 5);
    expect(tabRow(root, "w1", 2).classList).not.toContain("drop-after");
  });

  it("does not start a drag while a filter is active", async () => {
    const { root, fire, reorderTabs, settle } = await setup();
    const input = root.querySelector<HTMLInputElement>("input.filter")!;
    input.value = "shell";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    expect(tabRow(root, "w1", 1).getAttribute("draggable")).toBeNull();
    const start = await fire(tabRow(root, "w1", 1), "dragstart");
    expect(start.defaultPrevented).toBe(true);
    await fire(tabRow(root, "w1", 2), "drop");
    expect(reorderTabs).not.toHaveBeenCalled();
  });

  describe("with worktrees below their space", () => {
    const grouped = () => [
      node("w1", 1),
      { ...node("w2", 2), parent: "w1" },
      { ...node("w3", 3), parent: "w1" },
      node("w4", 4),
    ];

    it("indents a member row and not its head", async () => {
      const { rowOf } = await setup("manual", grouped());
      expect(rowOf("w2").classList).toContain("member");
      expect(rowOf("w1").classList).not.toContain("member");
    });

    it("lets a member be dropped on a sibling only", async () => {
      const { rowOf, fire, reorder } = await setup("manual", grouped());
      await fire(rowOf("w2"), "dragstart");
      await fire(rowOf("w4"), "dragover", 5);
      expect(rowOf("w4").classList).not.toContain("drop-after");
      await fire(rowOf("w4"), "drop");
      expect(reorder).not.toHaveBeenCalled();
      await fire(rowOf("w2"), "dragstart");
      await fire(rowOf("w3"), "dragover", 5);
      await fire(rowOf("w3"), "drop");
      expect(reorder).toHaveBeenCalledExactlyOnceWith("w2", "w3", true);
    });

    it("refuses a head dropped on a member", async () => {
      const { rowOf, fire, reorder } = await setup("manual", grouped());
      await fire(rowOf("w4"), "dragstart");
      await fire(rowOf("w2"), "dragover", 5);
      await fire(rowOf("w2"), "drop");
      expect(reorder).not.toHaveBeenCalled();
    });
  });
});
