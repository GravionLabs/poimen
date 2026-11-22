import { Injectable, effect, inject, signal, untracked } from "@angular/core";
import { EditorTabs, tabKind, type EditorGroup } from "../core/editor-tabs";

/** How long a hidden terminal view keeps its xterm and stream (#978). */
export const HIDDEN_VIEW_MS = 30_000;
/** The most hidden views kept at once, over all groups; the one hidden longest ago goes first. */
export const HIDDEN_VIEW_LIMIT = 3;

/**
 * Which terminal views of each editor group stay mounted: the active pane tab and the ones hidden
 * in the last {@link HIDDEN_VIEW_MS}, at most {@link HIDDEN_VIEW_LIMIT} of them. The app renders a
 * mounted view with `hidden` when it is not the active tab, so coming back shows it without a new
 * attach; once it expires the view is destroyed as before (stream closed, xterm disposed).
 */
@Injectable({ providedIn: "root" })
export class HiddenViews {
  private readonly editor = inject(EditorTabs);
  private readonly mounted = signal<ReadonlyMap<number, readonly string[]>>(new Map());
  /** The active pane of each group at the last look. */
  private readonly lastActive = new Map<number, string | null>();
  /** Hidden panes in the order they were hidden, with the timer that expires each. */
  private readonly hidden = new Map<string, ReturnType<typeof setTimeout>>();

  constructor() {
    effect(() => {
      const groups = this.editor.groups();
      untracked(() => this.observe(groups));
    });
  }

  /** The pane ids to render in a group, in a stable order (the active one is among them). */
  of(groupId: number): readonly string[] {
    return this.mounted().get(groupId) ?? [];
  }

  /** Follows the groups: parks the pane a group left, forgets closed ones. */
  observe(groups: readonly EditorGroup[]): void {
    const next = new Map<number, string[]>();
    const live = new Set<string>();
    for (const group of groups) {
      const active = group.activeId && tabKind(group.activeId) === "pane" ? group.activeId : null;
      const before = this.lastActive.get(group.id) ?? null;
      const open = new Set(group.tabs.map((t) => t.paneId));
      const kept = (this.mounted().get(group.id) ?? []).filter((id) => open.has(id));
      if (active) this.unpark(active);
      if (before && before !== active && open.has(before) && kept.includes(before))
        this.park(before);
      if (active && !kept.includes(active)) kept.push(active);
      this.lastActive.set(group.id, active);
      next.set(group.id, kept);
      for (const id of kept) live.add(id);
    }
    for (const id of [...this.hidden.keys()]) if (!live.has(id)) this.unpark(id);
    for (const id of [...this.lastActive.keys()]) {
      if (!groups.some((g) => g.id === id)) this.lastActive.delete(id);
    }
    // A pane hidden before the first look has no entry: only mounted panes can be parked.
    this.mounted.set(this.trim(next));
  }

  private park(id: string): void {
    this.unpark(id);
    this.hidden.set(
      id,
      setTimeout(() => this.expire(id), HIDDEN_VIEW_MS),
    );
  }

  private unpark(id: string): void {
    const timer = this.hidden.get(id);
    if (timer === undefined) return;
    clearTimeout(timer);
    this.hidden.delete(id);
  }

  private expire(id: string): void {
    this.hidden.delete(id);
    this.mounted.update((m) => this.without(m, id));
  }

  /** Drops the oldest hidden views beyond the limit. */
  private trim(next: Map<number, string[]>): Map<number, string[]> {
    while (this.hidden.size > HIDDEN_VIEW_LIMIT) {
      const oldest = this.hidden.keys().next().value as string;
      this.unpark(oldest);
      for (const [group, ids] of next)
        next.set(
          group,
          ids.filter((id) => id !== oldest),
        );
    }
    return next;
  }

  private without(
    m: ReadonlyMap<number, readonly string[]>,
    id: string,
  ): ReadonlyMap<number, readonly string[]> {
    return new Map([...m].map(([group, ids]) => [group, ids.filter((i) => i !== id)]));
  }
}

/** The ids with only the first of those that share a split tab (`tabOf`), so a layout renders once. */
export function onePerTab(
  ids: readonly string[],
  tabOf: (id: string) => string | undefined,
): readonly string[] {
  const seen = new Set<string>();
  return ids.filter((id) => {
    const tab = tabOf(id);
    if (!tab) return true;
    if (seen.has(tab)) return false;
    seen.add(tab);
    return true;
  });
}
