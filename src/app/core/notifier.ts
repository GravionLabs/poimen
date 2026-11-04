import { Injectable, inject } from "@angular/core";
import type { StatusChange } from "./attention";
import type { AgentStatus } from "./herdr";
import { notificationMessage } from "./notification-center";
import { TauriBridge, type Unlisten } from "./tauri";

/**
 * How long a pane must stay done before its notification goes out. herdr's screen heuristics
 * report a Claude pane as done for a moment between tool calls (done, then working again within
 * about two seconds); without the delay every such flicker became a notification.
 */
export const DONE_DELAY_MS = 5000;

export interface NotifyContext {
  /** Whether the user sees the pane right now: its tab is on screen and the window has focus. */
  seen: (paneId: string) => boolean;
  /** The pane's current status, read again when a delayed notification is due. */
  status: (paneId: string) => AgentStatus | undefined;
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
 *
 * A done notification waits `DONE_DELAY_MS` and goes out only if the pane is still done and not
 * seen; after one, the pane stays quiet about done until the user has seen it.
 */
@Injectable({ providedIn: "root" })
export class Notifier {
  private readonly bridge = inject(TauriBridge);
  /** Done notifications waiting for their delay, by pane. */
  private readonly pending = new Map<string, ReturnType<typeof setTimeout>>();
  /** Panes that got a done notification the user has not seen since. */
  private readonly doneSent = new Set<string>();

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
    for (const id of this.doneSent) if (ctx.seen(id)) this.doneSent.delete(id);
    for (const { paneId, status } of changes) {
      if (!(status === "blocked" ? ctx.onBlocked : ctx.onDone)) continue;
      if (ctx.seen(paneId)) continue;
      if (status === "blocked") this.send(paneId, status, ctx);
      else this.scheduleDone(paneId, ctx);
    }
  }

  private scheduleDone(paneId: string, ctx: NotifyContext): void {
    if (this.doneSent.has(paneId) || this.pending.has(paneId)) return;
    const timer = setTimeout(() => {
      this.pending.delete(paneId);
      if (ctx.status(paneId) !== "done" || ctx.seen(paneId)) return;
      this.doneSent.add(paneId);
      this.send(paneId, "done", ctx);
    }, DONE_DELAY_MS);
    this.pending.set(paneId, timer);
  }

  private send(paneId: string, status: StatusChange["status"], ctx: NotifyContext): void {
    const body = notificationMessage(ctx.label(paneId), status);
    void this.bridge
      .notify("poimen", body, { paneId })
      .catch((error: unknown) => console.warn("poimen: notification failed", error));
  }
}
