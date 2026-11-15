import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { buildTree, orderSpaces, reorderIds, type Snapshot } from "./snapshot";
import { SettingsStore } from "./settings";
import { SpacesStore } from "./spaces-store";
import { SpaceActions, neighbourSpace } from "./space-actions";
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
const snap = (...ids: string[]): Snapshot => ({
  spaces: ids.map((id) => space(id, Number(id.slice(1)))),
  tabs: [],
  terminals: [],
});
const ids = (list: { space_id: string }[]) => list.map((s) => s.space_id);

describe("orderSpaces", () => {
  const spaces = snap("w1", "w2", "w3", "w4").spaces;

  it("uses the number order without a stored order", () => {
    expect(ids(orderSpaces(spaces, []))).toEqual(["w1", "w2", "w3", "w4"]);
  });

  it("puts the stored order first and appends new spaces by number", () => {
    expect(ids(orderSpaces(spaces, ["w3", "w1"]))).toEqual(["w3", "w1", "w2", "w4"]);
  });

  it("skips stored ids of closed spaces", () => {
    expect(ids(orderSpaces(spaces, ["w9", "w4", "w2"]))).toEqual(["w4", "w2", "w1", "w3"]);
  });

  it("does not modify its input", () => {
    const copy = [...spaces];
    orderSpaces(spaces, ["w4"]);
    expect(spaces).toEqual(copy);
  });
});

describe("reorderIds", () => {
  const list = ["a", "b", "c", "d"];

  it("moves a space before a target", () => {
    expect(reorderIds(list, "d", "b", false)).toEqual(["a", "d", "b", "c"]);
  });

  it("moves a space after a target", () => {
    expect(reorderIds(list, "a", "c", true)).toEqual(["b", "c", "a", "d"]);
  });

  it("changes nothing on the same space, an unknown id or the same position", () => {
    expect(reorderIds(list, "b", "b", true)).toBeNull();
    expect(reorderIds(list, "x", "b", true)).toBeNull();
    expect(reorderIds(list, "b", "x", true)).toBeNull();
    expect(reorderIds(list, "b", "c", false)).toBeNull();
    expect(reorderIds(list, "c", "b", true)).toBeNull();
  });
});

describe("buildTree with a user order", () => {
  it("lists the workspaces in that order", () => {
    expect(buildTree(snap("w1", "w2", "w3"), ["w3"]).map((n) => n.workspace.space_id)).toEqual([
      "w3",
      "w1",
      "w2",
    ]);
  });
});

describe("neighbourSpace with a user order", () => {
  it("follows the user order", () => {
    const order = ["w3", "w1", "w2"];
    expect(neighbourSpace(snap("w1", "w2", "w3"), "w3", order)).toBe("w1");
    expect(neighbourSpace(snap("w1", "w2", "w3"), "w2", order)).toBe("w1");
    expect(neighbourSpace(snap("w1", "w2", "w3"), "w1", order)).toBe("w2");
  });
});

describe("parseLayout spaceOrder", () => {
  it("keeps strings once and ignores other values", () => {
    const raw = JSON.stringify({ spaceOrder: ["w2", "w1", "w2", 3, ""] });
    expect(parseLayout(raw).spaceOrder).toEqual(["w2", "w1"]);
    expect(parseLayout(JSON.stringify({ spaceOrder: "w1" })).spaceOrder).toBeUndefined();
    expect(parseLayout(null).spaceOrder).toBeUndefined();
  });
});

function fakeStorage(): UiStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (key) => data.get(key) ?? null,
    setItem: (key, value) => void data.set(key, value),
  };
}

