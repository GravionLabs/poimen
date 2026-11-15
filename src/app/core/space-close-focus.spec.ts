import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SettingsStore } from "./settings";
import { SpaceActions } from "./space-actions";
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

const statuses: Record<string, string> = { w1: "idle", w2: "idle", w3: "idle" };
const cwds: Record<string, string> = { w1: "/src/a", w2: "/src/b", w3: "/src/a-wt" };

const wire = () => ({
  spaces: ["w1", "w2", "w3"].map((id, i) => ({
    space_id: id,
    label: id,
    number: i + 1,
    agent_status: "idle",
    cwd: cwds[id],
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

describe("focus after Close Space follows the sidebar order (#767)", () => {
  let injector: Injector;
  let store: SpacesStore;
  let settings: SettingsStore;
  let ui: UiState;
  let actions: SpaceActions;

  beforeEach(async () => {
    tauri.reset();
    tauri.answer("snapshot", wire);
    tauri.answer("space_close", () => undefined);
    tauri.answer("repo_info", () => ({}));
    injector = Injector.create({
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
    settings = injector.get(SettingsStore);
    ui = injector.get(UiState);
    actions = injector.get(SpaceActions);
    await store.refresh();
  });

  const focusAfterClosing = async (id: string) => {
    tauri.answer("space_close", () => undefined);
    const before = tauri.calls("space_close").length;
    await actions.requestClose(id);
    await actions.confirmClose();
    return tauri.calls("space_close")[before]?.["focus"];
  };

  it("uses the recent order", async () => {
    await settings.setSidebarSort("recent");
    ui.touchSpace("w1", 10);
    ui.touchSpace("w3", 20);
    // sidebar: w3, w1, w2
    expect(store.spaces().map((s) => s.workspace.space_id)).toEqual(["w3", "w1", "w2"]);
    expect(await focusAfterClosing("w3")).toBe("w1");
  });

  it("uses the attention order", async () => {
    statuses["w3"] = "blocked";
    await store.refresh();
    await settings.setSidebarSort("attention");
    // sidebar: w3, w1, w2 (by number, plain order would give w1, w2, w3)
    expect(store.spaces().map((s) => s.workspace.space_id)).toEqual(["w3", "w1", "w2"]);
    expect(await focusAfterClosing("w3")).toBe("w1");
    statuses["w3"] = "idle";
  });

  it("uses the worktree grouping", async () => {
    tauri.answer("repo_info", () => ({
      "/src/a": { branch: "main", worktree: false, common_dir: "/src/a/.git" },
      "/src/b": { branch: "main", worktree: false, common_dir: "/src/b/.git" },
      "/src/a-wt": { branch: "feat/x", worktree: true, common_dir: "/src/a/.git" },
    }));
    store.refreshRepos();
    // sidebar: w1, w3 (worktree of w1), w2
    await vi.waitFor(() =>
      expect(store.spaces().map((s) => s.workspace.space_id)).toEqual(["w1", "w3", "w2"]),
    );
    expect(await focusAfterClosing("w1")).toBe("w3");
  });
});
