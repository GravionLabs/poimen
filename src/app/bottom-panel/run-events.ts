import {
  cardKey,
  ageText,
  runTitle,
  type DeliverCard,
  type DeliverStatus,
  type Tone,
} from "../deliver/deliver-status";

/** A run offered to the Events tab. */
export interface RunChoice {
  key: string;
  root: string;
  status: DeliverStatus;
}

const inside = (dir: string, root: string): boolean => {
  const base = root.replace(/\/$/, "");
  return dir === root || dir.startsWith(base + "/");
};

/** The runs (with a readable state) of the repository that contains `dir`, oldest first. */
export function runsOfSpace(cards: readonly DeliverCard[], dir: string | undefined): RunChoice[] {
  if (!dir) return [];
  return cards.flatMap((card) =>
    card.state.kind === "ok" && inside(dir, card.root)
      ? [{ key: cardKey(card), root: card.root, status: card.state.status }]
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

/** Label of a run in the picker. */
export function runLabel(run: RunChoice, now: number): string {
  return `${runTitle(run.status)} (${run.status.status}, ${ageText(run.status.updated, now)})`;
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
