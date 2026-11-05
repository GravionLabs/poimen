import { Injector } from "@angular/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { TaskPicker, type TaskChoice } from "../task-dialog/task-picker";
import { HerdrStore } from "./herdr-store";
import { SpaceActions } from "./space-actions";
import { TaskStarter, stepFailure, type TaskPlan } from "./task-start";
import { TauriBridge } from "./tauri";
import { tauri } from "./tauri.mock";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("./tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("./tauri.mock")).tauri.listen,
}));

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

  beforeEach(() => {
    tauri.reset();
    tauri.answer("snapshot", () => ({ spaces: [], tabs: [], terminals: [] }));
    tauri.answer("task_plan", () => plan());
    tauri.answer("task_worktree", () => ({
      path: "/work/app-12",
      branch: "feat/12-x",
      created: true,
    }));
    tauri.answer("create_space", () => ({ workspace_id: "w2", created: true }));
    tauri.answer("task_tab", () => undefined);
    const injector = Injector.create({
      providers: [TauriBridge, HerdrStore, SpaceActions, TaskPicker, TaskStarter],
    });
    starter = injector.get(TaskStarter);
    spaces = injector.get(SpaceActions);
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
    tauri.answer("create_space", () => ({ workspace_id: "w1", created: false }));
    expect(await starter.run("/work/app", CHOICE)).toBe(true);
    expect(tauri.calls("create_space")).toHaveLength(1);
    expect(tauri.calls("task_tab")).toEqual([]);
    expect(spaces.warning()).toBeNull();
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
      { workspaceId: "w2", dir: "/work/app", name: "#12 plain", command: "sh -c 'x'" },
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
        ? { workspace_id: "w2", created: true }
        : { workspace_id: "", created: false, trust: request },
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
        ? { workspace_id: "w2", created: true }
        : { workspace_id: "", created: false, trust: request },
    );
    expect(await starter.run("/work/app", CHOICE)).toBe(true);
    expect(tauri.calls("task_tab")).toEqual([]);
    await spaces.answerTrust("without");
    expect(tauri.calls("task_tab")).toEqual([
      { workspaceId: "w2", dir: "/work/app", name: "#12 agent", command: "claude 'x'" },
    ]);
  });

  it("does nothing when the picker is cancelled", async () => {
    const injector = Injector.create({
      providers: [TauriBridge, HerdrStore, SpaceActions, TaskPicker, TaskStarter],
    });
    const picker = injector.get(TaskPicker);
    const started = injector.get(TaskStarter).start("/work/app");
    picker.cancel();
    expect(await started).toBe(false);
    expect(tauri.calls("task_plan")).toEqual([]);
  });
});

describe("stepFailure", () => {
  it("reads the message of an error object", () => {
    expect(stepFailure("space", { message: "no" })).toBe("Start task: space failed: no");
  });
});
