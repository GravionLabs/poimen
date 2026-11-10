// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it } from "vitest";
import {
  EditorTabs,
  changesDir,
  changesTab,
  flowPane,
  flowTab,
  moveItem,
  parseEditor,
  spaceOfTab,
  spaceTab,
  tabKind,
  tabPaneId,
} from "./editor-tabs";
import { UI_STATE_KEY, UI_STORAGE } from "./ui-state";

describe("changes tab ids", () => {
  it("map a directory to its changes tab and back", () => {
    expect(changesTab("/src/repo")).toBe("changes:/src/repo");
    expect(changesDir(changesTab("/src/repo"))).toBe("/src/repo");
    expect(changesDir("w1:p2")).toBeNull();
    expect(tabKind(changesTab("/src/repo"))).toBe("changes");
    expect(tabPaneId(changesTab("/src/repo"))).toBeNull();
  });

  it("restore like the other kinds", () => {
    const id = changesTab("/src/repo");
    const stored = {
      groups: [{ id: 1, tabs: [{ paneId: id, pinned: true }], activeId: id }],
      layout: { kind: "group", id: 1 },
      focusedId: 1,
    };
    expect(parseEditor(stored)?.groups[0].activeId).toBe(id);
  });
});

describe("flow tab ids", () => {
  it("map a pane to its flow tab and back", () => {
    expect(flowPane(flowTab("w1:p2"))).toBe("w1:p2");
    expect(flowPane("w1:p2")).toBeNull();
    expect(tabKind(flowTab("w1:p2"))).toBe("flow");
    expect(tabPaneId(flowTab("w1:p2"))).toBe("w1:p2");
    expect(tabPaneId("w1:p2")).toBe("w1:p2");
    expect(tabPaneId("poimen:settings")).toBeNull();
  });
});

describe("space overview tab ids", () => {
  it("map a space to its overview tab and back", () => {
    expect(spaceOfTab(spaceTab("w1"))).toBe("w1");
    expect(spaceOfTab("w1:p2")).toBeNull();
    expect(tabKind(spaceTab("w1"))).toBe("space");
    expect(tabPaneId(spaceTab("w1"))).toBeNull();
  });

  it("opens one pinned tab per space and focuses it again", () => {
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: null }] });
    const tabs = TestBed.inject(EditorTabs);
    tabs.openSpace("w1");
    tabs.open("w1:p1");
    tabs.openSpace("w1");
    expect(tabs.tabs().map((t) => t.paneId)).toEqual([spaceTab("w1"), "w1:p1"]);
    expect(tabs.tabs()[0].pinned).toBe(true);
    expect(tabs.activeId()).toBe(spaceTab("w1"));
  });
});

describe("moveItem", () => {
  it("moves an item to a new index", () => {
    expect(moveItem(["a", "b", "c"], 0, 2)).toEqual(["b", "c", "a"]);
    expect(moveItem(["a", "b", "c"], 2, 0)).toEqual(["c", "a", "b"]);
  });

  it("ignores no-op and out-of-range moves", () => {
    const list = ["a", "b"];
    expect(moveItem(list, 1, 1)).toBe(list);
    expect(moveItem(list, 5, 0)).toBe(list);
    expect(moveItem(list, 0, -1)).toBe(list);
  });
});

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

