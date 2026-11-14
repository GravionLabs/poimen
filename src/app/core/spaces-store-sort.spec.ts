import { Injector } from "@angular/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Attention } from "./attention";
import { SettingsStore } from "./settings";
import { SORT_SETTLE_MS } from "./sort-settle";
import { SpacesStore } from "./spaces-store";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";
import { UI_STORAGE, UiState } from "./ui-state";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

type Status = "idle" | "working" | "blocked" | "done";
const statuses: Record<string, Status> = { w1: "idle", w2: "idle", w3: "idle" };

const wire = () => ({
  spaces: ["w1", "w2", "w3"].map((id, i) => ({
    space_id: id,
    label: id,
    number: i + 1,
    agent_status: "idle",
  })),
  tabs: [],
  terminals: Object.entries(statuses).map(([space, status]) => ({
    terminal_id: `t-${space}`,
    tab_id: "x",
    space_id: space,
    agent: "claude",
    agent_status: status,
  })),
});

describe("sorting spaces by attention and recent (#712)", () => {
  let store: SpacesStore;
  let settings: SettingsStore;
  let ui: UiState;
  let attention: Attention;

  beforeEach(async () => {
    vi.useFakeTimers();
    tauri.reset();
    Object.assign(statuses, { w1: "idle", w2: "idle", w3: "idle" });
    tauri.answer("snapshot", wire);
    const injector = Injector.create({
      providers: [
        TauriBridge,
        SettingsStore,
        SpacesStore,
        Attention,
        { provide: UI_STORAGE, useValue: null },
        UiState,
      ],
    });
    store = injector.get(SpacesStore);
    settings = injector.get(SettingsStore);
    ui = injector.get(UiState);
    attention = injector.get(Attention);
    await store.refresh();
  });

  afterEach(() => vi.useRealTimers());

  const listed = () => store.spaces().map((s) => s.workspace.space_id);

  it("puts the blocked space first and sorts at once on a mode change", async () => {
    statuses["w3"] = "blocked";
    statuses["w2"] = "working";
    await store.refresh();
    await settings.setSidebarSort("attention");
    expect(listed()).toEqual(["w3", "w2", "w1"]);
  });

  it("counts done as unread only through Attention", async () => {
    statuses["w2"] = "done";
    await store.refresh();
    await settings.setSidebarSort("attention");
    expect(listed()).toEqual(["w1", "w2", "w3"]);
    attention.markUnread("t-w2");
    expect(listed()).toEqual(["w1", "w2", "w3"]);
    vi.advanceTimersByTime(SORT_SETTLE_MS);
    expect(listed()).toEqual(["w2", "w1", "w3"]);
  });

  it("waits for the settle time after a status change, then re-sorts", async () => {
    await settings.setSidebarSort("attention");
    expect(listed()).toEqual(["w1", "w2", "w3"]);
    statuses["w3"] = "blocked";
    await store.refresh();
    expect(listed()).toEqual(["w1", "w2", "w3"]);
    vi.advanceTimersByTime(SORT_SETTLE_MS - 1);
    expect(listed()).toEqual(["w1", "w2", "w3"]);
    vi.advanceTimersByTime(1);
    expect(listed()).toEqual(["w3", "w1", "w2"]);
  });

  it("does not re-sort while the pointer is pressed, and does afterwards", async () => {
    await settings.setSidebarSort("attention");
    store.holdSort(true);
    statuses["w3"] = "blocked";
    await store.refresh();
    vi.advanceTimersByTime(SORT_SETTLE_MS * 2);
    expect(listed()).toEqual(["w1", "w2", "w3"]);
    store.holdSort(false);
    expect(listed()).toEqual(["w3", "w1", "w2"]);
  });

  it("sorts recent by focus time at once, never focused by newest number", async () => {
    await settings.setSidebarSort("recent");
    expect(listed()).toEqual(["w3", "w2", "w1"]);
    ui.touchSpace("w1", 10);
    expect(listed()).toEqual(["w1", "w3", "w2"]);
    ui.touchSpace("w2", 20);
    expect(listed()).toEqual(["w2", "w1", "w3"]);
  });

  it("forgets the focus time of a closed space", async () => {
    ui.touchSpace("w2", 20);
    ui.touchSpace("gone", 30);
    await store.refresh();
    expect(Object.keys(ui.recentSpaces())).toEqual(["w2"]);
  });
});
