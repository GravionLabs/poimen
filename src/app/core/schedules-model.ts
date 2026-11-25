/** `schedule.list` of poimen-server (#1000, `docs/protocol.md` "Schedules"), as the app shows it. */
export type ScheduleOutcome = "running" | "ok" | "failed" | "skipped";

export interface ScheduleResult {
  outcome: ScheduleOutcome;
  at_ms: number;
  reason?: string;
}

export interface ScheduleRun {
  terminal_id: string;
  due_ms: number;
  started_ms: number;
  ended_ms?: number;
  exit_code?: number;
  outcome: ScheduleOutcome;
}

export interface Schedule {
  key: string;
  cron: string;
  timezone: string;
  dir: string;
  mode: string;
  enabled: boolean;
  next_ms?: number;
  last?: ScheduleResult;
  runs?: ScheduleRun[];
}

export interface ScheduleList {
  paused: boolean;
  schedules: Schedule[];
}

/** A failed run, or the collapsed skips of one reason, of `schedule.alerts` (#1308). */
export interface ScheduleAlert {
  /** Stays the same while skips are added to the alert. */
  id: number;
  /** Grows with every change; the acknowledgement removes up to one. */
  seq: number;
  key: string;
  dir: string;
  mode: string;
  outcome: ScheduleOutcome;
  reason?: string;
  /** The last time. */
  at_ms: number;
  count: number;
}

/** What a notification says about an alert; `count` is how often the skip happened. */
export function alertMessage(alert: ScheduleAlert, count: number): string {
  const what = alert.outcome === "failed" ? "failed" : "skipped";
  const times = count > 1 ? ` ${count} times` : "";
  const reason = alert.reason ? `: ${alert.reason}` : "";
  return `Scheduled run ${alert.mode} in ${alert.dir} ${what}${times}${reason}`;
}

/** A run with the schedule it belongs to. */
export interface ScheduledRun extends ScheduleRun {
  key: string;
  mode: string;
  dir: string;
}

/** The schedules that run next: enabled with a next run, none while all are paused. */
export function activeSchedules(list: ScheduleList): (Schedule & { next_ms: number })[] {
  if (list.paused) return [];
  return list.schedules.filter(
    (s): s is Schedule & { next_ms: number } => s.enabled && s.next_ms !== undefined,
  );
}

/** What the status bar says about the schedules. */
export interface ScheduleHint {
  text: string;
  tooltip: string;
}

/**
 * The status bar hint (#1309): the next run of the active schedules, or that they are paused;
 * `null` without a schedule that would run.
 */
export function scheduleHint(list: ScheduleList, now: number): ScheduleHint | null {
  // While paused the server sends no `next_ms`, so any enabled schedule counts.
  if (list.paused && list.schedules.some((s) => s.enabled)) {
    return { text: "Schedules paused", tooltip: "No schedule runs while they are paused" };
  }
  const [next] = activeSchedules(list).sort((a, b) => a.next_ms - b.next_ms);
  if (!next) return null;
  return {
    text: `Schedules · next ${shortTime(next.next_ms, now)}`,
    tooltip: `Next: ${next.mode} in ${next.dir}`,
  };
}

/** A local time as `09:00`, with the day (`Jan 10 09:00`) when it is not the day of `now`. */
export function shortTime(ms: number, now: number): string {
  const d = new Date(ms);
  const time = scheduleTime(ms).slice(-5);
  if (d.toDateString() === new Date(now).toDateString()) return time;
  return `${d.toLocaleDateString("en-US", { month: "short", day: "numeric" })} ${time}`;
}

/** The runs of all schedules, newest first. */
export function scheduledRuns(list: ScheduleList): ScheduledRun[] {
  return list.schedules
    .flatMap((s) => (s.runs ?? []).map((r) => ({ ...r, key: s.key, mode: s.mode, dir: s.dir })))
    .sort((a, b) => b.started_ms - a.started_ms);
}

/** A time as `2026-01-10 09:00` in the local time zone. */
export function scheduleTime(ms: number): string {
  const d = new Date(ms);
  const two = (n: number) => String(n).padStart(2, "0");
  const day = `${d.getFullYear()}-${two(d.getMonth() + 1)}-${two(d.getDate())}`;
  return `${day} ${two(d.getHours())}:${two(d.getMinutes())}`;
}

export function nextRunText(s: Schedule, paused: boolean): string {
  if (!s.enabled) return "Off";
  if (paused) return "Paused";
  return s.next_ms === undefined ? "Never" : scheduleTime(s.next_ms);
}

export function lastResultText(last: ScheduleResult | undefined): string {
  if (!last) return "No run yet";
  const reason = last.reason ? `: ${last.reason}` : "";
  return `${last.outcome}${reason} (${scheduleTime(last.at_ms)})`;
}

export function outcomeSeverity(outcome: ScheduleOutcome): "info" | "success" | "danger" | "warn" {
  if (outcome === "running") return "info";
  if (outcome === "ok") return "success";
  return outcome === "failed" ? "danger" : "warn";
}
