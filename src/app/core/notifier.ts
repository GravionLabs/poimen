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
  /** Notifications sent per pane that may still be in the tray, with the status they announced. */
  private readonly shown = new Map<string, { id: number; status: StatusChange["status"] }[]>();
  private nextId = 1;

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
    this.withdrawSeen(ctx);
    for (const id of this.doneSent) if (ctx.seen(id)) this.doneSent.delete(id);
    for (const { paneId, status } of changes) {
      if (!(status === "blocked" ? ctx.onBlocked : ctx.onDone)) continue;
      if (ctx.seen(paneId)) continue;
      if (status === "blocked") this.send(paneId, status, ctx);
      else this.scheduleDone(paneId, ctx);
    }
  }

  /**
   * Removes the notifications that no longer match the in-app marks, so the tray (and the count
   * GNOME, KDE and Ubuntu Dock draw on the icon from it) follows them: those of a pane the user
   * sees, of a pane whose status left the one announced and of a pane that is gone. Only Linux
   * and the BSDs can close a shown notification; elsewhere the call does nothing.
   */
  private withdrawSeen(ctx: NotifyContext): void {
    const ids: number[] = [];
    for (const [paneId, list] of this.shown) {
      const seen = ctx.seen(paneId);
      const current = ctx.status(paneId);
      const keep = seen ? [] : list.filter((n) => n.status === current);
      ids.push(...list.filter((n) => !keep.includes(n)).map((n) => n.id));
      if (keep.length) this.shown.set(paneId, keep);
      else this.shown.delete(paneId);
    }
    for (const id of this.doneSent) if (ctx.status(id) === undefined) this.doneSent.delete(id);
    if (!ids.length) return;
    void this.bridge
      .removeNotifications(ids)
      .catch((error: unknown) => console.warn("poimen: could not withdraw notifications", error));
  }

  /**
   * Rechecks (and withdraws) whenever `target` fires `focus`: coming back to the window makes
   * the active tabs seen again. Returns the function that stops listening.
   */
  watchFocus(target: EventTarget, ctx: () => NotifyContext): () => void {
    const onFocus = () => this.notify([], ctx());
    target.addEventListener("focus", onFocus);
    return () => target.removeEventListener("focus", onFocus);
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
    const id = this.nextId++;
    this.shown.set(paneId, [...(this.shown.get(paneId) ?? []), { id, status }]);
    void this.bridge
      .notify("poimen", body, { paneId }, id)
      .catch((error: unknown) => console.warn("poimen: notification failed", error));
  }
}
