import { Injectable, computed, signal } from "@angular/core";
import type { StatusChange } from "./attention";
import { alertMessage, type ScheduleAlert } from "./schedules-model";

/** How many entries the center keeps; older ones fall off. */
export const MAX_NOTIFICATIONS = 100;

/** One agent status change: a pane turned blocked, failed or done. */
export interface NotificationEntry {
  id: number;
  paneId: string;
  /** Tab title of the pane when the change happened. */
  label: string;
  status: StatusChange["status"] | "silent";
  message: string;
  /** Epoch milliseconds. */
  time: number;
  read: boolean;
  /** Set for an attention of a run (#806): the repository (and machine) the Runs view reveals. */
  deliver?: { root: string; machine?: string };
  /** Set for a write of an agent through `poimen mcp` (#615): what it typed. */
  detail?: string;
  /** Set for collapsed skips of a schedule (#1308): what a later alert is added to. */
  skips?: { key: string; reason?: string; alertId: number; count: number; earlier: number };
  /** Set for the result of the checks of a pull request (#995); a failed one offers `fix-checks`. */
  checks?: CheckResult;
}

/** The checks of the pull request of a space finished (#995). */
export interface CheckResult {
  passed: boolean;
  number: number;
  title: string;
  url: string;
  /** The head commit the result belongs to. */
  sha: string;
  /** The space label. */
  label: string;
  /** A pane of the space, so a click opens it. */
  paneId: string;
  /** The directory of the space (the root of the `fix-checks` action). */
  dir: string;
  branch: string;
  base: string;
  crossRepo: boolean;
}

/** An attention of a run that asks for the user (needs you, warning or danger). */
export interface RunAlert {
  root: string;
  machine?: string;
  /** Names the run: its subject, else the repository directory. */
  label: string;
  message: string;
}

/** The message of a status change, the same wording as the system notification. */
export function notificationMessage(label: string, status: StatusChange["status"]): string {
  if (status === "error") return `${label} failed`;
  return status === "blocked" ? `${label} is waiting for you` : `${label} is done`;
}

/** The panes that turned silent (#899) between two snapshots: they had no mark and have one now. */
export function newlySilent(
  previous: ReadonlyMap<string, number | undefined>,
  current: ReadonlyMap<string, number | undefined>,
): string[] {
  return [...current]
    .filter(
      ([id, since]) => since !== undefined && previous.has(id) && previous.get(id) === undefined,
    )
    .map(([id]) => id);
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

  /** Records the attentions of runs (#806); each entry opens the Runs view at its repository. */
  recordRuns(alerts: readonly RunAlert[], now: number): void {
    if (alerts.length === 0) return;
    const added = alerts.map((a): NotificationEntry => ({
      id: this.nextId++,
      paneId: "",
      label: a.label,
      status: "blocked",
      message: a.message,
      time: now,
      read: false,
      deliver: { root: a.root, ...(a.machine === undefined ? {} : { machine: a.machine }) },
    }));
    this.entries.update((list) => prependCapped(list, added));
  }

  /** Records the results of checks (#995): the pull request and the space, one entry each. */
  recordChecks(results: readonly CheckResult[], now: number): void {
    if (results.length === 0) return;
    const added = results.map((r): NotificationEntry => ({
      id: this.nextId++,
      paneId: r.paneId,
      label: r.label,
      status: r.passed ? "done" : "error",
      message: `Checks of #${r.number} ${r.passed ? "passed" : "failed"}: ${r.title}`,
      time: now,
      read: false,
      checks: r,
    }));
    this.entries.update((list) => prependCapped(list, added));
  }

  /** The `seq` and time of the alerts added, so a failed acknowledgement adds none twice. */
  private readonly addedAlerts = new Set<string>();

  /**
   * Records the alerts of scheduled runs (#1308): one entry per failed run, and one per schedule
   * and reason for skips, which counts up and moves to the front with every further alert.
   */
  recordSchedules(alerts: readonly ScheduleAlert[]): void {
    for (const alert of [...alerts].sort((a, b) => a.seq - b.seq)) {
      const mark = `${alert.seq}:${alert.at_ms}`;
      if (this.addedAlerts.has(mark)) continue;
      this.addedAlerts.add(mark);
      this.entries.update((list) => this.withAlert(list, alert));
    }
  }

  private withAlert(list: readonly NotificationEntry[], alert: ScheduleAlert): NotificationEntry[] {
    const failed = alert.outcome === "failed";
    const old = failed
      ? undefined
      : list.find((e) => e.skips?.key === alert.key && e.skips.reason === alert.reason);
    // The same alert again already holds its earlier count; a new one adds to the entry's.
    let earlier = 0;
    if (old?.skips) {
      earlier = old.skips.alertId === alert.id ? old.skips.earlier : old.skips.count;
    }
    const count = earlier + alert.count;
    const entry: NotificationEntry = {
      id: this.nextId++,
      paneId: "",
      label: `${alert.mode} · ${alert.dir}`,
      status: failed ? "error" : "silent",
      message: alertMessage(alert, count),
      time: alert.at_ms,
      read: false,
      ...(failed
        ? {}
        : {
            skips: { key: alert.key, reason: alert.reason, alertId: alert.id, count, earlier },
          }),
    };
    return prependCapped(
      list.filter((e) => e !== old),
      [entry],
    );
  }

  /** Records a write of an agent through `poimen mcp` (#615); `label` is the calling terminal. */
  recordAgentWrite(label: string, message: string, detail: string, now: number): void {
    const entry: NotificationEntry = {
      id: this.nextId++,
      paneId: "",
      label,
      status: "done",
      message,
      time: now,
      read: false,
      detail,
    };
    this.entries.update((list) => prependCapped(list, [entry]));
  }

  /** Records a failure the server reports, such as a resume of a hibernated agent (#904). */
  recordFailure(label: string, message: string, now: number): void {
    const entry: NotificationEntry = {
      id: this.nextId++,
      paneId: "",
      label,
      status: "error",
      message,
      time: now,
      read: false,
    };
    this.entries.update((list) => prependCapped(list, [entry]));
  }

  private silentPrevious: ReadonlyMap<string, number | undefined> = new Map();

  /**
   * Records one entry per pane that turned silent (#899); no ring, sound or badge. `silentSince` is
   * `agent_silent_since` of every pane; clearing it and turning silent again is a new transition.
   */
  observeSilent(
    silentSince: ReadonlyMap<string, number | undefined>,
    label: (paneId: string) => string,
    now: number,
  ): void {
    const ids = newlySilent(this.silentPrevious, silentSince);
    this.silentPrevious = new Map(silentSince);
    if (ids.length === 0) return;
    const added = ids.map((paneId): NotificationEntry => {
      const name = label(paneId);
      return {
        id: this.nextId++,
        paneId,
        label: name,
        status: "silent",
        message: `${name} has been silent for a while`,
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
