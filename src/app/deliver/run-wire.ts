/** Small readings of a run as the backend and the legacy status file send it. */

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

/** Only a `/pull/<n>` URL is a pull request; the producer once wrote the issue URL (#538). */
export const asPullUrl = (v: unknown): string | null =>
  typeof v === "string" && /\/pull\/\d+\/?$/.test(v) ? v : null;

/**
 * `run.finished` lists the open tasks as cancelled (agent-events 0.3, #1408); `leftOpen` has them
 * as they were, and the views show those as left open.
 */
export function withLeftOpen(run: unknown): unknown {
  if (!isObject(run)) return run;
  const left = run["leftOpen"];
  const finished = run["finished"];
  if (!Array.isArray(left) || !isObject(finished)) return run;
  const open = new Set(left.map((t: unknown) => (isObject(t) ? t["task"] : null)));
  const cancelled = finished["cancelled"];
  return {
    ...run,
    tasks: left,
    finished: {
      ...finished,
      cancelled: Array.isArray(cancelled)
        ? cancelled.filter((t: unknown) => !(isObject(t) && open.has(t["task"])))
        : [],
    },
  };
}
