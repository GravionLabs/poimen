// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  mergeSnapshots,
  qualifyId,
  qualifySnapshot,
  splitId,
  paneTitle,
  withMachine,
  type Snapshot,
} from "./snapshot";
import { Attention } from "./attention";
import { DockBadge } from "./dock-badge";
import { NotificationCenter } from "./notification-center";
import { SpacesStore } from "./spaces-store";
import { Machines, type MachineStatus } from "./machines";
import { REMOTE_REFRESH_DEBOUNCE_MS, RemoteSpaces, remoteSnapshot } from "./remote-spaces";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const box = { name: "box", ssh: "box", reposDir: "~/Source/{owner}/{repo}" };
const connected: MachineStatus = { ...box, state: "connected", serverVersion: "0.17.0" };
const offline: MachineStatus = { ...box, state: "offline", error: "ssh: timeout" };

/** A snapshot of one space `api` with one agent terminal, ids as the machine's server has them. */
const snap = (status: "idle" | "blocked" = "idle"): Snapshot => ({
  spaces: [{ space_id: "s1", label: "api", number: 1, agent_status: status }],
  tabs: [
    {
      tab_id: "tab1",
      space_id: "s1",
      label: "claude",
      number: 1,
      agent_status: status,
      root: { kind: "terminal", terminal_id: "t1" },
      active_terminal: "t1",
    },
  ],
  terminals: [
    { terminal_id: "t1", tab_id: "tab1", space_id: "s1", agent: "claude", agent_status: status },
  ],
});

describe("qualified ids", () => {
  it("qualifies and splits ids, also when the machine name has a separator", () => {
    expect(qualifyId("box", "t1")).toBe("box::t1");
    expect(splitId("box::t1")).toEqual({ machine: "box", id: "t1" });
    expect(splitId(qualifyId("a::b", "t1"))).toEqual({ machine: "a::b", id: "t1" });
    expect(splitId("t1")).toEqual({ id: "t1" });
  });

  it("qualifies every id of a snapshot, including the tab tree", () => {
    const q = qualifySnapshot("box", snap());
    expect(q.spaces[0]).toMatchObject({ space_id: "box::s1", machine: "box" });
    expect(q.tabs[0]).toMatchObject({
      tab_id: "box::tab1",
      space_id: "box::s1",
      active_terminal: "box::t1",
      root: { kind: "terminal", terminal_id: "box::t1" },
    });
    expect(q.terminals[0]).toMatchObject({
      terminal_id: "box::t1",
      tab_id: "box::tab1",
      space_id: "box::s1",
      machine: "box",
    });
  });

  it("merges two machines and this Mac with the same space names and ids", () => {
    const merged = mergeSnapshots(snap(), [
      qualifySnapshot("box", snap()),
      qualifySnapshot("lab", snap()),
    ]);
    expect(merged.spaces.map((s) => s.label)).toEqual(["api", "api", "api"]);
    expect(merged.spaces.map((s) => s.space_id)).toEqual(["s1", "box::s1", "lab::s1"]);
    expect(new Set(merged.terminals.map((t) => t.terminal_id)).size).toBe(3);
  });

  it("puts the machine name in front of a title only for a remote pane", () => {
    expect(withMachine("API / claude", "box")).toBe("box: API / claude");
    expect(withMachine("API / claude", undefined)).toBe("API / claude");
  });
});

describe("remoteSnapshot", () => {
  it("shows the last known spaces of an offline machine with status unknown", () => {
    const read = new Map([["box", snap("blocked")]]);
    expect(remoteSnapshot([connected], read).terminals[0].agent_status).toBe("blocked");
    const old = remoteSnapshot([offline], read);
    expect(old.spaces).toHaveLength(1);
    expect(old.terminals[0].agent_status).toBe("unknown");
    expect(old.spaces[0].agent_status).toBe("unknown");
  });

  it("has nothing for a machine that was never read", () => {
    expect(remoteSnapshot([connected], new Map()).spaces).toEqual([]);
  });
});

