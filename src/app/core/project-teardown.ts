import { Injectable, inject } from "@angular/core";
import { ResultDialog } from "./result-dialog";
import { SpaceActions } from "./space-actions";
import { TauriBridge } from "./tauri";

/** Wire shape of `RunResult` (`project.rs`). */
export interface RunResult {
  exitCode: number | null;
  timedOut: boolean;
  output: string;
  truncated: boolean;
}

/** Wire shape of `TeardownReport` (`project.rs`). */
export interface TeardownReport {
  plan: "none" | "untrusted" | "ran";
  result: RunResult | null;
}

/** The dialog text for a report. */
export function teardownSummary(report: TeardownReport): string {
  if (report.plan === "none") return "This project has no teardown command.";
  if (report.plan === "untrusted") {
    return "The project file is not trusted, so the teardown did not run. Create a space for the project to review and trust its commands.";
  }
  const r = report.result;
  if (!r) return "The teardown did not run.";
  if (r.timedOut) return "The teardown timed out after 5 minutes and was stopped.";
  if (r.exitCode === 0) return "The teardown finished successfully.";
  return r.exitCode === null
    ? "The teardown was killed."
    : `The teardown failed (exit ${r.exitCode}).`;
}

/** "Project: Run teardown": runs the `teardown` of the project file in a worktree (`project_teardown`). */
@Injectable({ providedIn: "root" })
export class ProjectTeardown {
  private readonly bridge = inject(TauriBridge);
  private readonly dialog = inject(ResultDialog);
  private readonly spaces = inject(SpaceActions);

  /** Runs it in `dir` (the worktree of the focused space) and shows the output. */
  async run(dir: string): Promise<TeardownReport | null> {
    let report: TeardownReport;
    try {
      report = await this.bridge.invoke<TeardownReport>("project_teardown", { dir });
    } catch (err) {
      const reason = err && typeof err === "object" && "message" in err ? err.message : err;
      this.spaces.notify(`Project: Run teardown failed: ${String(reason)}`);
      return null;
    }
    const output = report.result?.output ?? "";
    this.dialog.show({
      title: "Project: Run teardown",
      summary: teardownSummary(report),
      text: report.result?.truncated ? `${output}\n[output truncated at 1 MB]` : output,
    });
    return report;
  }
}