describe("EditorTabs", () => {
  let tabs: EditorTabs;
  const ids = () => tabs.tabs().map((t) => t.paneId);

  beforeEach(() => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: null }] });
    tabs = TestBed.inject(EditorTabs);
  });

  it("replaces a tab in place, keeping position, pinned and active state", () => {
    tabs.open("a", true);
    tabs.open("b", true);
    tabs.open("c", true);
    tabs.activate("b");
    tabs.replace("b", "x");
    expect(ids()).toEqual(["a", "x", "c"]);
    expect(tabs.groups()[0].activeId).toBe("x");
    expect(tabs.groups()[0].tabs[1].pinned).toBe(true);
  });

  it("replace without the old tab does nothing, and closes it when the new one is open", () => {
    tabs.open("a", true);
    tabs.open("b", true);
    tabs.replace("zz", "x");
    expect(ids()).toEqual(["a", "b"]);
    tabs.replace("a", "b");
    expect(ids()).toEqual(["b"]);
  });

  it("closePane hands the tab to the successor and closes the flow tab", () => {
    tabs.open("a", true);
    tabs.openFlow("a");
    tabs.open("b", true);
    tabs.activate("a");
    tabs.closePane("a", "x");
    expect(ids()).toEqual(["x", "b"]);
    expect(tabs.activeId()).toBe("x");
  });

  it("closePane without a successor closes the tab and its flow tab", () => {
    tabs.open("a", true);
    tabs.openFlow("a");
    tabs.open("b", true);
    tabs.closePane("a");
    expect(ids()).toEqual(["b"]);
  });

  it("opens one pinned flow tab per pane and activates it when it is open", () => {
    tabs.open("a", true);
    tabs.openFlow("a");
    tabs.open("b", true);
    tabs.openFlow("b");
    tabs.openFlow("a");
    expect(ids()).toEqual(["a", flowTab("a"), "b", flowTab("b")]);
    expect(tabs.activeId()).toBe(flowTab("a"));
    expect(tabs.tabs().every((t) => t.pinned)).toBe(true);
  });

  it("opens one pinned changes tab per directory and keeps it when panes are pruned", () => {
    tabs.open("a", true);
    tabs.openChanges("/r");
    tabs.openChanges("/r");
    expect(ids()).toEqual(["a", changesTab("/r")]);
    expect(tabs.activeId()).toBe(changesTab("/r"));
    expect(tabs.prune(new Set(["a"]))).toEqual([]);
  });

  it("closes a flow tab with its pane when the pane is gone", () => {
    tabs.open("a", true);
    tabs.openFlow("a");
    tabs.openFlow("b");
    expect(tabs.prune(new Set(["a"]))).toEqual([flowTab("b")]);
    expect(ids()).toEqual(["a", flowTab("a")]);
  });

  it("cycles the active tab of the focused group and wraps around", () => {
    ["a", "b", "c"].forEach((id) => tabs.open(id, true));
    tabs.activate("a");
    tabs.cycle(1);
    expect(tabs.activeId()).toBe("b");
    tabs.cycle(-1);
    tabs.cycle(-1);
    expect(tabs.activeId()).toBe("c");
    tabs.cycle(1);
    expect(tabs.activeId()).toBe("a");
  });

  it("does not cycle with fewer than two tabs", () => {
    tabs.cycle(1);
    expect(tabs.activeId()).toBeNull();
    tabs.open("a");
    tabs.cycle(1);
    expect(tabs.activeId()).toBe("a");
  });

  it("opens a pane as an active preview tab", () => {
    tabs.open("p1");
    expect(tabs.tabs()).toEqual([{ paneId: "p1", pinned: false }]);
    expect(tabs.activeId()).toBe("p1");
  });

  it("replaces the preview tab in place with the next preview", () => {
    tabs.open("p1", true);
    tabs.open("p2");
    tabs.open("p3");
    expect(ids()).toEqual(["p1", "p3"]);
    expect(tabs.activeId()).toBe("p3");
  });

  it("keeps pinned tabs and appends a new preview after them", () => {
    tabs.open("p1", true);
    tabs.open("p2", true);
    tabs.open("p3");
    expect(ids()).toEqual(["p1", "p2", "p3"]);
  });

  it("only activates an already open pane (no duplicate tab)", () => {
    tabs.open("p1", true);
    tabs.open("p2", true);
    tabs.open("p1");
    expect(ids()).toEqual(["p1", "p2"]);
    expect(tabs.activeId()).toBe("p1");
  });

  it("pins a preview tab on a second open with pin (double click)", () => {
    tabs.open("p1");
    tabs.open("p1", true);
    expect(tabs.tabs()[0].pinned).toBe(true);
    tabs.open("p2");
    expect(ids()).toEqual(["p1", "p2"]);
  });

  it("closes the active tab and activates its right neighbor, else the left one", () => {
    ["a", "b", "c"].forEach((id) => tabs.open(id, true));
    tabs.activate("b");
    tabs.close("b");
    expect(tabs.activeId()).toBe("c");
    tabs.close("c");
    expect(tabs.activeId()).toBe("a");
    tabs.close("a");
    expect(tabs.activeId()).toBeNull();
  });

  it("keeps the active tab when another one closes", () => {
    tabs.open("a", true);
    tabs.open("b", true);
    tabs.close("a");
    expect(tabs.activeId()).toBe("b");
  });

  it("ignores closing or activating an unknown pane", () => {
    tabs.open("a", true);
    tabs.close("zz");
    tabs.activate("zz");
    expect(ids()).toEqual(["a"]);
    expect(tabs.activeId()).toBe("a");
  });

  it("reorders tabs", () => {
    ["a", "b", "c"].forEach((id) => tabs.open(id, true));
    tabs.move(2, 0);
    expect(ids()).toEqual(["c", "a", "b"]);
  });

  it("reports the real mode and forgets it when the tab closes", () => {
    tabs.open("a", true);
    expect(tabs.mode("a")).toBe("observe");
    tabs.report("a", "control", null);
    expect(tabs.mode("a")).toBe("control");
    tabs.report("a", "observe", "held by another client");
    expect(tabs.blocked("a")).toBe("held by another client");
    tabs.close("a");
    expect(tabs.mode("a")).toBe("observe");
    expect(tabs.blocked("a")).toBeNull();
  });

  it("wants input by default, read-only when autoControl is off, and keeps the user's choice", () => {
    tabs.open("a", true);
    expect(tabs.wanted("a", true)).toBe("control");
    expect(tabs.wanted("a", false)).toBe("observe");
    tabs.choose("a", "observe");
    expect(tabs.wanted("a", true)).toBe("observe");
    tabs.choose("a", "control");
    expect(tabs.wanted("a", false)).toBe("control");
    tabs.close("a");
    expect(tabs.wanted("a", true)).toBe("control");
  });

  it("closeAll empties the group", () => {
    tabs.open("a", true);
    tabs.closeAll();
    expect(tabs.tabs()).toEqual([]);
    expect(tabs.activeId()).toBeNull();
  });

  describe("groups", () => {
    const paneIds = (groupId: number) =>
      tabs
        .groups()
        .find((g) => g.id === groupId)!
        .tabs.map((t) => t.paneId);

    beforeEach(() => ["a", "b", "c"].forEach((id) => tabs.open(id, true)));

    it("splits a tab right into a new focused group", () => {
      tabs.split("b", "row");
      expect(tabs.groups().length).toBe(2);
      expect(paneIds(1)).toEqual(["a", "c"]);
      expect(paneIds(2)).toEqual(["b"]);
      expect(tabs.focusedId()).toBe(2);
      expect(tabs.activeId()).toBe("b");
      expect(tabs.layout()).toMatchObject({ kind: "split", dir: "row" });
    });

    it("splits down into a column", () => {
      tabs.split("c", "column");
      expect(tabs.layout()).toMatchObject({ kind: "split", dir: "column" });
    });

    it("does not split a group's only tab", () => {
      tabs.split("b", "row");
      tabs.split("b", "row");
      expect(tabs.groups().length).toBe(2);
    });

    it("keeps the pane's mode when it moves to a new group", () => {
      tabs.report("b", "control", null);
      tabs.split("b", "row");
      expect(tabs.mode("b")).toBe("control");
    });

    it("removes a group when its last tab closes and focuses a neighbor", () => {
      tabs.split("b", "row");
      tabs.close("b");
      expect(tabs.groups().length).toBe(1);
      expect(tabs.layout()).toEqual({ kind: "group", id: 1 });
      expect(tabs.focusedId()).toBe(1);
      expect(tabs.activeId()).toBe("c");
    });

    it("keeps an empty single group", () => {
      ["a", "b", "c"].forEach((id) => tabs.close(id));
      expect(tabs.groups().length).toBe(1);
      expect(tabs.tabs()).toEqual([]);
    });

    it("moves a tab into another group at an index and removes the emptied group", () => {
      tabs.split("b", "row");
      tabs.moveToGroup("b", 1, 0);
      expect(paneIds(1)).toEqual(["b", "a", "c"]);
      expect(tabs.groups().length).toBe(1);
      expect(tabs.focusedId()).toBe(1);
      expect(tabs.activeId()).toBe("b");
    });

    it("moves a tab out of a group that keeps other tabs", () => {
      tabs.split("b", "row");
      tabs.moveToGroup("a", 2);
      expect(paneIds(1)).toEqual(["c"]);
      expect(paneIds(2)).toEqual(["b", "a"]);
      expect(tabs.groups().find((g) => g.id === 1)!.activeId).toBe("c");
    });

    it("ignores moves to the same or an unknown group", () => {
      tabs.moveToGroup("a", 1);
      tabs.moveToGroup("a", 99);
      expect(paneIds(1)).toEqual(["a", "b", "c"]);
    });

    it("opens a pane that is open in another group by focusing that group", () => {
      tabs.split("b", "row");
      tabs.focusGroup(1);
      tabs.open("b");
      expect(tabs.focusedId()).toBe(2);
      expect(tabs.openIds().size).toBe(3);
    });

    it("lists the open panes of every group and reorders inside a group", () => {
      tabs.split("c", "row");
      expect([...tabs.openIds()].sort()).toEqual(["a", "b", "c"]);
      tabs.move(0, 1, 1);
      expect(paneIds(1)).toEqual(["b", "a"]);
    });

    it("updates split sizes", () => {
      tabs.split("b", "row");
      const layout = tabs.layout();
      if (layout.kind !== "split") throw new Error("expected a split");
      tabs.setSizes(layout.id, [3, 1]);
      expect(tabs.layout()).toMatchObject({ sizes: [0.75, 0.25] });
    });

    it("closeAll returns to a single empty group", () => {
      tabs.split("b", "row");
      tabs.closeAll();
      expect(tabs.groups().length).toBe(1);
      expect(tabs.layout()).toEqual({ kind: "group", id: 1 });
      expect(tabs.focusedId()).toBe(1);
    });
  });
});