describe("RemoteSpaces and the store", () => {
  beforeAll(() => {
    try {
      getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
    } catch {
      // already initialized
    }
  });

  beforeEach(() => {
    vi.useFakeTimers();
    tauri.reset();
    tauri.answer("list_machines", () => [{ ...connected }]);
    tauri.answer("snapshot", () => snap("idle"));
    tauri.answer("machine_snapshot", () => snap("idle"));
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [TauriBridge, SpacesStore] });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  async function started() {
    const machines = TestBed.inject(Machines);
    const remote = TestBed.inject(RemoteSpaces);
    const store = TestBed.inject(SpacesStore);
    await machines.start();
    await remote.start();
    TestBed.tick();
    await vi.advanceTimersByTimeAsync(0);
    return { machines, remote, store };
  }

  it("reads a machine when it connects and merges it into the store beside this Mac", async () => {
    const { store } = await started();
    expect(tauri.invoke).toHaveBeenCalledWith("machine_snapshot", { name: "box" });
    expect(store.remoteTree("box").map((n) => n.workspace.label)).toEqual(["api"]);
    expect([...store.panes().keys()]).toContain("box::t1");
    expect(store.pane("box::t1")?.machine).toBe("box");
  });

  it("counts a blocked remote agent as waiting and keeps it out of the local snapshot", async () => {
    tauri.answer("machine_snapshot", () => snap("blocked"));
    const { store } = await started();
    expect(store.waiting()).toBe(1);
    expect(store.nextWaiting(null)?.terminal_id).toBe("box::t1");
    expect(store.snapshot().terminals).toEqual([]);
  });

  it("applies a remote agent status at once and reads the machine again after an event", async () => {
    const { store } = await started();
    tauri.answer("machine_snapshot", () => snap("blocked"));
    tauri.invoke.mockClear();
    tauri.emit("machine:event", {
      machine: "box",
      kind: "terminal.agent_status_changed",
      data: { terminal_id: "t1", agent_status: "blocked" },
    });
    expect(store.pane("box::t1")?.agent_status).toBe("blocked");
    await vi.advanceTimersByTimeAsync(REMOTE_REFRESH_DEBOUNCE_MS);
    expect(tauri.invoke).toHaveBeenCalledWith("machine_snapshot", { name: "box" });
  });

  it("keeps the last known spaces, greyed, when the machine goes offline", async () => {
    const { store } = await started();
    tauri.emit("machine:state_changed", offline);
    TestBed.tick();
    expect(store.remoteTree("box")).toHaveLength(1);
    expect(store.pane("box::t1")?.agent_status).toBe("unknown");
  });

  it("carries a remote agent's change to attention, the notification center and the dock badge with the machine name", async () => {
    const { store } = await started();
    const attention = TestBed.inject(Attention);
    const center = TestBed.inject(NotificationCenter);
    const setBadge = vi.spyOn(TauriBridge.prototype, "setBadgeCount").mockResolvedValue();
    TestBed.inject(DockBadge);
    const statuses = () =>
      new Map([...store.panes()].map(([id, p]) => [id, p.agent_status] as const));
    const label = (id: string) => paneTitle(store.all(), store.pane(id)!);

    attention.observe(statuses(), new Set());
    tauri.emit("machine:event", {
      machine: "box",
      kind: "terminal.agent_status_changed",
      data: { terminal_id: "t1", agent_status: "blocked" },
    });
    const changes = attention.observe(statuses(), new Set());
    center.record(changes, label, 1);

    expect(changes).toEqual([{ paneId: "box::t1", status: "blocked" }]);
    expect(attention.count()).toBe(1);
    expect(center.entries()[0].label).toBe("box: API / claude");
    TestBed.tick();
    expect(setBadge).toHaveBeenLastCalledWith(1);
  });

  it("labels a remote pane with the machine name once in its title", async () => {
    const { store } = await started();
    expect(paneTitle(store.all(), store.pane("box::t1")!)).toBe("box: API / claude");
  });
});
