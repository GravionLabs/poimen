import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Snapshot } from "./snapshot";
import { SpacesStore } from "./spaces-store";
import { SpaceActions } from "./space-actions";
import { SpaceRebuild, type RebuildOutcome } from "./space-rebuild";
import { TauriBridge } from "./tauri";
import { UI_STORAGE, UiState } from "./ui-state";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

/** The snapshot of a restarted server: space `s1` in `/work/app` without terminals. */
const restarted = (): Snapshot => ({
  spaces: [{ space_id: "s1", label: "app", number: 1, agent_status: "unknown", cwd: "/work/app" }],
  tabs: [],
  terminals: [],
});

const REQUEST = { root: "/work/app", files: [], hash: "h1", commands: [] };

describe("SpaceRebuild", () => {
  let store: SpacesStore;
  let actions: SpaceActions;
  let rebuild: SpaceRebuild;
  let snapshot: Snapshot;
  let answer: (args?: Record<string, unknown>) => RebuildOutcome;

  beforeEach(() => {
    tauri.reset();
    snapshot = restarted();
    answer = () => ({ state: "rebuilt", warnings: [], setup_started: false });
    tauri.answer("snapshot", () => snapshot);
    tauri.answer("rebuild_space", (args) => answer(args));
    const injector = Injector.create({
      providers: [
        TauriBridge,
        SpacesStore,
        { provide: UI_STORAGE, useValue: null },
        UiState,
        SpaceActions,
        SpaceRebuild,
      ],
    });
    store = injector.get(SpacesStore);
    actions = injector.get(SpaceActions);
    rebuild = injector.get(SpaceRebuild);
    rebuild.start();
  });

  const calls = () => tauri.calls("rebuild_space");
  const settle = () => vi.waitFor(() => expect(rebuild.marks().get("s1")?.kind).not.toBe("busy"));

  it("rebuilds an empty space on the first snapshot of a session without asking", async () => {
    await store.refresh();
    await settle();
    expect(calls()).toEqual([{ spaceId: "s1", decision: null, confirmed: false }]);
    expect(rebuild.marks().size).toBe(0);
    // The rebuilt space is re-read.
    expect(tauri.calls("snapshot").length).toBeGreaterThanOrEqual(2);
  });

  it("leaves spaces with terminals and spaces without a directory alone", async () => {
    snapshot = {
      spaces: [
        { space_id: "s1", label: "a", number: 1, agent_status: "idle", cwd: "/work/app" },
        { space_id: "s2", label: "b", number: 2, agent_status: "idle" },
      ],
      tabs: [],
      terminals: [{ terminal_id: "t1", tab_id: "t1", space_id: "s1", agent_status: "idle" }],
    };
    await store.refresh();
    expect(calls()).toEqual([]);
  });

  it("checks once per session: a later read does not try again", async () => {
    answer = () => ({ state: "missing", reason: "the directory /work/app no longer exists" });
    await store.refresh();
    await settle();
    await store.refresh();
    expect(calls()).toHaveLength(1);
    expect(rebuild.marks().get("s1")).toEqual({
      kind: "missing",
      reason: "the directory /work/app no longer exists",
    });
  });

  it("checks again after the server connection was lost and came back", async () => {
    answer = () => ({ state: "consent", reasons: ['pane "agent" starts an agent: claude'] });
    await store.start();
    await settle();
    tauri.emit("herdr", { kind: "poimen.disconnected", data: { reason: "gone" } });
    await store.refresh();
    await settle();
    expect(calls()).toHaveLength(2);
    expect(calls().every((c) => c?.["confirmed"] === false)).toBe(true);
  });

  it("asks for consent and rebuilds on the user's Rebuild", async () => {
    answer = (args) =>
      args?.["confirmed"]
        ? { state: "rebuilt", warnings: ["layout not fully applied: boom"], setup_started: true }
        : { state: "consent", reasons: ["setup runs: pnpm i"] };
    await store.refresh();
    await settle();
    expect(rebuild.marks().get("s1")).toEqual({ kind: "consent", reasons: ["setup runs: pnpm i"] });
    await rebuild.rebuild("s1");
    expect(calls()[1]).toEqual({ spaceId: "s1", decision: null, confirmed: true });
    expect(rebuild.marks().has("s1")).toBe(false);
    expect(actions.warning()).toBe("layout not fully applied: boom");
  });

  it("goes through the trust dialog: trust and without resume, cancel keeps the action", async () => {
    answer = (args) =>
      args?.["decision"]
        ? { state: "rebuilt", warnings: [], setup_started: false }
        : { state: "trust", request: REQUEST };
    await rebuild.rebuild("s1");
    expect(actions.trust()?.request).toEqual(REQUEST);
    expect(rebuild.marks().get("s1")?.kind).toBe("consent");
    await actions.answerTrust("cancel");
    expect(calls()).toHaveLength(1);
    expect(rebuild.marks().get("s1")?.kind).toBe("consent");

    await rebuild.rebuild("s1");
    await actions.answerTrust("trust");
    expect(calls()[2]).toEqual({
      spaceId: "s1",
      decision: { kind: "trust", hash: "h1" },
      confirmed: true,
    });

    // Asked again, now answered without project commands.
    await rebuild.rebuild("s1");
    await actions.answerTrust("without");
    expect(calls().at(-1)).toEqual({
      spaceId: "s1",
      decision: { kind: "without" },
      confirmed: true,
    });
    expect(rebuild.marks().has("s1")).toBe(false);
  });

  it("marks a failed rebuild and does not retry it in the same session", async () => {
    tauri.answer("rebuild_space", () => {
      throw new Error("server gone");
    });
    await store.refresh();
    await settle();
    expect(rebuild.marks().get("s1")).toEqual({ kind: "failed", reason: "Error: server gone" });
    await store.refresh();
    expect(calls()).toHaveLength(1);
  });
});
