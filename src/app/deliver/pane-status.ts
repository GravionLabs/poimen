import type { StatusChange } from "../core/attention";
import type { AgentStatus } from "../core/herdr";
import { runState, type DeliverCard, type DeliverStatus } from "./deliver-status";

/** The key of `run.started`'s `host` that names the terminal a run belongs to. */
export const TERMINAL_KEY = "poimen.terminal";

/**
 * The status poimen shows for a pane: the hook status combined with the runs linked to it. A
 * running (not stale) run keeps the pane `working` after the agent's turn ended (Stop reports
 * `done`, the loop it started still runs); a run that needs the user, or a `blocked` hook, makes
 * it `blocked`. Without such a run the hook status is unchanged; `gone` always wins.
 */
export function effectiveStatus<S extends AgentStatus | "gone">(
  hook: S,
  runs: readonly DeliverStatus[],
  now: number,
): S | "working" | "blocked" {
  if (hook === "gone") return hook;
  const running = runs.filter((run) => runState(run, now) === "running");
  if (running.length === 0) return hook;
  return hook === "blocked" || running.some((run) => run.needsYou) ? "blocked" : "working";
}

/**
 * The runs per terminal. A run is linked to a pane only through `host["poimen.terminal"]`; the
 * fallback of `Deliver.paneOf` (an agent pane in the repository) does not link.
 */
export function runsByTerminal(
  cards: readonly DeliverCard[],
): ReadonlyMap<string, DeliverStatus[]> {
  const map = new Map<string, DeliverStatus[]>();
  for (const card of cards) {
    if (card.state.kind !== "ok") continue;
    const id = card.state.status.host[TERMINAL_KEY];
    if (!id) continue;
    map.set(id, [...(map.get(id) ?? []), card.state.status]);
  }
  return map;
}

/** The terminal a run that just finished (running before, finished now) was linked to. */
export function endedTerminal(
  prev: DeliverStatus | undefined,
  next: DeliverStatus,
): string | undefined {
  if (!prev || prev.run !== next.run) return undefined;
  return prev.status === "running" && next.status === "finished"
    ? next.host[TERMINAL_KEY]
    : undefined;
}

/**
 * Drops the `done` changes of panes whose linked run just ended: the run's own end-of-run
 * notification (`transitions`) already tells the user, so the pane going `working` to `done` must
 * not notify a second time. The pane keeps its status and unread mark (`Attention` saw it).
 */
export function withoutRunEnds(
  changes: readonly StatusChange[],
  ended: ReadonlySet<string>,
): StatusChange[] {
  return changes.filter((c) => !(c.status === "done" && ended.has(c.paneId)));
}
