import { ChangeDetectionStrategy, Component, Injector, inject, output } from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { NotificationCenter, type NotificationEntry } from "../core/notification-center";
import { PrWatch } from "../core/pr-watch";

/** Local time of day of a notification. */
export function notificationTime(time: number): string {
  return new Date(time).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** Sidebar view: recent agent status changes of all spaces, newest first. */
@Component({
  imports: [HxButton],
  selector: "app-notifications",
  templateUrl: "./notifications.html",
  styleUrl: "./notifications.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Notifications {
  protected readonly center = inject(NotificationCenter);
  private readonly injector = inject(Injector);
  /** An entry was clicked: open the tab of its pane. */
  readonly paneChosen = output<string>();
  /** An entry of a run was clicked: reveal its repository in the Runs view. */
  readonly runChosen = output<{ root: string; machine?: string }>();
  protected readonly timeOf = notificationTime;

  /** The "Fix checks" button of a failed result: starts the action, opens nothing else. */
  protected fixChecks(entry: NotificationEntry): void {
    this.center.markRead(entry.id);
    if (entry.checks) void this.injector.get(PrWatch).fixChecks(entry.checks);
  }

  protected choose(entry: NotificationEntry): void {
    this.center.markRead(entry.id);
    if (entry.deliver) this.runChosen.emit(entry.deliver);
    else if (entry.paneId) this.paneChosen.emit(entry.paneId);
  }
}
