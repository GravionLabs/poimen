import { Injectable, inject } from "@angular/core";
import type { StatusChange } from "./attention";
import { TauriBridge, type Unlisten } from "./tauri";

export interface NotifyContext {
  /** Panes whose tab is on screen. */
  viewed: ReadonlySet<string>;
  /** Whether the poimen window has focus. */
  focused: boolean;
  onBlocked: boolean;
  onDone: boolean;
  /** Tab title of a pane. */
  label: (paneId: string) => string;
  now: number;
}

/**
 * System notifications for agents that turn blocked or done. Each carries its pane id as `extra`;
 * the plugin reports a click on it (`onAction`, `tap`) with that extra, so the click opens the
 * pane's tab, whichever of several notifications was clicked.
 */
@Injectable({ providedIn: "root" })
export class Notifier {
  private readonly bridge = inject(TauriBridge);

  /** Calls `open` with the pane of every clicked notification; call before notifying. */
  listen(open: (paneId: string) => void): Promise<Unlisten> {
    return this.bridge.onNotificationClick((extra) => {
      if (typeof extra["paneId"] !== "string") return;
      open(extra["paneId"]);
      void this.bridge
        .focusWindow()
        .catch((error: unknown) => console.warn("poimen: could not focus the window", error));
    });
  }

  /** Shows a notification per change that settings allow and the user cannot already see. */
  notify(changes: readonly StatusChange[], ctx: NotifyContext): void {
    for (const { paneId, status } of changes) {
      if (!(status === "blocked" ? ctx.onBlocked : ctx.onDone)) continue;
      if (ctx.focused && ctx.viewed.has(paneId)) continue;
      const label = ctx.label(paneId);
      const body = status === "blocked" ? `${label} is waiting for you` : `${label} is done`;
      void this.bridge
        .notify("poimen", body, { paneId })
        .catch((error: unknown) => console.warn("poimen: notification failed", error));
    }
  }
}
