/** Elapsed times of agent statuses, as the cards show them. */

import type { Terminal } from "./snapshot";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** Time since a status change as `now` (under a minute), `5m`, `2h` or `3d`; a future time is `now`. */
export function formatElapsed(ms: number): string {
  if (!(ms >= MINUTE)) return "now";
  if (ms < HOUR) return `${Math.floor(ms / MINUTE)}m`;
  if (ms < DAY) return `${Math.floor(ms / HOUR)}h`;
  return `${Math.floor(ms / DAY)}d`;
}

/** The tooltip of a tab with the silent label after it (#1149); the tab itself shows no text. */
export function withSilent(title: string, silentSince: number | undefined, now: number): string {
  return silentSince === undefined ? title : `${title} · ${silentLabel(silentSince, now)}`;
}

/** The tooltip of a tab whose agent is done with a warning (#1152): the program's message follows. */
export function withWarning(title: string, warning: boolean, message: string | undefined): string {
  if (!warning) return title;
  const text = message?.trim();
  return `${title} · done with a warning${text ? `: ${text}` : ""}`;
}

/** The tooltip of an agent tab: the title with the silent time and a warning with its message. */
export function tabMarks(title: string, pane: Terminal | undefined, now: number): string {
  const silent = withSilent(title, pane?.agent_silent_since, now);
  return withWarning(silent, pane?.agent_tone !== undefined, pane?.agent_message);
}

/** The mark of a silent agent, `silent 12m`; the server decided it is silent, the client only formats. */
export function silentLabel(silentSince: number | undefined, now: number): string | undefined {
  return silentSince === undefined ? undefined : `silent ${formatElapsed(now - silentSince)}`;
}
