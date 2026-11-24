import { HxButton } from "@gravionlabs/helix-ui";
import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import type { NotifyHook } from "../core/notify-settings";
import { SettingsStore } from "../core/settings";
import { TauriBridge } from "../core/tauri";

type Status = "blocked" | "done";

/**
 * Settings page "Notifications": the `notify` hooks of config.json (#1017) and a Test button per
 * status. The server runs the hooks, also with the app closed; Test runs the same code here with a
 * sample message.
 */
@Component({
  selector: "app-notifications-page",
  imports: [HxButton],
  templateUrl: "./notifications-page.html",
  styleUrls: ["./settings-view.css"],
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class NotificationsPage {
  private readonly store = inject(SettingsStore);
  private readonly bridge = inject(TauriBridge);

  protected readonly statuses: readonly Status[] = ["blocked", "done"];
  protected readonly notify = computed(() => this.store.settings().notify);
  protected readonly warnings = computed(() =>
    this.store.settings().warnings.filter((w) => w.startsWith("notify")),
  );
  protected readonly running = signal<Status | null>(null);
  protected readonly result = signal<{
    status: Status;
    error: string | null;
  } | null>(null);

  protected hook(status: Status): NotifyHook {
    return this.notify()[status];
  }

  protected configured(status: Status): boolean {
    const h = this.hook(status);
    return h.command !== null || h.url !== null;
  }

  protected async test(status: Status): Promise<void> {
    this.running.set(status);
    this.result.set(null);
    try {
      await this.bridge.invoke("notify_test", { status });
      this.result.set({ status, error: null });
    } catch (e) {
      this.result.set({
        status,
        error: e instanceof Error ? e.message : String(e),
      });
    } finally {
      this.running.set(null);
    }
  }
}
