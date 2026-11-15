// @vitest-environment jsdom
import { TestBed, getTestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { SpaceActions } from "../core/space-actions";
import { TaskStarter } from "../core/task-start";
import { tauri } from "../core/tauri.mock";
import { IssueTasks } from "./issue-task";

vi.mock("@tauri-apps/api/core", async () => ({
  invoke: (await import("../core/tauri.mock")).tauri.invoke,
}));
vi.mock("@tauri-apps/api/event", async () => ({
  listen: (await import("../core/tauri.mock")).tauri.listen,
}));

beforeAll(() => {
  getTestBed().initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
});

describe("IssueTasks", () => {
  let starter: { start: ReturnType<typeof vi.fn> };
  let spaces: { create: ReturnType<typeof vi.fn> };

  beforeEach(() => {
    TestBed.resetTestingModule();
    tauri.reset();
    starter = { start: vi.fn(async () => true) };
    spaces = { create: vi.fn(async () => true) };
    TestBed.configureTestingModule({
      providers: [
        { provide: TaskStarter, useValue: starter },
        { provide: SpaceActions, useValue: spaces },
      ],
    });
  });

  function service(): IssueTasks {
    return TestBed.inject(IssueTasks);
  }

  it("opens the task picker of the repository preset to the issue", async () => {
    tauri.answer("worktree_branches", () => []);
    await service().load(["/src/n"]);
    await service().start("/src/n", 7);
    expect(starter.start).toHaveBeenCalledWith("/src/n", 7);
    expect(spaces.create).not.toHaveBeenCalled();
  });

  it("offers Start task… without a task worktree of the issue", async () => {
    tauri.answer("worktree_branches", () => [{ path: "/src/n-70", branch: "feat/70-other" }]);
    await service().load(["/src/n"]);
    expect(service().label("/src/n", 7)).toBe("Start task…");
  });

  it("offers Open space and focuses the space of an existing task worktree", async () => {
    tauri.answer("worktree_branches", () => [{ path: "/src/n-7", branch: "feat/7-login" }]);
    await service().load(["/src/n"]);
    expect(service().label("/src/n", 7)).toBe("Open space");
    await service().start("/src/n", 7);
    expect(spaces.create).toHaveBeenCalledWith("/src/n-7", "#7");
    expect(starter.start).not.toHaveBeenCalled();
  });

  it("treats a failed worktree read as no worktrees", async () => {
    tauri.answer("worktree_branches", () => {
      throw new Error("not a repository");
    });
    await service().load(["/src/n"]);
    expect(service().label("/src/n", 7)).toBe("Start task…");
  });

  it("does not match a detached worktree (no branch)", async () => {
    tauri.answer("worktree_branches", () => [{ path: "/src/n-d", branch: null }]);
    await service().load(["/src/n"]);
    expect(service().worktreeOf("/src/n", 7)).toBeNull();
  });
});
