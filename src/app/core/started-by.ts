import { qualifyId, rowName, type SpaceNode, type Snapshot, type Terminal } from "./snapshot";

/**
 * The terminal whose agent started `pane` through `start_agent` (#982), looked up in
 * `terminals` (a remote pane's `started_by` is the machine's own id, so it is qualified here);
 * `undefined` when it was started by nobody or the starter is gone.
 */
export function starterOf(terminals: readonly Terminal[], pane: Terminal): Terminal | undefined {
  if (!pane.started_by) return undefined;
  const id = pane.machine ? qualifyId(pane.machine, pane.started_by) : pane.started_by;
  return terminals.find((t) => t.terminal_id === id);
}

/** `name (space)` of the terminal that started `pane`, when it sits in another space. */
export function startedByOtherSpace(snapshot: Snapshot, pane: Terminal): string | undefined {
  const starter = starterOf(snapshot.terminals, pane);
  if (!starter || starter.space_id === pane.space_id) return undefined;
  const space = snapshot.spaces.find((s) => s.space_id === starter.space_id)?.label;
  return `${rowName(starter)} (${space ?? "other space"})`;
}

/** Sets `startedBy` on the rows whose starter is in another space; a starter of the same space nests (`nestTabs`). */
export function markStarters(nodes: readonly SpaceNode[], snapshot: Snapshot): void {
  for (const row of nodes.flatMap((n) => n.rows)) {
    const by = startedByOtherSpace(snapshot, row.pane);
    if (by) row.startedBy = by;
  }
}
