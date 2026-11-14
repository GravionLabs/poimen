import { Injector } from "@angular/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Terminal, Snapshot } from "./snapshot";
import { SpacesStore, REPOS_REFRESH_MS } from "./spaces-store";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const pane = (terminal_id: string, over: Partial<Terminal> = {}): Terminal => ({
  terminal_id,
  tab_id: "w1:t1",
  space_id: "w1",
  agent_status: "idle",
  ...over,
});

const snapshot = (...terminals: Terminal[]): Snapshot => ({
  spaces: [{ space_id: "w1", label: "One", number: 1, agent_status: "idle" }],
  tabs: [{ tab_id: "w1:t1", space_id: "w1", label: "main", number: 1, agent_status: "idle" }],
  terminals,
});

describe("SpacesStore repos", () => {
  let injector: ReturnType<typeof Injector.create>;
  let store: SpacesStore;

  let focused = true;

  beforeEach(() => {
    vi.useFakeTimers();
    tauri.reset();
    focused = true;
    vi.stubGlobal("window", new EventTarget());
    vi.stubGlobal("document", { hasFocus: () => focused });
    injector = Injector.create({ providers: [TauriBridge, SpacesStore] });
    store = injector.get(SpacesStore);
  });

  afterEach(() => {
    injector.destroy();
    vi.unstubAllGlobals();
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

  it("asks for the directory of a space without terminals and keeps it on the space", async () => {
    tauri.answer("snapshot", () => ({
      spaces: [{ space_id: "s1", label: "app", number: 1, agent_status: "unknown", cwd: "/src/b" }],
      tabs: [],
      terminals: [],
    }));
    tauri.answer("repo_info", () => ({ "/src/b": { branch: "main", worktree: false } }));

    await store.start();

    expect(tauri.calls("repo_info")).toEqual([{ dirs: ["/src/b"] }]);
    expect(store.spaces()[0].repo).toEqual({ branch: "main", worktree: false });
    expect(store.spaces()[0].workspace.cwd).toBe("/src/b");
  });

  it("carries dirty state and ahead/behind into the rows, null as unknown", async () => {
    tauri.answer("snapshot", () => snapshot(pane("w1:p1", { cwd: "/src/a" })));
    tauri.answer("repo_info", () => ({
      "/src/a": { branch: "main", worktree: false, dirty: 3, ahead: 2, behind: null },
    }));

    await store.start();

    const r = store.spaces()[0].rows[0];
    expect([r.dirty, r.ahead, r.behind]).toEqual([3, 2, undefined]);
  });

  it("does not run git on every snapshot read, but on focus, on done and on the 60 s timer", async () => {
    tauri.answer("snapshot", () => snapshot(pane("w1:p1", { cwd: "/src/a" })));
    tauri.answer("repo_info", () => ({}));
    await store.start();
    expect(tauri.calls("repo_info")).toHaveLength(1);

    await store.refresh();
    expect(tauri.calls("repo_info")).toHaveLength(1);

    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(100);
    expect(tauri.calls("repo_info")).toHaveLength(2);

    tauri.emit("herdr", {
      kind: "pane.agent_status_changed",
      data: { pane_id: "w1:p1", agent_status: "done" },
    });
    await vi.advanceTimersByTimeAsync(100);
    expect(tauri.calls("repo_info")).toHaveLength(3);

    await vi.advanceTimersByTimeAsync(REPOS_REFRESH_MS);
    expect(tauri.calls("repo_info")).toHaveLength(4);
  });

  it.each(["done", "idle"] as const)(
    "re-reads the repos when a pane turns %s while the window is focused",
    async (agent_status) => {
      tauri.answer("snapshot", () => snapshot(pane("w1:p1", { cwd: "/src/a" })));
      tauri.answer("repo_info", () => ({}));
      await store.start();
      expect(tauri.calls("repo_info")).toHaveLength(1);

      tauri.emit("herdr", {
        kind: "pane.agent_status_changed",
        data: { pane_id: "w1:p1", agent_status },
      });
      await vi.advanceTimersByTimeAsync(100);

      expect(tauri.calls("repo_info")).toHaveLength(2);
    },
  );

  it.each(["done", "idle"] as const)(
    "does not re-read the repos when a pane turns %s while the window is not focused",
    async (agent_status) => {
      tauri.answer("snapshot", () => snapshot(pane("w1:p1", { cwd: "/src/a" })));
      tauri.answer("repo_info", () => ({}));
      await store.start();
      focused = false;

      tauri.emit("herdr", {
        kind: "pane.agent_status_changed",
        data: { pane_id: "w1:p1", agent_status },
      });
      await vi.advanceTimersByTimeAsync(100);

      expect(tauri.calls("repo_info")).toHaveLength(1);
    },
  );

  it("does not re-read the repos when a pane turns working", async () => {
    tauri.answer("snapshot", () => snapshot(pane("w1:p1", { cwd: "/src/a" })));
    tauri.answer("repo_info", () => ({}));
    await store.start();

    tauri.emit("herdr", {
      kind: "pane.agent_status_changed",
      data: { pane_id: "w1:p1", agent_status: "working" },
    });
    await vi.advanceTimersByTimeAsync(100);

    expect(tauri.calls("repo_info")).toHaveLength(1);
  });

  it("does not run git on the timer while the window is not focused", async () => {
    tauri.answer("snapshot", () => snapshot(pane("w1:p1", { cwd: "/src/a" })));
    tauri.answer("repo_info", () => ({}));
    focused = false;
    await store.start();

    await vi.advanceTimersByTimeAsync(REPOS_REFRESH_MS * 2);

    expect(tauri.calls("repo_info")).toHaveLength(1);
  });

  it("asks for the pull requests of the pane directories and puts them on the rows", async () => {
    tauri.answer("snapshot", () => snapshot(pane("w1:p1", { cwd: "/src/a" })));
    tauri.answer("repo_info", () => ({ "/src/a": { branch: "feat/x", worktree: false } }));
    const pr = {
      number: 5,
      title: "T",
      url: "https://github.com/o/r/pull/5",
      state: "open",
      isDraft: false,
      checks: { state: "none", passed: 0, failed: 0, pending: 0 },
    };
    tauri.answer("branch_pulls", () => ({ "/src/a": pr }));

    await store.start();
    await vi.advanceTimersByTimeAsync(0);

    expect(tauri.calls("branch_pulls")).toEqual([{ dirs: ["/src/a"] }]);
    expect(store.spaces()[0].rows[0].pr?.number).toBe(5);
  });

  it("keeps the previous pull requests when the lookup fails", async () => {
    tauri.answer("snapshot", () => snapshot(pane("w1:p1", { cwd: "/src/a" })));
    tauri.answer("repo_info", () => ({ "/src/a": { branch: "feat/x", worktree: false } }));
    tauri.answer("branch_pulls", () => ({
      "/src/a": { number: 5, title: "T", url: "u", state: "open", isDraft: false, checks: {} },
    }));
    await store.start();
    await vi.advanceTimersByTimeAsync(0);
    tauri.answer("branch_pulls", () => {
      throw new Error("boom");
    });

    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(100);

    expect(tauri.calls("branch_pulls")).toHaveLength(2);
    expect(store.spaces()[0].rows[0].pr?.number).toBe(5);
    expect(store.connectionError()).toBeNull();
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
