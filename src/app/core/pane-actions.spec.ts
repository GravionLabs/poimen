import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Pane, Snapshot } from "./herdr";
import { HerdrStore } from "./herdr-store";
import { PaneActions } from "./pane-actions";
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
  cwd: "/work/app",
  ...over,
});

describe("PaneActions", () => {
  let current: Snapshot;
  let actions: PaneActions;
  let store: HerdrStore;

  beforeEach(async () => {
    tauri.reset();
    current = {
      workspaces: [{ workspace_id: "w1", label: "One", number: 1, agent_status: "idle" }],
      tabs: [
        { tab_id: "w1:t1", workspace_id: "w1", label: "main", number: 1, agent_status: "idle" },
      ],
      panes: [pane("w1:p1"), pane("w1:p2", { agent: "claude", agent_status: "working" })],
    };
    tauri.answer("snapshot", () => current);
    const injector = Injector.create({ providers: [TauriBridge, HerdrStore, PaneActions] });
    store = injector.get(HerdrStore);
    actions = injector.get(PaneActions);
    await store.refresh();
  });

  it("asks for a directory, offering the workspace's directories", () => {
    actions.askNew("w1");
    expect(actions.dialog()).toEqual({
      kind: "new",
      workspaceId: "w1",
      dirs: ["/work/app"],
      cwd: "/work/app",
    });
  });

  it("creates a terminal in the chosen directory and returns the new pane", async () => {
    tauri.answer("terminal_create", () => {
      current = { ...current, panes: [...current.panes, pane("w1:p3", { cwd: "/tmp" })] };
      return "w1:p3";
    });
    actions.askNew("w1");
    const created = await actions.confirmNew("  /tmp ");
    expect(tauri.calls("terminal_create")).toEqual([{ workspaceId: "w1", cwd: "/tmp" }]);
    expect(created?.pane_id).toBe("w1:p3");
    expect(actions.dialog()).toBeNull();
  });

  it("sends no directory when the field is empty", async () => {
    tauri.answer("terminal_create", () => null);
    actions.askNew("w1");
    expect(await actions.confirmNew("")).toBeUndefined();
    expect(tauri.calls("terminal_create")).toEqual([{ workspaceId: "w1", cwd: null }]);
  });

  it("splits in the pane's own directory", async () => {
    tauri.answer("pane_split", () => "w1:p9");
    await actions.split("w1:p1", "down");
    expect(tauri.calls("pane_split")).toEqual([
      { target: "w1:p1", direction: "down", cwd: "/work/app" },
    ]);
  });

  it("reports a failed split instead of throwing", async () => {
    tauri.answer("pane_split", () => {
      throw "herdr not_found: no pane";
    });
    expect(await actions.split("w1:p1", "right")).toBeUndefined();
    expect(actions.error()).toContain("not_found");
    actions.dismissError();
    expect(actions.error()).toBeNull();
  });

  it("does not close before the user confirmed", () => {
    actions.askClose("w1:p2");
    expect(tauri.calls("pane_close")).toEqual([]);
    expect(actions.dialog()).toMatchObject({ kind: "close", paneId: "w1:p2", name: "claude" });
    actions.cancel();
    expect(actions.dialog()).toBeNull();
    expect(tauri.calls("pane_close")).toEqual([]);
  });

  it("warns about a running agent in the close confirmation", () => {
    actions.askClose("w1:p2");
    expect(actions.dialog()).toMatchObject({ warning: expect.stringContaining("working") });
  });

  it("closes the confirmed pane and re-reads the snapshot", async () => {
    tauri.answer("pane_close", () => {
      current = { ...current, panes: current.panes.filter((p) => p.pane_id !== "w1:p1") };
    });
    actions.askClose("w1:p1");
    expect(await actions.confirmClose()).toBe("w1:p1");
    expect(tauri.calls("pane_close")).toEqual([{ paneId: "w1:p1" }]);
    expect(store.pane("w1:p1")).toBeUndefined();
  });

  it("keeps the error when herdr refuses the close", async () => {
    tauri.answer("pane_close", () => {
      throw "herdr not_found: gone";
    });
    actions.askClose("w1:p1");
    expect(await actions.confirmClose()).toBeUndefined();
    expect(actions.error()).toContain("gone");
  });

  it("confirms nothing without a matching dialog", async () => {
    expect(await actions.confirmClose()).toBeUndefined();
    expect(await actions.confirmNew("/x")).toBeUndefined();
    expect(tauri.calls("pane_close")).toEqual([]);
  });
});
