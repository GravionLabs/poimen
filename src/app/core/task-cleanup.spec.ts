import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { HerdrStore } from "./herdr-store";
import { SpaceActions } from "./space-actions";
import { TaskCleanup, cleanupLog, cleanupSummary, type CleanupReport } from "./task-cleanup";
import { ResultDialog } from "./result-dialog";
import { TaskLog } from "./task-log";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";
import { UI_STORAGE, UiState } from "./ui-state";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

const REPORT: CleanupReport = {
  removed: [
    { path: "/work/app-1", branch: "feat/1-a", pr: 11, branchDeleted: true, note: null },
    {
      path: "/work/app-2",
      branch: "feat/2-b",
      pr: 12,
      branchDeleted: false,
      note: "branch kept: 1 commit(s) on no remote branch",
    },
  ],
  left: [
    {
      path: "/work/app-3",
      branch: "feat/3-c",
      pr: 13,
      reason: "it has uncommitted or untracked changes",
      command: "git worktree remove '/work/app-3' && git branch -d 'feat/3-c'",
    },
  ],
};

describe("cleanup report text", () => {
  it("summarizes as '<n> removed, <m> left'", () => {
    expect(cleanupSummary(REPORT)).toBe("Clean up merged tasks: 2 removed, 1 left");
    expect(cleanupSummary({ removed: [], left: [] })).toBe(
      "Clean up merged tasks: 0 removed, 0 left",
    );
  });

  it("logs kept branches and every left worktree with reason and the command by hand", () => {
    expect(cleanupLog(REPORT)).toEqual([
      { text: "Clean up merged tasks: 2 removed, 1 left" },
      {
        text: "Removed /work/app-2 (PR #12); branch kept: 1 commit(s) on no remote branch",
      },
      {
        text: "Left /work/app-3 (feat/3-c, PR #13): it has uncommitted or untracked changes",
      },
      {
        text: "Remove it by hand: git worktree remove '/work/app-3' && git branch -d 'feat/3-c'",
        detail: true,
      },
    ]);
  });
});

describe("TaskCleanup", () => {
  let cleanup: TaskCleanup;
  let spaces: SpaceActions;
  let log: TaskLog;
  let ui: UiState;
  let injector: Injector;

  beforeEach(() => {
    tauri.reset();
    tauri.answer("snapshot", () => ({ spaces: [], tabs: [], terminals: [] }));
    injector = Injector.create({
      providers: [
        { provide: UI_STORAGE, useValue: null },
        TauriBridge,
        HerdrStore,
        UiState,
        SpaceActions,
        TaskLog,
        ResultDialog,
        TaskCleanup,
      ],
    });
    cleanup = injector.get(TaskCleanup);
    spaces = injector.get(SpaceActions);
    log = injector.get(TaskLog);
    ui = injector.get(UiState);
  });

  it("passes the repository and the focused space, then notifies and logs", async () => {
    tauri.answer("task_cleanup", () => REPORT);
    expect(await cleanup.run("/work/app", "w1")).toEqual(REPORT);
    expect(tauri.calls("task_cleanup")).toEqual([
      { dir: "/work/app", focusedSpace: "w1", skipTeardown: false },
    ]);
    expect(spaces.warning()).toBe("Clean up merged tasks: 2 removed, 1 left");
    expect(log.lines().map((l) => l.text)).toHaveLength(4);
    expect(ui.panelTab()).toBe("log");
    expect(ui.panelOpen()).toBe(true);
  });

  it("asks before removing without teardown when the project is untrusted", async () => {
    const dialog = injector.get(ResultDialog);
    tauri.answer("task_cleanup", (args) =>
      (args as { skipTeardown: boolean }).skipTeardown
        ? REPORT
        : { removed: [], left: [], teardownUntrusted: true },
    );
    await cleanup.run("/work/app", "w1");
    expect(log.lines()).toHaveLength(0);
    expect(dialog.content()?.action?.label).toBe("Remove without teardown");
    dialog.runAction();
    await vi.waitFor(() => expect(tauri.calls("task_cleanup")).toHaveLength(2));
    expect(tauri.calls("task_cleanup")[1]).toMatchObject({ skipTeardown: true });
  });

  it("does not open the log when nothing was left", async () => {
    tauri.answer("task_cleanup", () => ({ removed: REPORT.removed, left: [] }));
    await cleanup.run("/work/app", null);
    expect(ui.panelOpen()).toBe(false);
    expect(spaces.warning()).toBe("Clean up merged tasks: 2 removed, 0 left");
  });

  it("shows and logs a failure and resolves to null", async () => {
    tauri.answer("task_cleanup", () => {
      throw "gh is not installed";
    });
    expect(await cleanup.run("/work/app", null)).toBeNull();
    expect(spaces.warning()).toBe("Clean up merged tasks failed: gh is not installed");
    expect(log.lines()[0].text).toBe("Clean up merged tasks failed: gh is not installed");
  });
});
