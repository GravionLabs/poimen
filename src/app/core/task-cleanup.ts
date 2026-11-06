import { Injectable, inject } from "@angular/core";
import { HerdrStore } from "./herdr-store";
import { ResultDialog } from "./result-dialog";
import { SpaceActions } from "./space-actions";
import { TaskLog } from "./task-log";
import { TauriBridge } from "./tauri";
import { UiState } from "./ui-state";

/** A worktree the cleanup removed (`Removed` of `worktree.rs`). */
export interface CleanupRemoved {
  path: string;
  branch: string;
  pr: number;
  branchDeleted: boolean;
  note: string | null;
}

/** A worktree the cleanup left (`Left` of `worktree.rs`). */
export interface CleanupLeft {
  path: string;
  branch: string;
  pr: number;
  reason: string;
  command: string;
}

/** What `task_cleanup` returns. */
export interface CleanupReport {
  removed: CleanupRemoved[];
  left: CleanupLeft[];
  /** The project has a teardown but is not trusted: nothing was removed. */
  teardownUntrusted?: boolean;
}

/** The result notification: `<n> removed, <m> left`. */
export function cleanupSummary(report: CleanupReport): string {
  return `Clean up merged tasks: ${report.removed.length} removed, ${report.left.length} left`;
}

/** The lines for the Log tab: the summary, kept branches, and each left worktree with its command. */
export function cleanupLog(report: CleanupReport): { text: string; detail?: boolean }[] {
  const lines: { text: string; detail?: boolean }[] = [{ text: cleanupSummary(report) }];
  for (const r of report.removed) {
    if (r.note) lines.push({ text: `Removed ${r.path} (PR #${r.pr}); ${r.note}` });
  }
  for (const l of report.left) {
    lines.push({ text: `Left ${l.path} (${l.branch}, PR #${l.pr}): ${l.reason}` });
    lines.push({ text: `Remove it by hand: ${l.command}`, detail: true });
  }
  return lines;
}

/**
 * "Clean up merged tasks": removes the worktrees, spaces and local branches of the repository
 * whose pull request is merged (`task_cleanup`, see `worktree.rs` for what is never touched). A
 * user action only; the result is a notice, the details are in the Log tab of the bottom panel.
 */
@Injectable({ providedIn: "root" })
export class TaskCleanup {
  private readonly bridge = inject(TauriBridge);
  private readonly spaces = inject(SpaceActions);
  private readonly store = inject(HerdrStore);
  private readonly log = inject(TaskLog);
  private readonly ui = inject(UiState);
  private readonly dialog = inject(ResultDialog);

  /** Cleans up the repository at `dir`; `focusedSpace` is never touched. Resolves to the report. */
  async run(
    dir: string,
    focusedSpace: string | null,
    skipTeardown = false,
  ): Promise<CleanupReport | null> {
    let report: CleanupReport;
    try {
      report = await this.bridge.invoke<CleanupReport>("task_cleanup", {
        dir,
        focusedSpace,
        skipTeardown,
      });
    } catch (err) {
      const reason = err && typeof err === "object" && "message" in err ? err.message : err;
      const text = `Clean up merged tasks failed: ${String(reason)}`;
      this.log.add([{ text }]);
      this.spaces.notify(text);
      return null;
    }
    if (report.teardownUntrusted) {
      this.dialog.show({
        title: "Clean up merged tasks",
        summary:
          "The project file has a teardown command but is not trusted, so it cannot run. Nothing was removed. Remove the merged worktrees without running the teardown?",
        text: "",
        action: {
          label: "Remove without teardown",
          run: () => void this.run(dir, focusedSpace, true),
        },
      });
      return report;
    }
    await this.store.refresh();
    this.log.add(cleanupLog(report));
    if (report.left.length > 0) this.ui.selectPanelTab("log");
    this.spaces.notify(cleanupSummary(report));
    return report;
  }
}
