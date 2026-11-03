import { ChangeDetectionStrategy, Component, inject, output } from "@angular/core";
import { NotificationCenter } from "../core/notification-center";

/** Local time of day of a notification. */
export function notificationTime(time: number): string {
  return new Date(time).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

/** Sidebar view: recent agent status changes of all spaces, newest first. */
@Component({
  selector: "app-notifications",
  templateUrl: "./notifications.html",
  styleUrl: "./notifications.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Notifications {
  protected readonly center = inject(NotificationCenter);
  /** An entry was clicked: open the tab of its pane. */
  readonly paneChosen = output<string>();
  protected readonly timeOf = notificationTime;

  protected choose(id: number, paneId: string): void {
    this.center.markRead(id);
    this.paneChosen.emit(paneId);
  }
}
