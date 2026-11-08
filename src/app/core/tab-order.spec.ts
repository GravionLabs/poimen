import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { buildTree, orderTabs, type Snapshot } from "./herdr";
import { HerdrStore } from "./herdr-store";
import { SpaceActions } from "./space-actions";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";
import { UI_STATE_KEY, UI_STORAGE, UiState, parseLayout, type UiStorage } from "./ui-state";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const space = (id: string, number: number) => ({
  space_id: id,
  label: id,
  number,
  agent_status: "idle" as const,
});
const tab = (id: string, spaceId: string, number: number) => ({
  tab_id: id,
  space_id: spaceId,
  number,
  label: id,
  agent_status: "idle" as const,
});
const pane = (id: string, tabId: string, spaceId: string) => ({
  terminal_id: id,
  tab_id: tabId,
  space_id: spaceId,
  agent_status: "idle" as const,
});
const snap = (tabIds: string[]): Snapshot => ({
  spaces: [space("w1", 1), space("w2", 2)],
  tabs: [...tabIds.map((t, i) => tab(t, "w1", i + 1)), tab("x1", "w2", 1), tab("x2", "w2", 2)],
  terminals: [
    ...tabIds.map((t) => pane(`p-${t}`, t, "w1")),
    pane("p-x1", "x1", "w2"),
    pane("p-x2", "x2", "w2"),
  ],
});
const ids = (list: { tab_id: string }[]) => list.map((t) => t.tab_id);

describe("orderTabs", () => {
  const tabs = [tab("a", "w1", 1), tab("b", "w1", 2), tab("c", "w1", 3), tab("d", "w1", 4)];

  it("uses the number order without a stored order", () => {
    expect(ids(orderTabs(tabs, []))).toEqual(["a", "b", "c", "d"]);
  });

  it("puts the stored order first and appends new tabs by number", () => {
    expect(ids(orderTabs(tabs, ["c", "a"]))).toEqual(["c", "a", "b", "d"]);
  });

  it("skips closed tabs of the stored order", () => {
    expect(ids(orderTabs(tabs, ["z", "d", "b"]))).toEqual(["d", "b", "a", "c"]);
  });

  it("orders each space on its own in the tree", () => {
    const nodes = buildTree(snap(["a", "b", "c"]), [], { w1: ["c", "a", "b"] });
    expect(nodes.map((n) => ids(n.tabs.map((t) => t.tab)))).toEqual([
      ["c", "a", "b"],
      ["x1", "x2"],
    ]);
  });
});

describe("tab order in the app", () => {
  let data: Map<string, string>;
  let store: HerdrStore;
  let actions: SpaceActions;
  let ui: UiState;
  const start = () => {
    const storage: UiStorage = {
      getItem: (k) => data.get(k) ?? null,
      setItem: (k, v) => void data.set(k, v),
    };
    const injector = Injector.create({
      providers: [
        TauriBridge,
        HerdrStore,
        { provide: UI_STORAGE, useValue: storage },
        UiState,
        SpaceActions,
      ],
    });
    store = injector.get(HerdrStore);
    actions = injector.get(SpaceActions);
    ui = injector.get(UiState);
  };
  const rows = (spaceId: string) =>
    store
      .spaces()
      .find((s) => s.workspace.space_id === spaceId)!
      .rows.map((r) => r.pane.tab_id);

  beforeEach(async () => {
    tauri.reset();
    tauri.answer("snapshot", () => snap(["a", "b", "c"]));
    data = new Map();
    start();
    await store.refresh();
  });

  it("moves a tab before or after another and lists the rows there", () => {
    actions.reorderTabs("w1", "c", "a", false);
    expect(rows("w1")).toEqual(["c", "a", "b"]);
    actions.reorderTabs("w1", "c", "b", true);
    expect(rows("w1")).toEqual(["a", "b", "c"]);
  });

  it("refuses a drop across spaces and stores nothing", () => {
    actions.reorderTabs("w1", "a", "x2", true);
    actions.reorderTabs("w2", "a", "x1", false);
    expect(rows("w1")).toEqual(["a", "b", "c"]);
    expect(rows("w2")).toEqual(["x1", "x2"]);
    expect(data.has(UI_STATE_KEY)).toBe(false);
  });

  it("stores nothing for the same tab or an unchanged position", () => {
    actions.reorderTabs("w1", "a", "a", true);
    actions.reorderTabs("w1", "a", "b", false);
    expect(data.has(UI_STATE_KEY)).toBe(false);
  });

  it("restores the order after a restart", async () => {
    actions.reorderTabs("w1", "c", "a", false);
    start();
    await store.refresh();
    expect(rows("w1")).toEqual(["c", "a", "b"]);
  });

  it("appends new tabs and drops closed ones from the stored order", async () => {
    actions.reorderTabs("w1", "c", "a", false);
    tauri.answer("snapshot", () => snap(["a", "c", "d"]));
    await store.refresh();
    expect(rows("w1")).toEqual(["c", "a", "d"]);
    expect(ui.tabOrder()).toEqual({ w1: ["c", "a"] });
    expect(JSON.parse(data.get(UI_STATE_KEY)!).tabOrder).toEqual({ w1: ["c", "a"] });
  });

  it("ignores a malformed stored tab order", () => {
    const state = parseLayout(
      JSON.stringify({ tabOrder: { w1: ["a", 3, "a"], w2: "x", "": ["q"] } }),
    );
    expect(state.tabOrder).toEqual({ w1: ["a"] });
  });
});
