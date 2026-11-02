import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it } from "vitest";
import { EditorTabs, moveItem } from "./editor-tabs";

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

  it("defaults to read-only and forgets the mode when the tab closes", () => {
    tabs.open("a", true);
    expect(tabs.mode("a")).toBe("observe");
    tabs.setMode("a", "control");
    expect(tabs.mode("a")).toBe("control");
    tabs.close("a");
    expect(tabs.mode("a")).toBe("observe");
  });

  it("closeAll empties the group", () => {
    tabs.open("a", true);
    tabs.closeAll();
    expect(tabs.tabs()).toEqual([]);
    expect(tabs.activeId()).toBeNull();
  });
});
