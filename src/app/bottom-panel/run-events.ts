import {
  cardKey,
  ageText,
  runTitle,
  stepText,
  type DeliverCard,
  type DeliverStatus,
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

/**
 * The runs (with a readable state) of the repository that contains `dir`, oldest first. `machine`
 * is the machine of the space (`dir` is a path there); only its runs count, so a local and a
 * remote repository at the same path stay apart.
 */
export function runsOfSpace(
  cards: readonly DeliverCard[],
  dir: string | undefined,
  machine?: string,
): RunChoice[] {
  if (!dir) return [];
  return cards.flatMap((card) =>
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
  );
}

/**
 * The run to show: the one whose `host["poimen.terminal"]` is the active terminal, else the one
 * the user picked, else the latest run (by `updated`) of the space's repository.
 */
export function selectRun(
  runs: readonly RunChoice[],
  terminalId: string | null | undefined,
  picked: string | null,
): RunChoice | undefined {
  const byTerminal = terminalId
    ? runs.find((r) => r.status.host["poimen.terminal"] === terminalId)
    : undefined;
  if (picked) {
    const chosen = runs.find((r) => r.key === picked);
    if (chosen) return chosen;
  }
  if (byTerminal) return byTerminal;
  return [...runs].sort((a, b) => Date.parse(b.status.updated) - Date.parse(a.status.updated))[0];
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
