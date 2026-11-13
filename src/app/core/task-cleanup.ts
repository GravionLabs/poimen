import { Injectable, inject } from "@angular/core";
import { splitId } from "./herdr";
import { HerdrStore } from "./herdr-store";
import { Machines } from "./machines";
import { RemoteSpaces } from "./remote-spaces";
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

/** Where a cleanup runs: this Mac (`null`) or a connected machine by name (#284). */
export type CleanupTarget = string | null;

/** The name of a target in the messages. */
export const targetName = (target: CleanupTarget): string => target ?? "This Mac";

/** ` on <machine>` after the action's name, nothing for this Mac. */
const onText = (target: CleanupTarget): string => (target === null ? "" : ` on ${target}`);

/** `<n> removed, <m> left` of one report. */
const counts = (report: CleanupReport): string =>
  `${report.removed.length} removed, ${report.left.length} left`;

/** The result notification: `<n> removed, <m> left`, `on <machine>` for a machine's report. */
export function cleanupSummary(report: CleanupReport, target: CleanupTarget = null): string {
  return `Clean up merged tasks${onText(target)}: ${counts(report)}`;
}

/** The lines for the Log tab: the summary, kept branches, and each left worktree with its command. */
export function cleanupLog(
  report: CleanupReport,
  target: CleanupTarget = null,
): { text: string; detail?: boolean }[] {
  const on = onText(target);
  const lines: { text: string; detail?: boolean }[] = [{ text: cleanupSummary(report, target) }];
  for (const r of report.removed) {
    if (r.note) lines.push({ text: `Removed ${r.path}${on} (PR #${r.pr}); ${r.note}` });
  }
  for (const l of report.left) {
    lines.push({ text: `Left ${l.path}${on} (${l.branch}, PR #${l.pr}): ${l.reason}` });
    lines.push({ text: `Remove it by hand${on}: ${l.command}`, detail: true });
  }
  return lines;
}

/** The outcome of one target: its report, or why it failed. */
export type TargetOutcome =
  { target: CleanupTarget; report: CleanupReport } | { target: CleanupTarget; error: string };

/**
 * The one notification of a run: one `<machine> <n> removed, <m> left` (or `<machine> failed:
 * <reason>`) per target when the run had a machine (`named`), else the text of this Mac alone.
 */
export function runSummary(outcomes: readonly TargetOutcome[], named: boolean): string {
  if (!named && outcomes.length === 1) {
    const only = outcomes[0];
    return "report" in only
      ? cleanupSummary(only.report)
      : `Clean up merged tasks failed: ${only.error}`;
  }
  const parts = outcomes.map((o) =>
    "report" in o
      ? `${targetName(o.target)} ${counts(o.report)}`
      : `${targetName(o.target)} failed: ${o.error}`,
  );
  return `Clean up merged tasks: ${parts.join("; ")}`;
}

/** The id of `focusedSpace` on `target`: a qualified id belongs to its machine only. */
function focusedOn(focusedSpace: string | null, target: CleanupTarget): string | null {
  if (focusedSpace === null) return null;
  const { machine, id } = splitId(focusedSpace);
  return (machine ?? null) === target ? id : null;
}

const reasonOf = (err: unknown): string =>
  String(err && typeof err === "object" && "message" in err ? err.message : err);

/**
 * "Clean up merged tasks": removes the worktrees, spaces and local branches of the repository
 * whose pull request is merged (`task_cleanup`, see `worktree.rs` for what is never touched), on
 * this Mac and on every connected machine (`machine_task_cleanup`, #284: the merged pull requests
 * come from the local `gh`, the git work runs on the machine's server in its clone). A user
 * action only; the result is one notice with a part per machine, the details are in the Log tab
 * of the bottom panel.
 */
@Injectable({ providedIn: "root" })
export class TaskCleanup {
  private readonly bridge = inject(TauriBridge);
  private readonly spaces = inject(SpaceActions);
  private readonly store = inject(HerdrStore);
  private readonly log = inject(TaskLog);
  private readonly ui = inject(UiState);
  private readonly dialog = inject(ResultDialog);
  private readonly machines = inject(Machines);
  private readonly remote = inject(RemoteSpaces);

  /**
   * Cleans up the repository at `dir` (a local clone) on this Mac and the connected machines, or
   * only on `only`; `focusedSpace` (an id of the merged snapshot) is never touched. Resolves to
   * the outcome per target.
   */
  async run(
    dir: string,
    focusedSpace: string | null,
    skipTeardown = false,
    only?: readonly CleanupTarget[],
  ): Promise<TargetOutcome[]> {
    const connected = this.machines
      .machines()
      .filter((m) => m.state === "connected")
      .map((m) => m.name);
    const targets = only ?? [null, ...connected];
    const outcomes = await Promise.all(
      targets.map((target) => this.runOn(target, dir, focusedSpace, skipTeardown)),
    );
    const untrusted = outcomes.filter((o) => "report" in o && o.report.teardownUntrusted);
    const done = outcomes.filter((o) => !("report" in o && o.report.teardownUntrusted));
    if (done.length > 0) {
      for (const o of done) {
        if ("report" in o) this.log.add(cleanupLog(o.report, o.target));
        else
          this.log.add([{ text: `Clean up merged tasks${onText(o.target)} failed: ${o.error}` }]);
      }
      if (done.some((o) => "report" in o && o.report.left.length > 0))
        this.ui.selectPanelTab("log");
      this.spaces.notify(
        runSummary(
          done,
          targets.some((t) => t !== null),
        ),
      );
    }
    if (untrusted.length > 0) {
      const names = untrusted.map((o) => targetName(o.target)).join(", ");
      this.dialog.show({
        title: "Clean up merged tasks",
        summary: `The project file has a teardown command but is not trusted (${names}), so it cannot run. Nothing was removed there. Remove the merged worktrees without running the teardown?`,
        text: "",
        action: {
          label: "Remove without teardown",
          run: () =>
            void this.run(
              dir,
              focusedSpace,
              true,
              untrusted.map((o) => o.target),
            ),
        },
      });
    }
    return outcomes;
  }

  /** One target: its report, after which its spaces are read again; a failure is its reason. */
  private async runOn(
    target: CleanupTarget,
    dir: string,
    focusedSpace: string | null,
    skipTeardown: boolean,
  ): Promise<TargetOutcome> {
    const focused = focusedOn(focusedSpace, target);
    try {
      const report =
        target === null
          ? await this.bridge.invoke<CleanupReport>("task_cleanup", {
              dir,
              focusedSpace: focused,
              skipTeardown,
            })
          : await this.bridge.invoke<CleanupReport>("machine_task_cleanup", {
              name: target,
              dir,
              focusedSpace: focused,
              skipTeardown,
            });
      if (!report.teardownUntrusted) {
        if (target === null) await this.store.refresh();
        else await this.remote.refresh(target);
      }
      return { target, report };
    } catch (err) {
      return { target, error: reasonOf(err) };
    }
  }
}
