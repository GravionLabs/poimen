/** Elapsed times of agent statuses, as the cards show them. */

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

/** The mark of a silent agent, `silent 12m`; the server decided it is silent, the client only formats. */
export function silentLabel(silentSince: number | undefined, now: number): string | undefined {
  return silentSince === undefined ? undefined : `silent ${formatElapsed(now - silentSince)}`;
}
