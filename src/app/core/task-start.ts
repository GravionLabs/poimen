import { Injectable, inject } from "@angular/core";
import { TaskPicker, type TaskChoice } from "../task-dialog/task-picker";
import { HerdrStore } from "./herdr-store";
import { SpaceActions, type SpaceOutcome } from "./space-actions";
import { TauriBridge } from "./tauri";

/** The steps of starting a task; a failure names the one that failed. */
export type TaskStep = "mode" | "worktree" | "space" | "tab";

const STEP_NAMES: Record<TaskStep, string> = {
  mode: "task mode",
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

  /** Opens the picker for the repository at `dir`; resolves to whether a task was started. */
  async start(dir: string): Promise<boolean> {
    const choice = await this.picker.open(dir);
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
    if (plan.worktree) return this.inWorktree(dir, issue, choice.mode, plan);
    return this.inTab(dir, plan);
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
