import { Injectable, inject } from "@angular/core";
import type { StatusChange } from "./attention";
import { TauriBridge } from "./tauri";

/** How long after a notification a window focus counts as a click on it. */
export const CLICK_WINDOW_MS = 30_000;

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
 * System notifications for agents that turn blocked or done. The notification plugin reports no
 * click on desktop, so a click is recognized as the window gaining focus shortly after a
 * notification was shown while it was in the background: that opens the notified pane's tab.
 */
@Injectable({ providedIn: "root" })
export class Notifier {
  private readonly bridge = inject(TauriBridge);
  private pending: { paneId: string; at: number } | null = null;

  /** Shows a notification per change that settings allow and the user cannot already see. */
  notify(changes: readonly StatusChange[], ctx: NotifyContext): void {
    for (const { paneId, status } of changes) {
      if (!(status === "blocked" ? ctx.onBlocked : ctx.onDone)) continue;
      if (ctx.focused && ctx.viewed.has(paneId)) continue;
      const label = ctx.label(paneId);
      const body = status === "blocked" ? `${label} is waiting for you` : `${label} is done`;
      if (!ctx.focused) this.pending = { paneId, at: ctx.now };
      void this.bridge.notify("poimen", body).catch(() => undefined);
    }
  }

  /** The window gained focus: the pane to open when a recent notification was clicked, once. */
  takeClicked(now: number): string | null {
    const pending = this.pending;
    this.pending = null;
    return pending && now - pending.at <= CLICK_WINDOW_MS ? pending.paneId : null;
  }
}
