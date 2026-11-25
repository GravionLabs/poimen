import { DestroyRef, Injectable, computed, inject, signal } from "@angular/core";
import { scheduledRuns, type ScheduleList } from "./schedules-model";
import { TauriBridge } from "./tauri";

/** How often the list is read again while a view shows it. */
export const SCHEDULES_POLL_MS = 5000;

/**
 * The schedules of the local poimen-server (#1000) for the settings page and the Runs view. The
 * server and the user config own them: this reads `schedules_list` while a view watches, and the
 * switches write the config.
 */
@Injectable({ providedIn: "root" })
export class Schedules {
  private readonly bridge = inject(TauriBridge);
  private readonly _list = signal<ScheduleList>({ paused: false, schedules: [] });
  private readonly _error = signal<string | null>(null);
  private watchers = 0;
  private timer: ReturnType<typeof setInterval> | undefined;

  readonly list = this._list.asReadonly();
  /** Why the list could not be read or a switch not written. */
  readonly error = this._error.asReadonly();
  readonly runs = computed(() => scheduledRuns(this._list()));

  /** Reads the list now and every few seconds until the caller is destroyed. */
  watch(owner: DestroyRef): void {
    if (this.watchers++ === 0) {
      this.timer = setInterval(() => void this.refresh(), SCHEDULES_POLL_MS);
    }
    owner.onDestroy(() => {
      if (--this.watchers === 0) clearInterval(this.timer);
    });
    void this.refresh();
  }

  async refresh(): Promise<void> {
    try {
      const list = await this.bridge.invoke<ScheduleList | undefined>("schedules_list");
      if (!Array.isArray(list?.schedules)) return;
      this._list.set(list);
      this._error.set(null);
    } catch (e) {
      this._error.set(`Schedules could not be read: ${String(e)}`);
    }
  }

  setEnabled(key: string, enabled: boolean): Promise<void> {
    return this.write("set_schedule_enabled", { key, enabled });
  }

  setPaused(paused: boolean): Promise<void> {
    return this.write("set_schedules_paused", { paused });
  }

  private async write(command: string, args: Record<string, unknown>): Promise<void> {
    try {
      await this.bridge.invoke(command, args);
    } catch (e) {
      await this.refresh();
      this._error.set(`The schedule could not be changed: ${String(e)}`);
      return;
    }
    await this.refresh();
  }
}
