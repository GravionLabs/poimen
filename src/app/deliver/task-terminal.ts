import { splitId, type Terminal } from "../core/snapshot";

/** What identifies the run a task belongs to: its id and where its repository is. */
export interface TaskRun {
  run: string;
  /** The machine of the run (#692); absent on this Mac. */
  machine?: string;
  /** The machine is not connected: its terminals are not offered. */
  offline?: boolean;
}

/** The creation order of a terminal: the counter in its id (`t12`, `box::t12`); ids without one come first. */
function creation(terminal: Terminal): number {
  const n = /(\d+)$/.exec(splitId(terminal.terminal_id).id);
  return n ? Number(n[1]) : -1;
}

/**
 * The terminals poimen-server started for `task` of a run (#745): the ones of the run's machine
 * (`machine` as in `qualifySnapshot`) whose `run` and `task` are those of the task, newest first
 * by creation (a retry starts another terminal). Only these two fields of the snapshot count: no
 * terminal output, no name; a session poimen did not start has neither and never matches. A run on
 * a machine that is offline has none.
 */
export function taskTerminals(
  terminals: readonly Terminal[],
  run: TaskRun,
  task: string,
): Terminal[] {
  if (run.offline || !run.run || !task) return [];
  return terminals
    .filter((t) => t.machine === run.machine && t.run === run.run && t.task === task)
    .sort((a, b) => creation(b) - creation(a));
}
