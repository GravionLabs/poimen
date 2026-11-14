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
const statuses: Record<string, Status> = { w1: "working", w2: "working", w3: "idle" };

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

describe("priority order of the agents (#722)", () => {
  let store: SpacesStore;
  let injector: Injector;
  let settings: SettingsStore;

  beforeEach(async () => {
    vi.useFakeTimers();
    tauri.reset();
    Object.assign(statuses, { w1: "working", w2: "working", w3: "idle" });
    tauri.answer("snapshot", wire);
    injector = Injector.create({
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
    await store.refresh();
  });

  afterEach(() => vi.useRealTimers());

  const section = (id: string) => store.agentSections().get(id);

  it("computes no sections in list mode", () => {
    expect(store.agentSections().size).toBe(0);
  });

  it("sections the agents at once when the mode is switched", async () => {
    await settings.setSidebarAgents("priority");
    expect([section("t-w1"), section("t-w3")]).toEqual(["working", "idle"]);
  });

  it("holds a section change back until the statuses settled", async () => {
    await settings.setSidebarAgents("priority");
    expect(section("t-w3")).toBe("idle");
    statuses["w3"] = "blocked";
    await store.refresh();
    expect(section("t-w3")).toBe("idle");
    vi.advanceTimersByTime(SORT_SETTLE_MS + 10);
    expect(section("t-w3")).toBe("needs");
  });

  it("takes the next waiting agent in the shown order only in split layout and priority mode", async () => {
    const attention = injector.get(Attention);
    const at = (a: Status, b: Status, now: number) =>
      attention.observe(
        new Map([
          ["t-w1", a],
          ["t-w3", b],
        ]),
        new Set(),
        now,
      );
    at("working", "idle", 100);
    at("working", "blocked", 200);
    at("blocked", "blocked", 300);
    statuses["w1"] = "blocked";
    statuses["w3"] = "blocked";
    await store.refresh();
    // Layout and mode as they are: the list order.
    expect(store.nextWaiting(null)?.terminal_id).toBe("t-w1");
    await settings.setSidebarAgents("priority");
    expect(store.nextWaiting(null)?.terminal_id).toBe("t-w1");
    await settings.setSidebarLayout("split");
    // t-w3 has waited since 200, t-w1 since 300: t-w3 is shown first.
    expect(store.nextWaiting(null)?.terminal_id).toBe("t-w3");
    expect(store.nextWaiting("t-w3")?.terminal_id).toBe("t-w1");
    await settings.setSidebarAgents("list");
    expect(store.nextWaiting(null)?.terminal_id).toBe("t-w1");
  });
});