describe("space order in the app", () => {
  let storage: ReturnType<typeof fakeStorage>;
  let injector: Injector;
  let store: SpacesStore;
  let actions: SpaceActions;
  let ui: UiState;

  const start = () => {
    injector = Injector.create({
      providers: [
        TauriBridge,
        SettingsStore,
        SpacesStore,
        { provide: UI_STORAGE, useValue: storage },
        UiState,
        SpaceActions,
      ],
    });
    store = injector.get(SpacesStore);
    actions = injector.get(SpaceActions);
    ui = injector.get(UiState);
  };
  const listed = () => store.spaces().map((s) => s.workspace.space_id);

  beforeEach(async () => {
    tauri.reset();
    tauri.answer("snapshot", () => snap("w1", "w2", "w3"));
    storage = fakeStorage();
    start();
    await store.refresh();
  });

  it("drops a space before or after another and lists it there", () => {
    actions.reorder("w3", "w1", false);
    expect(listed()).toEqual(["w3", "w1", "w2"]);
    actions.reorder("w3", "w2", true);
    expect(listed()).toEqual(["w1", "w2", "w3"]);
  });

  it("changes and stores nothing for a drop on the same position", () => {
    actions.reorder("w1", "w2", false);
    actions.reorder("w2", "w2", true);
    expect(listed()).toEqual(["w1", "w2", "w3"]);
    expect(storage.data.has(UI_STATE_KEY)).toBe(false);
  });

  it("restores the order after a restart", async () => {
    actions.reorder("w3", "w1", false);
    start();
    await store.refresh();
    expect(listed()).toEqual(["w3", "w1", "w2"]);
  });

  it("appends a new space and removes a closed one from the stored order", async () => {
    actions.reorder("w3", "w1", false);
    tauri.answer("snapshot", () => snap("w1", "w4", "w3"));
    await store.refresh();
    expect(listed()).toEqual(["w3", "w1", "w4"]);
    expect(ui.spaceOrder()).toEqual(["w3", "w1"]);
    expect(JSON.parse(storage.data.get(UI_STATE_KEY)!).spaceOrder).toEqual(["w3", "w1"]);
  });

  it("shifts a space up and down and stops at the ends", () => {
    actions.shift("w3", -1);
    expect(listed()).toEqual(["w1", "w3", "w2"]);
    actions.shift("w1", 1);
    expect(listed()).toEqual(["w3", "w1", "w2"]);
    actions.shift("w3", -1);
    actions.shift("w2", 1);
    expect(listed()).toEqual(["w3", "w1", "w2"]);
  });

  it("orders the tree and the neighbour of a closed space the same way", () => {
    actions.reorder("w3", "w1", false);
    expect(store.tree().map((n) => n.workspace.space_id)).toEqual(["w3", "w1", "w2"]);
    expect(neighbourSpace(store.snapshot(), "w3", store.spaceOrder())).toBe("w1");
  });
});

describe("orderSpaces by name", () => {
  const named = (id: string, label: string, number: number) => ({ ...space(id, number), label });

  it("sorts labels with numbers in numeric order, ignoring case", () => {
    const list = [
      named("a", "#12 beta", 1),
      named("b", "#9 alpha", 2),
      named("c", "apple", 3),
      named("d", "Banana", 4),
    ];
    expect(ids(orderSpaces(list, ["d"], "name"))).toEqual(["b", "a", "c", "d"]);
  });

  it("breaks ties by backend number and does not touch the input", () => {
    const list = [named("a", "same", 2), named("b", "SAME", 1)];
    expect(ids(orderSpaces(list, [], "name"))).toEqual(["b", "a"]);
    expect(ids(list)).toEqual(["a", "b"]);
  });

  it("keeps the manual order in manual mode", () => {
    const list = [named("a", "z", 1), named("b", "a", 2)];
    expect(ids(orderSpaces(list, ["a", "b"], "manual"))).toEqual(["a", "b"]);
  });

  it("finds the neighbour in name order and buildTree follows it", () => {
    const snapshot: Snapshot = {
      spaces: [named("a", "c", 1), named("b", "a", 2), named("c", "b", 3)],
      tabs: [],
      terminals: [],
    };
    expect(buildTree(snapshot, [], {}, "name").map((n) => n.workspace.space_id)).toEqual([
      "b",
      "c",
      "a",
    ]);
    expect(neighbourSpace(snapshot, "b", [], "name")).toBe("c");
  });
});

describe("sorting spaces by name in the app", () => {
  let store: SpacesStore;
  let actions: SpaceActions;
  let settings: SettingsStore;
  let ui: UiState;

  beforeEach(async () => {
    tauri.reset();
    const names: Record<string, string> = { w1: "#12 b", w2: "#9 a", w3: "c" };
    tauri.answer("snapshot", () => {
      const s = snap("w1", "w2", "w3");
      return { ...s, spaces: s.spaces.map((x) => ({ ...x, label: names[x.space_id] })) };
    });
    const injector = Injector.create({
      providers: [
        TauriBridge,
        SettingsStore,
        SpacesStore,
        { provide: UI_STORAGE, useValue: null },
        UiState,
        SpaceActions,
      ],
    });
    store = injector.get(SpacesStore);
    actions = injector.get(SpaceActions);
    settings = injector.get(SettingsStore);
    ui = injector.get(UiState);
    await store.refresh();
  });

  const listed = () => store.spaces().map((s) => s.workspace.space_id);

  it("lists by name in name mode and ignores drops and shifts", async () => {
    await settings.setSidebarSort("name");
    expect(listed()).toEqual(["w2", "w1", "w3"]);
    actions.reorder("w3", "w2", false);
    actions.shift("w3", -1);
    expect(listed()).toEqual(["w2", "w1", "w3"]);
    expect(ui.spaceOrder()).toEqual([]);
  });

  it("sorts now: stores the by-name order and returns to manual", async () => {
    await settings.setSidebarSort("name");
    actions.sortNow();
    expect(ui.spaceOrder()).toEqual(["w2", "w1", "w3"]);
    await vi.waitFor(() => expect(settings.settings().sidebarSort).toBe("manual"));
    expect(listed()).toEqual(["w2", "w1", "w3"]);
  });
});
