import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it } from "vitest";
import { EditorTabs, moveItem, splitKeyDirection } from "./editor-tabs";

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

describe("EditorTabs", () => {
  let tabs: EditorTabs;
  const ids = () => tabs.tabs().map((t) => t.paneId);

  beforeEach(() => {
    tabs = Injector.create({ providers: [EditorTabs] }).get(EditorTabs);
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

describe("splitKeyDirection", () => {
  const key = (k: string, extra: Partial<KeyboardEvent> = {}) => ({
    key: k,
    metaKey: false,
    ctrlKey: false,
    altKey: false,
    shiftKey: false,
    ...extra,
  });

  it("splits right with Cmd or Ctrl and backslash, down with Shift", () => {
    expect(splitKeyDirection(key("\\", { metaKey: true }))).toBe("row");
    expect(splitKeyDirection(key("\\", { ctrlKey: true }))).toBe("row");
    expect(splitKeyDirection(key("|", { ctrlKey: true, shiftKey: true }))).toBe("column");
  });

  it("ignores other keys, a missing modifier and Alt", () => {
    expect(splitKeyDirection(key("\\"))).toBeNull();
    expect(splitKeyDirection(key("b", { metaKey: true }))).toBeNull();
    expect(splitKeyDirection(key("\\", { metaKey: true, altKey: true }))).toBeNull();
  });
});
