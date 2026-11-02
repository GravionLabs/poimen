import { Injector } from "@angular/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Pane, Snapshot } from "./herdr";
import { HerdrStore } from "./herdr-store";
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

describe("HerdrStore repos", () => {
  let injector: ReturnType<typeof Injector.create>;
  let store: HerdrStore;

  beforeEach(() => {
    vi.useFakeTimers();
    tauri.reset();
    injector = Injector.create({ providers: [TauriBridge, HerdrStore] });
    store = injector.get(HerdrStore);
  });

  afterEach(() => {
    injector.destroy();
    vi.useRealTimers();
  });

  it("asks for each pane directory once and shows the branch in the spaces", async () => {
    tauri.answer("snapshot", () =>
      snapshot(pane("w1:p1", { foreground_cwd: "/src/a" }), pane("w1:p2", { cwd: "/src/a" })),
    );
    tauri.answer("repo_info", () => ({ "/src/a": { branch: "main", worktree: false } }));

    await store.start();

    expect(tauri.calls("repo_info")).toEqual([{ dirs: ["/src/a"] }]);
    expect(store.spaces()[0].rows.map((r) => r.branch)).toEqual(["main", "main"]);
  });

  it("keeps the spaces without branches when the lookup fails", async () => {
    tauri.answer("snapshot", () => snapshot(pane("w1:p1", { cwd: "/src/a" })));
    tauri.answer("repo_info", () => {
      throw new Error("git missing");
    });

    await store.start();

    expect(store.connectionError()).toBeNull();
    expect(store.spaces()[0].rows).toHaveLength(1);
    expect(store.spaces()[0].rows[0].branch).toBeUndefined();
  });
});
