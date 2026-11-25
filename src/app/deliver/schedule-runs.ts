import { ChangeDetectionStrategy, Component, computed, inject, output } from "@angular/core";
import { HxButton, HxTag } from "@gravionlabs/helix-ui";
import { Schedules } from "../core/schedules";
import { outcomeSeverity, scheduleTime, type ScheduledRun } from "../core/schedules-model";
import { SpacesStore } from "../core/spaces-store";

/** The runs shown; the server keeps more per schedule. */
const SHOWN = 20;

/**
 * "Scheduled runs" of the Runs view (#1000): the runs `poimen-server` started for the user's
 * schedules, marked with the trigger "schedule". The view renders it only with a run and keeps
 * `Schedules` read. Its styles are in `src/styles.css`, like those of the lead runs.
 */
@Component({
  selector: "app-schedule-runs",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [HxButton, HxTag],
  templateUrl: "./schedule-runs.html",
})
export class ScheduleRuns {
  private readonly schedules = inject(Schedules);
  private readonly spaces = inject(SpacesStore);
  /** The terminal of a run was asked for. */
  readonly paneChosen = output<string>();

  protected readonly rows = computed(() => {
    const open = this.spaces.panes();
    return this.schedules
      .runs()
      .slice(0, SHOWN)
      .map((r: ScheduledRun) => ({
        ...r,
        severity: outcomeSeverity(r.outcome),
        time: scheduleTime(r.started_ms),
        open: open.has(r.terminal_id),
      }));
  });
}
