import { Injectable, inject, signal } from "@angular/core";
import { SpaceActions } from "../core/space-actions";
import { TaskStarter } from "../core/task-start";
import { TauriBridge } from "../core/tauri";
import type { WorktreeInfo } from "./pulls";

/**
 * The task of an open issue: "Start task…" runs the task picker preset to the issue, or, when a
 * task worktree of the issue exists, "Open space" focuses or creates its space instead. The
 * worktree is the one whose branch is `<type>/<nr>-…`, the default branch template of a task mode.
 */
@Injectable({ providedIn: "root" })
export class IssueTasks {
  private readonly bridge = inject(TauriBridge);
  private readonly starter = inject(TaskStarter);
  private readonly spaces = inject(SpaceActions);
  private readonly _worktrees = signal<ReadonlyMap<string, readonly WorktreeInfo[]>>(new Map());

  /** Reads the worktrees of the repositories at `roots`; a failure only leaves them blank. */
  async load(roots: readonly string[]): Promise<void> {
    const found = new Map<string, readonly WorktreeInfo[]>();
    await Promise.all(
      [...new Set(roots)].map(async (root) => {
        try {
          found.set(
            root,
            await this.bridge.invoke<WorktreeInfo[]>("worktree_branches", { dir: root }),
          );
        } catch {
          // No worktree marks for this repository.
        }
      }),
    );
    this._worktrees.set(found);
  }

  /** The task worktree of issue `nr` in the repository at `root`, if one exists. */
  worktreeOf(root: string, nr: number): WorktreeInfo | null {
    const pattern = new RegExp(`^[^/]+/${nr}-`);
    return (
      (this._worktrees().get(root) ?? []).find(
        (w) => w.branch !== null && pattern.test(w.branch),
      ) ?? null
    );
  }

  /** The menu label of the issue: "Open space" with a task worktree, else "Start task…". */
  label(root: string, nr: number): string {
    return this.worktreeOf(root, nr) ? "Open space" : "Start task…";
  }

  /** Opens the space of the task worktree, or the task picker preset to the issue. */
  async start(root: string, nr: number): Promise<void> {
    const wt = this.worktreeOf(root, nr);
    if (wt) await this.spaces.create(wt.path, `#${nr}`);
    else await this.starter.start(root, nr);
  }
}
