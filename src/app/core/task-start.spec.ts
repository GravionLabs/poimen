import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TaskPicker, type TaskChoice } from "../task-dialog/task-picker";
import { SpacesStore } from "./spaces-store";
import { Machines } from "./machines";
import { RemoteSpaces } from "./remote-spaces";
import { SpaceActions } from "./space-actions";
import { TaskStarter, remoteStepFailure, stepFailure, type TaskPlan } from "./task-start";
import { qualifyId, type Snapshot } from "./snapshot";
import { TauriBridge } from "./tauri";
import { UI_STORAGE, UiState } from "./ui-state";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

/** The spaces read from machine `dev` (a stand-in for `RemoteSpaces`, which needs a live app). */
let remoteSpaces: Snapshot["spaces"] = [];
const fakeRemote = {
  refresh: async (): Promise<void> => {},
  snapshot: () => ({ spaces: remoteSpaces, tabs: [], terminals: [] }),
};

const CHOICE: TaskChoice = {
  number: 12,
  title: "Add picker",
  url: "https://github.com/o/r/issues/12",
  mode: "agent",
};
const ISSUE = { nr: 12, title: "Add picker", url: "https://github.com/o/r/issues/12" };

function plan(over: Partial<TaskPlan> = {}): TaskPlan {
  return {
    worktree: true,
    command: "claude 'x'",
    label: "#12 Add picker",
    tab: "#12 agent",
    ...over,
  };
}

