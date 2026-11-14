/** How long no status may change before a sort by status is applied (#712). */
export const SORT_SETTLE_MS = 3000;

/** What {@link SortSettle.update} returns: the keys to sort by and when to look again. */
export interface Settled<V> {
  keys: ReadonlyMap<string, V>;
  /** Milliseconds until `update` must be called again to apply waiting changes; 0 when none wait. */
  wait: number;
}

/**
 * Holds back a re-sort caused by changing keys (statuses) until none changed for `settleMs`, so
 * rows do not jump while the user aims at one. A first call, an `immediate` one (mode change,
 * focus change) and a change of the set of ids (new or closed item) apply at once. Pure: the
 * clock is a parameter; the caller owns the timer.
 */
export class SortSettle<V> {
  private shown: ReadonlyMap<string, V> | null = null;
  private live: ReadonlyMap<string, V> | null = null;
  private changedAt = 0;

  constructor(
    private readonly now: () => number,
    private readonly settleMs = SORT_SETTLE_MS,
  ) {}

  update(live: ReadonlyMap<string, V>, immediate = false): Settled<V> {
    const now = this.now();
    if (!this.shown || immediate || !sameIds(this.shown, live)) {
      this.shown = live;
      this.live = live;
      return { keys: live, wait: 0 };
    }
    if (!sameValues(this.live!, live)) {
      this.live = live;
      this.changedAt = now;
    }
    if (sameValues(this.shown, live)) return { keys: this.shown, wait: 0 };
    const wait = this.changedAt + this.settleMs - now;
    if (wait <= 0) {
      this.shown = live;
      return { keys: live, wait: 0 };
    }
    return { keys: this.shown, wait };
  }
}

function sameIds<V>(a: ReadonlyMap<string, V>, b: ReadonlyMap<string, V>): boolean {
  return a.size === b.size && [...a.keys()].every((k) => b.has(k));
}

function sameValues<V>(a: ReadonlyMap<string, V>, b: ReadonlyMap<string, V>): boolean {
  return sameIds(a, b) && [...a].every(([k, v]) => b.get(k) === v);
}
