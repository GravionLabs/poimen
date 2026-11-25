import { HxCheckbox } from "@gravionlabs/helix-ui";
import { ChangeDetectionStrategy, Component, DestroyRef, computed, inject } from "@angular/core";
import { Schedules } from "../core/schedules";
import { lastResultText, nextRunText } from "../core/schedules-model";

/**
 * Settings page "Schedules" (#1000): the schedules of the user config with their next run and
 * last result, a switch per schedule and the global pause.
 */
@Component({
  selector: "app-schedules-editor",
  imports: [HxCheckbox],
  templateUrl: "./schedules-editor.html",
  styleUrl: "./settings-view.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SchedulesEditor {
  private readonly schedules = inject(Schedules);

  protected readonly paused = computed(() => this.schedules.list().paused);
  protected readonly rows = computed(() => {
    const { paused, schedules } = this.schedules.list();
    return schedules.map((s) => ({
      ...s,
      next: nextRunText(s, paused),
      result: lastResultText(s.last),
    }));
  });
  protected readonly error = this.schedules.error;

  constructor() {
    this.schedules.watch(inject(DestroyRef));
  }

  protected toggle(key: string, event: Event): Promise<void> {
    return this.write(event, (checked) => this.schedules.setEnabled(key, checked));
  }

  protected pause(event: Event): Promise<void> {
    return this.write(event, (checked) => this.schedules.setPaused(checked));
  }

  /** Writes a checkbox; one that was not written goes back to what the config says. */
  private async write(event: Event, set: (checked: boolean) => Promise<boolean>): Promise<void> {
    const box = event.target as HTMLInputElement;
    const checked = box.checked;
    if (!(await set(checked))) box.checked = !checked;
  }
}