describe("TaskStarter", () => {
  let starter: TaskStarter;
  let spaces: SpaceActions;
  let ui: UiState;

  beforeEach(() => {
    tauri.reset();
    remoteSpaces = [];
    tauri.answer("snapshot", () => ({ spaces: [], tabs: [], terminals: [] }));
    tauri.answer("task_plan", () => plan());
    tauri.answer("task_worktree", () => ({
      path: "/work/app-12",
      branch: "feat/12-x",
      created: true,
    }));
    tauri.answer("create_space", () => ({ space_id: "w2", created: true }));
    tauri.answer("task_tab", () => undefined);
    const injector = Injector.create({
      providers: [
        TauriBridge,
        SpacesStore,
        Machines,
        { provide: RemoteSpaces, useFactory: () => fakeRemote },
        { provide: UI_STORAGE, useValue: null },
        UiState,
        SpaceActions,
        TaskPicker,
        TaskStarter,
      ],
    });
    starter = injector.get(TaskStarter);
    spaces = injector.get(SpaceActions);
    ui = injector.get(UiState);
  });

  it("creates the worktree, then the space on its path with label and command", async () => {
    expect(await starter.run("/work/app", CHOICE)).toBe(true);
    expect(tauri.calls("task_plan")).toEqual([
      { repoDir: "/work/app", issue: ISSUE, mode: "agent" },
    ]);
    expect(tauri.calls("task_worktree")).toEqual([
      { repoDir: "/work/app", issue: ISSUE, mode: "agent" },
    ]);
    expect(tauri.calls("create_space")).toEqual([
      { cwd: "/work/app-12", label: "#12 Add picker", decision: null, command: "claude 'x'" },
    ]);
    expect(tauri.calls("task_tab")).toEqual([]);
  });

  it("an existing space is focused by create_space and nothing else runs", async () => {
    tauri.answer("create_space", () => ({ space_id: "w1", created: false }));
    expect(await starter.run("/work/app", CHOICE)).toBe(true);
    expect(tauri.calls("create_space")).toHaveLength(1);
    expect(tauri.calls("task_tab")).toEqual([]);
    expect(spaces.warning()).toBe(`Opened the existing space "w1".`);
  });

  it("a mode without a worktree runs the command in a tab of the repository's space", async () => {
    tauri.answer("task_plan", () =>
      plan({ worktree: false, command: "sh -c 'x'", tab: "#12 plain" }),
    );
    expect(await starter.run("/work/app", { ...CHOICE, mode: "plain" })).toBe(true);
    expect(tauri.calls("task_worktree")).toEqual([]);
    expect(tauri.calls("create_space")).toEqual([
      { cwd: "/work/app", label: null, decision: null, command: null },
    ]);
    expect(tauri.calls("task_tab")).toEqual([
      { spaceId: "w2", dir: "/work/app", name: "#12 plain", command: "sh -c 'x'" },
    ]);
  });

  it("a number typed by hand has no title or url", async () => {
    await starter.run("/work/app", { number: 5, title: null, url: null, mode: "agent" });
    expect(tauri.calls("task_plan")[0]?.["issue"]).toEqual({ nr: 5, title: "", url: "" });
  });

  it("a failed mode lookup names the step and starts nothing", async () => {
    tauri.answer("task_plan", () => {
      throw 'unknown task mode "x"';
    });
    expect(await starter.run("/work/app", CHOICE)).toBe(false);
    expect(spaces.warning()).toBe('Start task: task mode failed: unknown task mode "x"');
    expect(tauri.calls("task_worktree")).toEqual([]);
  });

  it("a failed worktree names the step and creates no space", async () => {
    tauri.answer("task_worktree", () => {
      throw "git: path exists";
    });
    expect(await starter.run("/work/app", CHOICE)).toBe(false);
    expect(spaces.warning()).toBe("Start task: git worktree failed: git: path exists");
    expect(tauri.calls("create_space")).toEqual([]);
  });

  it("a failed space names the step", async () => {
    tauri.answer("create_space", () => {
      throw "herdr: boom";
    });
    expect(await starter.run("/work/app", CHOICE)).toBe(false);
    expect(spaces.warning()).toBe("Start task: space failed: herdr: boom");
  });

  it("a failed tab names the step", async () => {
    tauri.answer("task_plan", () => plan({ worktree: false }));
    tauri.answer("task_tab", () => {
      throw "herdr: no tab";
    });
    expect(await starter.run("/work/app", CHOICE)).toBe(false);
    expect(spaces.warning()).toBe("Start task: terminal tab failed: herdr: no tab");
  });

  it("an untrusted project holds the task: the answer creates the space with the command", async () => {
    const request = { root: "/work/app-12", files: ["f"], hash: "h1", commands: [] };
    tauri.answer("create_space", (args) =>
      args?.["decision"]
        ? { space_id: "w2", created: true }
        : { space_id: "", created: false, trust: request },
    );
    expect(await starter.run("/work/app", CHOICE)).toBe(true);
    expect(spaces.warning()).toBeNull();
    await spaces.answerTrust("trust");
    expect(tauri.calls("create_space")[1]).toEqual({
      cwd: "/work/app-12",
      label: "#12 Add picker",
      decision: { kind: "trust", hash: "h1" },
      command: "claude 'x'",
    });
  });

  it("an untrusted project holds a tab task: the answer creates the space, then the tab", async () => {
    tauri.answer("task_plan", () => plan({ worktree: false, tab: "#12 agent" }));
    const request = { root: "/work/app", files: ["f"], hash: "h1", commands: [] };
    tauri.answer("create_space", (args) =>
      args?.["decision"]
        ? { space_id: "w2", created: true }
        : { space_id: "", created: false, trust: request },
    );
    expect(await starter.run("/work/app", CHOICE)).toBe(true);
    expect(tauri.calls("task_tab")).toEqual([]);
    await spaces.answerTrust("without");
    expect(tauri.calls("task_tab")).toEqual([
      { spaceId: "w2", dir: "/work/app", name: "#12 agent", command: "claude 'x'" },
    ]);
  });

  describe("on a machine", () => {
    const REMOTE: TaskChoice = { ...CHOICE, machine: "dev" };

    beforeEach(() => {
      tauri.answer("github_repo", () => ({ owner: "o", name: "r" }));
      tauri.answer("machine_repo_ensure", () => ({ path: "/srv/o/r", cloned: true }));
      tauri.answer("machine_worktree_create", () => ({
        path: "/srv/o/r-12",
        branch: "feat/12-x",
        created: true,
      }));
      tauri.answer("machine_create_space", () => ({ space_id: "s3", created: true }));
      tauri.answer("machine_task_tab", () => undefined);
    });

    it("ensures the clone, creates the worktree and the space on the machine and focuses it", async () => {
      expect(await starter.run("/work/app", REMOTE)).toBe(true);
      expect(tauri.calls("machine_repo_ensure")).toEqual([{ name: "dev", owner: "o", repo: "r" }]);
      expect(tauri.calls("machine_worktree_create")).toEqual([
        { name: "dev", repoDir: "/srv/o/r", issue: ISSUE, mode: "agent" },
      ]);
      expect(tauri.calls("machine_create_space")).toEqual([
        { name: "dev", cwd: "/srv/o/r-12", label: "#12 Add picker", command: "claude 'x'" },
      ]);
      expect(tauri.calls("machine_task_tab")).toEqual([]);
      expect(tauri.calls("task_worktree")).toEqual([]);
      expect(tauri.calls("create_space")).toEqual([]);
      expect(ui.splitWorkspace()).toBe(qualifyId("dev", "s3"));
    });

    it("a mode without a worktree runs in a tab at the clone root", async () => {
      tauri.answer("task_plan", () =>
        plan({ worktree: false, command: "sh -c 'x'", tab: "#12 plain" }),
      );
      expect(await starter.run("/work/app", { ...REMOTE, mode: "plain" })).toBe(true);
      expect(tauri.calls("machine_worktree_create")).toEqual([]);
      expect(tauri.calls("machine_create_space")).toEqual([
        { name: "dev", cwd: "/srv/o/r", label: null, command: null },
      ]);
      expect(tauri.calls("machine_task_tab")).toEqual([
        { name: "dev", spaceId: "s3", dir: "/srv/o/r", tab: "#12 plain", command: "sh -c 'x'" },
      ]);
    });

    it("an existing space is focused and says so", async () => {
      tauri.answer("machine_create_space", () => ({ space_id: "s7", created: false }));
      remoteSpaces = [
        { space_id: qualifyId("dev", "s7"), label: "#12 Add picker" } as Snapshot["spaces"][number],
      ];
      expect(await starter.run("/work/app", REMOTE)).toBe(true);
      expect(ui.splitWorkspace()).toBe(qualifyId("dev", "s7"));
      expect(spaces.warning()).toBe('Opened the existing space "#12 Add picker" on dev.');
      expect(tauri.calls("machine_task_tab")).toEqual([]);
    });

    it("an offline machine fails at the clone step, naming it, and creates nothing", async () => {
      tauri.answer("machine_repo_ensure", () => {
        throw "machine dev is offline";
      });
      expect(await starter.run("/work/app", REMOTE)).toBe(false);
      expect(spaces.warning()).toBe("Start task on dev: clone failed: machine dev is offline");
      expect(tauri.calls("machine_worktree_create")).toEqual([]);
      expect(tauri.calls("machine_create_space")).toEqual([]);
    });

    it("a failed clone names the machine and the step and creates no space", async () => {
      tauri.answer("machine_repo_ensure", () => {
        throw "git clone: permission denied";
      });
      expect(await starter.run("/work/app", REMOTE)).toBe(false);
      expect(spaces.warning()).toBe(
        "Start task on dev: clone failed: git clone: permission denied",
      );
      expect(tauri.calls("machine_create_space")).toEqual([]);
    });

    it("a repository without a GitHub origin cannot be cloned", async () => {
      tauri.answer("github_repo", () => null);
      expect(await starter.run("/work/app", REMOTE)).toBe(false);
      expect(spaces.warning()).toContain("Start task on dev: clone failed");
      expect(tauri.calls("machine_repo_ensure")).toEqual([]);
    });

    it("a failed worktree or space names the step and stops", async () => {
      tauri.answer("machine_worktree_create", () => {
        throw "path exists";
      });
      expect(await starter.run("/work/app", REMOTE)).toBe(false);
      expect(spaces.warning()).toBe("Start task on dev: git worktree failed: path exists");
      expect(tauri.calls("machine_create_space")).toEqual([]);
      tauri.answer("machine_worktree_create", () => ({ path: "/p", branch: "b", created: true }));
      tauri.answer("machine_create_space", () => {
        throw "not trusted";
      });
      expect(await starter.run("/work/app", REMOTE)).toBe(false);
      expect(spaces.warning()).toBe("Start task on dev: space failed: not trusted");
    });

    it("a failed tab names the step", async () => {
      tauri.answer("task_plan", () => plan({ worktree: false }));
      tauri.answer("machine_task_tab", () => {
        throw "no tab";
      });
      expect(await starter.run("/work/app", REMOTE)).toBe(false);
      expect(spaces.warning()).toBe("Start task on dev: terminal tab failed: no tab");
    });
  });

  it("does nothing when the picker is cancelled", async () => {
    const injector = Injector.create({
      providers: [
        TauriBridge,
        SpacesStore,
        Machines,
        { provide: RemoteSpaces, useFactory: () => fakeRemote },
        { provide: UI_STORAGE, useValue: null },
        UiState,
        SpaceActions,
        TaskPicker,
        TaskStarter,
      ],
    });
    const picker = injector.get(TaskPicker);
    const started = injector.get(TaskStarter).start("/work/app");
    picker.cancel();
    expect(await started).toBe(false);
    expect(tauri.calls("task_plan")).toEqual([]);
  });
});

describe("remoteStepFailure", () => {
  it("names the machine in front of the step", () => {
    expect(remoteStepFailure("dev", "clone", "no")).toBe("Start task on dev: clone failed: no");
  });
});

describe("stepFailure", () => {
  it("reads the message of an error object", () => {
    expect(stepFailure("space", { message: "no" })).toBe("Start task: space failed: no");
  });
});
