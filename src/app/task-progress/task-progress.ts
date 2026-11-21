import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { TaskStarter } from "../core/task-start";

/** The steps of a task that creates a space with a worktree, the current one marked (#1041). */
@Component({
  selector: "app-task-progress",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./task-progress.html",
  styles: `
    .progress {
      position: fixed;
      left: 16px;
      bottom: 40px;
      z-index: 20;
      margin: 0;
      padding: 8px 12px;
      list-style: none;
      background: var(--h-content-background);
      color: var(--h-text-color);
      border: 1px solid color-mix(in srgb, var(--h-text-color) 30%, transparent);
      border-radius: 6px;
      font-size: 12px;
    }
    li {
      display: flex;
      gap: 6px;
      padding: 2px 0;
      opacity: 0.7;
    }
    li.current {
      opacity: 1;
      font-weight: 600;
    }
  `,
})
export class TaskProgress {
  protected readonly starter = inject(TaskStarter);
  protected mark(state: string): string {
    return state === "done" ? "✓" : state === "current" ? "▸" : state === "skipped" ? "–" : "○";
  }
}
