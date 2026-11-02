import { Injector } from "@angular/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Pane, Snapshot } from "./herdr";
import { HerdrStore, REFRESH_DEBOUNCE_MS } from "./herdr-store";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const pane = (pane_id: string, over: Partial<Pane> = {}): Pane => ({
  pane_id,
  tab_id: "w1:t1",
  workspace_id: "w1",
  agent_status: "idle",
  ...over,
});

const snapshot = (...panes: Pane[]): Snapshot => ({
  workspaces: [{ workspace_id: "w1", label: "One", number: 1, agent_status: "idle" }],
  tabs: [{ tab_id: "w1:t1", workspace_id: "w1", label: "main", number: 1, agent_status: "idle" }],
  panes,
});

/** A promise the test settles by hand, to hold a `snapshot` call open. */
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => (resolve = r));
  return { promise, resolve };
}

describe("HerdrStore", () => {
  let injector: ReturnType<typeof Injector.create>;
  let store: HerdrStore;

  beforeEach(() => {
    vi.useFakeTimers();
    tauri.reset();
    tauri.answer("snapshot", () => snapshot(pane("w1:p1")));
    injector = Injector.create({ providers: [TauriBridge, HerdrStore] });
    store = injector.get(HerdrStore);
  });

  afterEach(() => {
    injector.destroy();
    vi.useRealTimers();
  });

  it("starts empty", () => {
    expect(store.snapshot()).toEqual({ workspaces: [], tabs: [], panes: [] });
    expect(store.tree()).toEqual([]);
    expect(store.connectionError()).toBeNull();
  });

  it("loads the first snapshot on start and listens once", async () => {
    await store.start();
    await store.start();

    expect(store.snapshot().panes.map((p) => p.pane_id)).toEqual(["w1:p1"]);
    expect(store.pane("w1:p1")?.agent_status).toBe("idle");
    expect(store.pane("w1:p9")).toBeUndefined();
    expect(tauri.listen).toHaveBeenCalledTimes(1);
    expect(tauri.calls("snapshot")).toHaveLength(1);
  });

  it("reads the herdr and app versions, herdr once per connection", async () => {
    tauri.answer("herdr_info", () => ({ version: "0.9.5", protocol: 22, supported: 22 }));
    tauri.answer("app_version", () => "0.2.0");
    await store.start();
    await store.refresh();

    expect(store.info()?.version).toBe("0.9.5");
    await vi.waitFor(() => expect(store.appVersion()).toBe("0.2.0"));
    expect(tauri.calls("herdr_info")).toHaveLength(1);
  });

  it("leaves the version unknown without an error when herdr cannot say", async () => {
    tauri.answer("herdr_info", () => {
      throw new Error("no socket");
    });
    await store.start();

    expect(store.info()).toBeNull();
    expect(store.connectionError()).toBeNull();
  });

  it("collapses a burst of events into one re-read after the debounce", async () => {
    await store.start();
    tauri.invoke.mockClear();

    tauri.emit("herdr", { kind: "pane.created", data: null });
    tauri.emit("herdr", { kind: "tab.renamed", data: null });
    tauri.emit("herdr", { kind: "pane.closed", data: null });
    await vi.advanceTimersByTimeAsync(REFRESH_DEBOUNCE_MS - 1);
    expect(tauri.calls("snapshot")).toHaveLength(0);

    await vi.advanceTimersByTimeAsync(1);
    expect(tauri.calls("snapshot")).toHaveLength(1);
  });

  it("queues exactly one more re-read for calls during a running one", async () => {
    const first = deferred<Snapshot>();
    tauri.answer("snapshot", () => first.promise);
    const running = store.refresh();

    tauri.answer("snapshot", () => snapshot(pane("w1:p1"), pane("w1:p2")));
    void store.refresh();
    void store.refresh();
    expect(tauri.calls("snapshot")).toHaveLength(1);

    first.resolve(snapshot(pane("w1:p1")));
    await running;
    await vi.runAllTimersAsync();

    expect(tauri.calls("snapshot")).toHaveLength(2);
    expect(store.snapshot().panes).toHaveLength(2);
  });

  it("applies an agent status change at once and re-reads afterwards", async () => {
    tauri.answer("snapshot", () => snapshot(pane("w1:p1"), pane("w1:p2")));
    await store.start();

    tauri.emit("herdr", {
      kind: "pane.agent_status_changed",
      data: { pane_id: "w1:p2", agent_status: "blocked" },
    });

    expect(store.pane("w1:p2")?.agent_status).toBe("blocked");
    expect(store.pane("w1:p1")?.agent_status).toBe("idle");
    expect(store.waiting()).toBe(1);

    await vi.advanceTimersByTimeAsync(REFRESH_DEBOUNCE_MS);
    expect(tauri.calls("snapshot")).toHaveLength(2);
  });

  it("ignores a status change without pane id or status", async () => {
    await store.start();
    const before = store.snapshot();

    tauri.emit("herdr", { kind: "pane.agent_status_changed", data: { pane_id: "w1:p1" } });
    tauri.emit("herdr", { kind: "pane.agent_status_changed", data: null });

    expect(store.snapshot()).toBe(before);
  });

  it("tracks the connection: disconnected without re-read, connected with one", async () => {
    await store.start();
    tauri.invoke.mockClear();

    tauri.emit("herdr", { kind: "poimen.disconnected", data: { reason: "socket gone" } });
    await vi.advanceTimersByTimeAsync(REFRESH_DEBOUNCE_MS);
    expect(store.connectionError()).toBe("socket gone");
    expect(store.connection()).toEqual({ state: "reconnecting", reason: "socket gone" });
    expect(tauri.calls("snapshot")).toHaveLength(0);

    tauri.emit("herdr", { kind: "poimen.connected", data: null });
    await vi.advanceTimersByTimeAsync(REFRESH_DEBOUNCE_MS);
    expect(store.connectionError()).toBeNull();
    expect(store.connection()).toEqual({ state: "connected" });
    expect(tauri.calls("snapshot")).toHaveLength(1);
  });

  it("reports a failed re-read as not reachable and keeps the last snapshot", async () => {
    await store.start();
    tauri.answer("snapshot", () => {
      throw "connection refused";
    });

    await store.refresh();

    expect(store.connectionError()).toBe("connection refused");
    expect(store.connection()).toEqual({ state: "unreachable", reason: "connection refused" });
    expect(store.snapshot().panes.map((p) => p.pane_id)).toEqual(["w1:p1"]);
  });

  it("stops listening and drops a pending re-read when destroyed", async () => {
    await store.start();
    tauri.invoke.mockClear();
    tauri.emit("herdr", { kind: "pane.created", data: null });

    injector.destroy();
    await vi.advanceTimersByTimeAsync(REFRESH_DEBOUNCE_MS);

    expect(tauri.unlisten).toHaveBeenCalledWith("herdr");
    expect(tauri.calls("snapshot")).toHaveLength(0);
    injector = Injector.create({ providers: [TauriBridge, HerdrStore] }); // afterEach destroys again
  });
});
