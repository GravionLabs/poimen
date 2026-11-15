import {
  cardKey,
  ageText,
  runTitle,
  stepText,
  type DeliverCard,
  type DeliverStatus,
  type RunEventRow,
  runState,
  startedAt,
  type Tone,
} from "../deliver/deliver-status";

/** A run offered to the Events tab. */
export interface RunChoice {
  key: string;
  root: string;
  status: DeliverStatus;
  /** The machine of a remote run (#692); absent on this Mac. */
  machine?: string;
  /** The machine is not connected: the last known events. */
  offline?: boolean;
}

const inside = (dir: string, root: string): boolean => {
  const base = root.replace(/\/$/, "");
  return dir === root || dir.startsWith(base + "/");
};

/** When a run started (first event), else its last update; 0 when neither is a date. */
const startMs = (r: RunChoice): number => Date.parse(startedAt(r.status) ?? r.status.updated) || 0;
const updatedMs = (r: RunChoice): number => Date.parse(r.status.updated) || 0;

/** Newest first: start of the run, then `updated`, then the run id (a total order). */
const newest = (a: RunChoice, b: RunChoice): number =>
  startMs(b) - startMs(a) ||
  updatedMs(b) - updatedMs(a) ||
  (a.status.run < b.status.run ? -1 : a.status.run > b.status.run ? 1 : 0);

/**
 * The runs (with a readable state) of the repository that contains `dir`, newest first (start of
 * the run, `updated` as the tie-break; the cards stay in arrival order for the Deliver view).
 * `machine` is the machine of the space (`dir` is a path there); only its runs count, so a local
 * and a remote repository at the same path stay apart.
 */
export function runsOfSpace(
  cards: readonly DeliverCard[],
  dir: string | undefined,
  machine?: string,
): RunChoice[] {
  if (!dir) return [];
  return cards
    .flatMap((card) =>
      card.state.kind === "ok" && card.machine === machine && inside(dir, card.root)
        ? [
            {
              key: cardKey(card),
              root: card.root,
              status: card.state.status,
              ...(card.machine === undefined ? {} : { machine: card.machine }),
              ...(card.offline ? { offline: true } : {}),
            },
          ]
        : [],
    )
    .sort(newest);
}

/** A run the user picked and the newest start among the runs when they picked it. */
export interface Pick {
  key: string;
  seen: number;
}

/** The pick of `key` among `runs` (newest first). */
export const pickRunOf = (runs: readonly RunChoice[], key: string): Pick => ({
  key,
  seen: runs.reduce((m, r) => Math.max(m, startMs(r)), 0),
});

/**
 * Whether a pick still holds: its run is listed and no run of the repository started after the
 * pick was made. Otherwise the Events tab follows the active run again.
 */
export const pickHolds = (runs: readonly RunChoice[], picked: Pick | null): boolean =>
  !!picked && runs.some((r) => r.key === picked.key) && !runs.some((r) => startMs(r) > picked.seen);

/**
 * The run to show: the pick that still holds, else the running (not stale) run of the active
 * terminal with the latest `updated`, else the latest run of that terminal, else the latest run of
 * the repository. A terminal is the `host["poimen.terminal"]` of the run.
 */
export function selectRun(
  runs: readonly RunChoice[],
  terminalId: string | null | undefined,
  picked: Pick | null,
  now: number,
): RunChoice | undefined {
  if (picked && pickHolds(runs, picked)) return runs.find((r) => r.key === picked.key);
  const sorted = [...runs].sort(newest);
  const mine = terminalId
    ? sorted.filter((r) => r.status.host["poimen.terminal"] === terminalId)
    : [];
  const running = mine
    .filter((r) => runState(r.status, now) === "running")
    .sort((a, b) => updatedMs(b) - updatedMs(a) || newest(a, b));
  return running[0] ?? mine[0] ?? sorted[0];
}

/** `text` cut to `max` characters with an ellipsis. */
export const shorten = (text: string, max: number): string =>
  text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text;

/**
 * The first open task of a run: `#177 review 4/7` (subject title, shortened, and its step with
 * `n/total` when both are known); empty without an open task.
 */
export function currentTaskText(status: DeliverStatus): string {
  const task = status.tasks[0];
  if (!task) return "";
  const step = task.step ? stepText(task.step) : "";
  return [shorten(task.subject.title, 40), step].filter((p) => p !== "").join(" ");
}

