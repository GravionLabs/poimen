import { Injectable, computed, signal } from "@angular/core";
import type { StatusChange } from "./attention";

/** How many entries the center keeps; older ones fall off. */
export const MAX_NOTIFICATIONS = 100;

/** One agent status change: a pane turned blocked or done. */
export interface NotificationEntry {
  id: number;
  paneId: string;
  /** Tab title of the pane when the change happened. */
  label: string;
  status: StatusChange["status"];
  message: string;
  /** Epoch milliseconds. */
  time: number;
  read: boolean;
}

/** The message of a status change, the same wording as the system notification. */
export function notificationMessage(label: string, status: StatusChange["status"]): string {
  return status === "blocked" ? `${label} is waiting for you` : `${label} is done`;
}

/** Puts `added` (oldest first) in front of `list`, newest first, and keeps at most `max`. */
export function prependCapped(
  list: readonly NotificationEntry[],
  added: readonly NotificationEntry[],
  max = MAX_NOTIFICATIONS,
): NotificationEntry[] {
  return [...added].reverse().concat(list).slice(0, max);
}

export function markAllRead(list: readonly NotificationEntry[]): NotificationEntry[] {
  return list.map((e) => (e.read ? e : { ...e, read: true }));
}

export function unreadCount(list: readonly NotificationEntry[]): number {
  return list.filter((e) => !e.read).length;
}

/**
 * Recent agent status changes (blocked, done) of all spaces, kept in memory for the session only.
 * herdr sends no separate notification message, so entries derive from the status changes
 * `Attention.observe` reports.
 */
@Injectable({ providedIn: "root" })
export class NotificationCenter {
  private nextId = 1;
  /** Newest first. */
  readonly entries = signal<readonly NotificationEntry[]>([]);
  readonly unread = computed(() => unreadCount(this.entries()));

  /** Records changes; `label` names a pane, `now` is the time of all of them. */
  record(changes: readonly StatusChange[], label: (paneId: string) => string, now: number): void {
    if (changes.length === 0) return;
    const added = changes.map(({ paneId, status }): NotificationEntry => {
      const name = label(paneId);
      return {
        id: this.nextId++,
        paneId,
        label: name,
        status,
        message: notificationMessage(name, status),
        time: now,
        read: false,
      };
    });
    this.entries.update((list) => prependCapped(list, added));
  }

  markRead(id: number): void {
    this.entries.update((list) => list.map((e) => (e.id === id ? { ...e, read: true } : e)));
  }

  markAllRead(): void {
    if (this.unread() > 0) this.entries.update(markAllRead);
  }
}