describe("restoring the editor area", () => {
  const stored = {
    groups: [
      { id: 1, tabs: [{ paneId: "a", pinned: true }], activeId: "a" },
      {
        id: 3,
        tabs: [
          { paneId: "b", pinned: true },
          { paneId: "c", pinned: false },
        ],
        activeId: "c",
      },
    ],
    layout: {
      kind: "split",
      id: 2,
      dir: "row",
      children: [
        { kind: "group", id: 1 },
        { kind: "group", id: 3 },
      ],
      sizes: [0.3, 0.7],
    },
    focusedId: 3,
  };
  const data = new Map<string, string>();
  const open = (editor: unknown) => {
    data.set(UI_STATE_KEY, JSON.stringify({ editor }));
    const storage = {
      getItem: (k: string) => data.get(k) ?? null,
      setItem: (k: string, v: string) => void data.set(k, v),
    };
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [{ provide: UI_STORAGE, useValue: storage }] });
    return TestBed.inject(EditorTabs);
  };

  it("restores tabs, active tab, groups, sizes and focus", () => {
    const tabs = open(stored);
    expect(tabs.groups().map((g) => g.tabs.map((t) => t.paneId))).toEqual([["a"], ["b", "c"]]);
    expect(tabs.activeId()).toBe("c");
    expect(tabs.focusedId()).toBe(3);
    expect(tabs.layout()).toEqual(stored.layout);
  });

  it("keeps new group ids unique after a restore", () => {
    const tabs = open(stored);
    tabs.split("c", "column");
    const ids = tabs.groups().map((g) => g.id);
    expect(ids.slice(0, 2)).toEqual([1, 3]);
    expect(ids[2]).toBeGreaterThan(3);
  });

  it("starts blank when nothing or garbage is stored", () => {
    expect(open(undefined).tabs()).toEqual([]);
    expect(open({ groups: "x" }).tabs()).toEqual([]);
  });

  it("rejects inconsistent state", () => {
    expect(parseEditor({ ...stored, focusedId: 9 })).toBeNull();
    expect(parseEditor({ ...stored, layout: { kind: "group", id: 1 } })).toBeNull();
    const dup = structuredClone(stored);
    dup.groups[1].tabs[0].paneId = "a";
    expect(parseEditor(dup)).toBeNull();
    const badActive = structuredClone(stored);
    badActive.groups[0].activeId = "zzz";
    expect(parseEditor(badActive)).toBeNull();
  });

  it("prunes tabs whose pane is gone and reports them", () => {
    const tabs = open(stored);
    expect(tabs.prune(new Set(["a", "b"]))).toEqual(["c"]);
    expect(tabs.openIds()).toEqual(new Set(["a", "b"]));
    expect(tabs.prune(new Set(["a", "b"]))).toEqual([]);
  });

  it("drops a group whose panes are all gone", () => {
    const tabs = open(stored);
    tabs.prune(new Set(["a"]));
    expect(tabs.groups().map((g) => g.id)).toEqual([1]);
    expect(tabs.layout()).toEqual({ kind: "group", id: 1 });
  });

  it("saves changes so the next start restores them", () => {
    const tabs = open(undefined);
    tabs.open("x", true);
    tabs.open("y", true);
    tabs.split("y", "row");
    TestBed.tick();
    const next = open(JSON.parse(data.get(UI_STATE_KEY)!).editor);
    expect(next.groups().map((g) => g.tabs.map((t) => t.paneId))).toEqual([["x"], ["y"]]);
    expect(next.focusedId()).toBe(tabs.focusedId());
  });
});