/**
 * The line of a run: `M014 3/18 · #177 review 4/7 (running, 2 min ago)`: title, run progress,
 * the current task and the state with its age. Parts without data are left out. A remote run has
 * its machine in front (`box: ...`) and `offline` in the state while the machine is not connected.
 */
export function runLabel(run: RunChoice, now: number): string {
  const s = run.status;
  const head = [runTitle(s, run.root), s.progress ? `${s.progress.done}/${s.progress.total}` : ""]
    .filter((p) => p !== "")
    .join(" ");
  const task = currentTaskText(s);
  const age = ageText(s.updated, now);
  const state = [s.status, age, run.offline ? "offline" : ""].filter((p) => p !== "").join(", ");
  const machine = run.machine ? `${run.machine}: ` : "";
  return `${machine}${[head, task].filter((p) => p !== "").join(" · ")} (${state})`;
}

/** Style of a row, from the tone alone: a CSS class and an icon. */
export interface ToneStyle {
  class: string;
  icon: string;
  attention: boolean;
}

const STYLES: Record<Tone, ToneStyle> = {
  neutral: { class: "tone-neutral", icon: "·", attention: false },
  info: { class: "tone-info", icon: "i", attention: false },
  success: { class: "tone-success", icon: "✓", attention: false },
  warning: { class: "tone-warning", icon: "!", attention: true },
  danger: { class: "tone-danger", icon: "×", attention: true },
};

export const toneStyle = (tone: Tone): ToneStyle => STYLES[tone];

/** `HH:MM:SS` of an ISO timestamp in local time; empty when it is missing or not a date. */
export function eventTime(ts: string | null): string {
  const date = ts ? new Date(ts) : null;
  if (!date || Number.isNaN(date.getTime())) return "";
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(date.getHours())}:${two(date.getMinutes())}:${two(date.getSeconds())}`;
}

/** One skipped task of a fold: `task.started` followed at once by its `task.finished` skipped. */
export interface SkippedTask {
  task: string;
  subject: string;
  text: string;
  reason: string;
}

/** What the Events tab lists: a plain row or a fold of consecutive skipped tasks (#781). */
export type EventItem =
  | { kind: "row"; row: RunEventRow }
  | {
      kind: "skipped";
      /** Stable while the fold grows at its end: the first task and its start. */
      key: string;
      ts: string | null;
      tasks: SkippedTask[];
      /** `21 skipped (blocked)`. */
      label: string;
    };

const quiet = (r: RunEventRow): boolean => r.tone !== "warning" && r.tone !== "danger";

/** The skipped task of `started` + `finished` when they form such a pair (core fields only). */
function skippedPair(started: RunEventRow | undefined, finished: RunEventRow | undefined) {
  if (
    !started ||
    !finished ||
    started.type !== "task.started" ||
    finished.type !== "task.finished" ||
    !started.task ||
    started.task !== finished.task ||
    finished.outcome !== "skipped" ||
    !quiet(started) ||
    !quiet(finished)
  )
    return null;
  return {
    task: started.task,
    subject: started.subject || finished.subject || "",
    text: finished.text || started.text || "",
    reason: finished.reason || "",
  } satisfies SkippedTask;
}

/**
 * Folds consecutive skipped tasks into one item. A pair is a `task.started` directly followed by
 * the `task.finished` of the same task with outcome `skipped`; rows with tone `warning` or
 * `danger` are never folded. A single skipped task stays a fold of one.
 */
export function foldSkipped(rows: readonly RunEventRow[]): EventItem[] {
  const items: EventItem[] = [];
  let i = 0;
  while (i < rows.length) {
    const pair = skippedPair(rows[i], rows[i + 1]);
    if (!pair) {
      items.push({ kind: "row", row: rows[i]! });
      i += 1;
      continue;
    }
    const tasks = [pair];
    const first = rows[i]!;
    i += 2;
    for (
      let next = skippedPair(rows[i], rows[i + 1]);
      next;
      next = skippedPair(rows[i], rows[i + 1])
    ) {
      tasks.push(next);
      i += 2;
    }
    const reasons = [...new Set(tasks.map((t) => t.reason).filter((r) => r !== ""))];
    items.push({
      kind: "skipped",
      key: `${first.seq ?? ""}:${pair.task}`,
      ts: first.ts,
      tasks,
      label: `${tasks.length} skipped${reasons.length ? ` (${reasons.join(", ")})` : ""}`,
    });
  }
  return items;
}
