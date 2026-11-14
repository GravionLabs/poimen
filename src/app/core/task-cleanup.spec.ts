import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SpacesStore } from "./spaces-store";
import { SpaceActions } from "./space-actions";
import { Machines } from "./machines";
import { RemoteSpaces } from "./remote-spaces";
import {
  TaskCleanup,
  cleanupLog,
  cleanupSummary,
  runSummary,
  type CleanupReport,
} from "./task-cleanup";
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

/** A stand-in for `RemoteSpaces`, which needs a live app: records the machines read again. */
const remote = {
  refreshed: [] as string[],
  refresh: async (name: string): Promise<void> => {
    remote.refreshed.push(name);
  },
};

describe("cleanup text per machine", () => {
  it("names the machine in the summary and on every log line", () => {
    expect(cleanupSummary(REPORT, "dev")).toBe("Clean up merged tasks on dev: 2 removed, 1 left");
    expect(cleanupLog(REPORT, "dev").map((l) => l.text)).toEqual([
      "Clean up merged tasks on dev: 2 removed, 1 left",
      "Removed /work/app-2 on dev (PR #12); branch kept: 1 commit(s) on no remote branch",
      "Left /work/app-3 on dev (feat/3-c, PR #13): it has uncommitted or untracked changes",
      "Remove it by hand on dev: git worktree remove '/work/app-3' && git branch -d 'feat/3-c'",
    ]);
  });

  it("joins one '<machine> <n> removed, <m> left' per target, failures with their reason", () => {
    expect(
      runSummary(
        [
          { target: null, report: REPORT },
          { target: "dev", report: { removed: [], left: [] } },
          { target: "box", error: "box is not connected" },
        ],
        true,
      ),
    ).toBe(
      "Clean up merged tasks: This Mac 2 removed, 1 left; dev 0 removed, 0 left; box failed: box is not connected",
    );
    expect(runSummary([{ target: null, report: REPORT }], false)).toBe(
      "Clean up merged tasks: 2 removed, 1 left",
    );
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
    remote.refreshed = [];
    tauri.answer("snapshot", () => ({ spaces: [], tabs: [], terminals: [] }));
    injector = Injector.create({
      providers: [
        { provide: UI_STORAGE, useValue: null },
        TauriBridge,
        SpacesStore,
        UiState,
        SpaceActions,
        TaskLog,
        ResultDialog,
        Machines,
        { provide: RemoteSpaces, useValue: remote },
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
    expect(await cleanup.run("/work/app", "w1")).toEqual([{ target: null, report: REPORT }]);
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
    expect(await cleanup.run("/work/app", null)).toEqual([
      { target: null, error: "gh is not installed" },
    ]);
    expect(spaces.warning()).toBe("Clean up merged tasks failed: gh is not installed");
    expect(log.lines()[0].text).toBe("Clean up merged tasks failed: gh is not installed");
  });

  describe("with machines (#284)", () => {
    const DEV_REPORT: CleanupReport = {
      removed: [
        { path: "/srv/o/app-4", branch: "feat/4-d", pr: 14, branchDeleted: true, note: null },
      ],
      left: [],
    };

    beforeEach(async () => {
      tauri.answer("list_machines", () => [
        { name: "dev", ssh: "me@dev", state: "connected", serverVersion: "1" },
        { name: "off", ssh: "me@off", state: "offline", error: "x" },
      ]);
      tauri.answer("machine_check", () => ({ version: "1", protocol: 6, checks: [] }));
      await injector.get(Machines).refresh();
    });

    it("runs on this Mac and every connected machine, never on one that is not connected", async () => {
      tauri.answer("task_cleanup", () => REPORT);
      tauri.answer("machine_task_cleanup", () => DEV_REPORT);
      await cleanup.run("/work/app", "dev::s3");
      expect(tauri.calls("task_cleanup")).toEqual([
        { dir: "/work/app", focusedSpace: null, skipTeardown: false },
      ]);
      // The focused space is the machine's own id there; the offline machine gets nothing.
      expect(tauri.calls("machine_task_cleanup")).toEqual([
        { name: "dev", dir: "/work/app", focusedSpace: "s3", skipTeardown: false },
      ]);
      expect(spaces.warning()).toBe(
        "Clean up merged tasks: This Mac 2 removed, 1 left; dev 1 removed, 0 left",
      );
      const texts = log.lines().map((l) => l.text);
      expect(texts).toContain("Clean up merged tasks: 2 removed, 1 left");
      expect(texts).toContain("Clean up merged tasks on dev: 1 removed, 0 left");
      expect(remote.refreshed).toEqual(["dev"]);
      expect(ui.panelTab()).toBe("log");
    });

    it("keeps the other results when one machine fails", async () => {
      tauri.answer("task_cleanup", () => ({ removed: [], left: [] }));
      tauri.answer("machine_task_cleanup", () => {
        throw "worktree.cleanup: conflict";
      });
      const outcomes = await cleanup.run("/work/app", null);
      expect(outcomes).toEqual([
        { target: null, report: { removed: [], left: [] } },
        { target: "dev", error: "worktree.cleanup: conflict" },
      ]);
      expect(spaces.warning()).toBe(
        "Clean up merged tasks: This Mac 0 removed, 0 left; dev failed: worktree.cleanup: conflict",
      );
      expect(log.lines().map((l) => l.text)).toContain(
        "Clean up merged tasks on dev failed: worktree.cleanup: conflict",
      );
    });

    it("asks once for the machines with an untrusted teardown and runs again only there", async () => {
      const dialog = injector.get(ResultDialog);
      tauri.answer("task_cleanup", () => REPORT);
      tauri.answer("machine_task_cleanup", (args) =>
        (args as { skipTeardown: boolean }).skipTeardown
          ? DEV_REPORT
          : { removed: [], left: [], teardownUntrusted: true },
      );
      await cleanup.run("/work/app", null);
      expect(spaces.warning()).toBe("Clean up merged tasks: This Mac 2 removed, 1 left");
      expect(dialog.content()?.summary).toContain("(dev)");
      dialog.runAction();
      await vi.waitFor(() => expect(tauri.calls("machine_task_cleanup")).toHaveLength(2));
      expect(tauri.calls("machine_task_cleanup")[1]).toMatchObject({
        name: "dev",
        skipTeardown: true,
      });
      expect(tauri.calls("task_cleanup")).toHaveLength(1);
      await vi.waitFor(() =>
        expect(spaces.warning()).toBe("Clean up merged tasks: dev 1 removed, 0 left"),
      );
    });
  });
});
