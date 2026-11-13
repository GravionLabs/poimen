import { Injectable, inject } from "@angular/core";
import { TaskPicker, type TaskChoice } from "../task-dialog/task-picker";
import { HerdrStore } from "./herdr-store";
import { qualifyId } from "./herdr";
import { RemoteSpaces } from "./remote-spaces";
import { SpaceActions, type SpaceOutcome } from "./space-actions";
import { TauriBridge } from "./tauri";
import { UiState } from "./ui-state";

/** The steps of starting a task; a failure names the one that failed. */
export type TaskStep = "mode" | "clone" | "worktree" | "space" | "tab";

const STEP_NAMES: Record<TaskStep, string> = {
  mode: "task mode",
  clone: "clone",
  worktree: "git worktree",
  space: "space",
  tab: "terminal tab",
};

/** What `task_plan` returns (`Plan` of `task_start.rs`). */
export interface TaskPlan {
  worktree: boolean;
  command: string;
  label: string;
  tab: string;
}

/** What `task_worktree` returns. */
export interface TaskWorktree {
  path: string;
  branch: string;
  created: boolean;
}

/** The message of a failed step: names the step and gives the reason. */
export function stepFailure(step: TaskStep, reason: unknown): string {
  const text =
    reason && typeof reason === "object" && "message" in reason
      ? String((reason as { message: unknown }).message)
      : String(reason);
  return `Start task: ${STEP_NAMES[step]} failed: ${text}`;
}

/** The message of a failed step of a task on `machine`: names the machine and the step. */
export function remoteStepFailure(machine: string, step: TaskStep, reason: unknown): string {
  return stepFailure(step, reason).replace("Start task:", `Start task on ${machine}:`);
}

/**
 * Starts a task end to end: the picker, then for a mode with a worktree the worktree and a space
 * on it that runs the mode's command in its root pane (an existing space is only focused), else a
 * tab with the command in the space of the repository. A failed step shows a notice naming it and
 * stops: a failed worktree creates no space.
 */
@Injectable({ providedIn: "root" })
export class TaskStarter {
  private readonly bridge = inject(TauriBridge);
  private readonly picker = inject(TaskPicker);
  private readonly spaces = inject(SpaceActions);
  private readonly store = inject(HerdrStore);
  private readonly remote = inject(RemoteSpaces);
  private readonly ui = inject(UiState);

  /** Opens the picker for the repository at `dir` (issue `preselect` highlighted); resolves to whether a task was started. */
  async start(dir: string, preselect?: number): Promise<boolean> {
    const choice = await this.picker.open(dir, preselect);
    return choice ? this.run(dir, choice) : false;
  }

  /** Runs the steps for a choice already made. */
  async run(dir: string, choice: TaskChoice): Promise<boolean> {
    const issue = { nr: choice.number, title: choice.title ?? "", url: choice.url ?? "" };
    let plan: TaskPlan;
    try {
      plan = await this.bridge.invoke<TaskPlan>("task_plan", {
        repoDir: dir,
        issue,
        mode: choice.mode,
      });
    } catch (err) {
      return this.fail("mode", err);
    }
    if (choice.machine) return this.onMachine(choice.machine, dir, issue, choice.mode, plan);
    if (plan.worktree) return this.inWorktree(dir, issue, choice.mode, plan);
    return this.inTab(dir, plan);
  }

  /**
   * The task on a machine: its clone (`repo.ensure`), then the worktree of a mode with one (else
   * the clone root), then the space (an existing one is only focused) and, without a worktree, the
   * tab with the command. Every step runs on the machine's server; a failed step names the
   * machine and creates nothing after it. The issue list stays local (`dir` is the local clone).
   */
  private async onMachine(
    machine: string,
    dir: string,
    issue: { nr: number; title: string; url: string },
    mode: string,
    plan: TaskPlan,
  ): Promise<boolean> {
    const fail = (step: TaskStep, reason: unknown): false => {
      this.spaces.notify(remoteStepFailure(machine, step, reason));
      return false;
    };
    let cwd: string;
    try {
      const repo = await this.bridge.invoke<{ owner: string; name: string } | null>("github_repo", {
        dir,
      });
      if (!repo) throw new Error("the repository has no GitHub origin to clone from");
      const ensured = await this.bridge.invoke<{ path: string }>("machine_repo_ensure", {
        name: machine,
        owner: repo.owner,
        repo: repo.name,
      });
      cwd = ensured.path;
    } catch (err) {
      return fail("clone", err);
    }
    if (plan.worktree) {
      try {
        const worktree = await this.bridge.invoke<TaskWorktree>("machine_worktree_create", {
          name: machine,
          repoDir: cwd,
          issue,
          mode,
        });
        cwd = worktree.path;
      } catch (err) {
        return fail("worktree", err);
      }
    }
    let outcome: SpaceOutcome;
    try {
      outcome = await this.bridge.invoke<SpaceOutcome>("machine_create_space", {
        name: machine,
        cwd,
        label: plan.worktree ? plan.label : null,
        command: plan.worktree ? plan.command : null,
      });
    } catch (err) {
      return fail("space", err);
    }
    if (!plan.worktree) {
      try {
        await this.bridge.invoke("machine_task_tab", {
          name: machine,
          workspaceId: outcome.workspace_id,
          dir: cwd,
          tab: plan.tab,
          command: plan.command,
        });
      } catch (err) {
        await this.remote.refresh(machine);
        return fail("tab", err);
      }
    }
    await this.remote.refresh(machine);
    this.ui.setSplitWorkspace(qualifyId(machine, outcome.workspace_id));
    if (!outcome.created) {
      const label = this.remote
        .snapshot()
        .spaces.find((s) => s.space_id === qualifyId(machine, outcome.workspace_id))?.label;
      this.spaces.notify(
        `Opened the existing space "${label ?? outcome.workspace_id}" on ${machine}.`,
      );
    }
    return true;
  }

  private async inWorktree(
    dir: string,
    issue: { nr: number; title: string; url: string },
    mode: string,
    plan: TaskPlan,
  ): Promise<boolean> {
    let worktree: TaskWorktree;
    try {
      worktree = await this.bridge.invoke<TaskWorktree>("task_worktree", {
        repoDir: dir,
        issue,
        mode,
      });
    } catch (err) {
      return this.fail("worktree", err);
    }
    const outcome = await this.spaces.create(worktree.path, plan.label, null, plan.command);
    if (outcome) return true;
    // Held for the trust question: the answer creates the space with the same command.
    if (this.spaces.trust()) return true;
    return this.fail("space", this.spaces.error() ?? "the space was not created");
  }

  private async inTab(dir: string, plan: TaskPlan): Promise<boolean> {
    // The tab is created by `after`, also when the trust answer creates the space later.
    let tabFailed = false;
    const after = async (outcome: SpaceOutcome): Promise<void> => {
      try {
        await this.bridge.invoke("task_tab", {
          workspaceId: outcome.workspace_id,
          dir,
          name: plan.tab,
          command: plan.command,
        });
      } catch (err) {
        tabFailed = true;
        this.fail("tab", err);
        return;
      }
      await this.store.refresh();
    };
    const outcome = await this.spaces.create(dir, "", null, null, after);
    if (outcome) return !tabFailed;
    // Held for the trust question: the answer creates the space, then the tab.
    if (this.spaces.trust()) return true;
    return this.fail("space", this.spaces.error() ?? "the space was not created");
  }

  private fail(step: TaskStep, reason: unknown): false {
    this.spaces.notify(stepFailure(step, reason));
    return false;
  }
}
