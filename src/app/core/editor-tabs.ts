import { Injectable, computed, signal } from "@angular/core";
import type { Mode } from "./herdr";
import {
  groupIds,
  removeGroup,
  setSplitSizes,
  splitGroup,
  type LayoutNode,
  type SplitDirection,
} from "./editor-layout";

/** An open editor tab. A preview tab (not pinned) is replaced by the next preview, as in VS Code. */
export interface EditorTab {
  paneId: string;
  pinned: boolean;
}

/** One editor group: its own tabs and active tab. */
export interface EditorGroup {
  id: number;
  tabs: readonly EditorTab[];
  activeId: string | null;
}

/** `Cmd+\` / `Ctrl+\` splits right, with Shift down; `null` for other keys. */
export function splitKeyDirection(
  event: Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">,
): SplitDirection | null {
  const backslash = event.key === "\\" || event.key === "|";
  if (!backslash || !(event.metaKey || event.ctrlKey) || event.altKey) return null;
  return event.shiftKey ? "column" : "row";
}

/** Moves the item at `from` to index `to`; out-of-range or no-op moves return the same array. */
export function moveItem<T>(list: readonly T[], from: number, to: number): readonly T[] {
  if (from === to || from < 0 || from >= list.length || to < 0 || to >= list.length) return list;
  const next = [...list];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

function without<V>(map: ReadonlyMap<string, V>, key: string): ReadonlyMap<string, V> {
  const next = new Map(map);
  next.delete(key);
  return next;
}

/**
 * The editor area: groups (each with its own tabs) laid out as a tree of splits, the focused group
 * and each tab's read-only/input mode. A pane is open in at most one group; `tabs` and `activeId`
 * are those of the focused group. UI state only: closing a tab never touches the herdr pane.
 */
@Injectable({ providedIn: "root" })
export class EditorTabs {
  readonly groups = signal<readonly EditorGroup[]>([{ id: 1, tabs: [], activeId: null }]);
  readonly layout = signal<LayoutNode>({ kind: "group", id: 1 });
  readonly focusedId = signal(1);
  private nextId = 2;

  private readonly focused = computed(
    () => this.groups().find((g) => g.id === this.focusedId()) ?? this.groups()[0],
  );
  readonly tabs = computed(() => this.focused().tabs);
  readonly activeId = computed(() => this.focused().activeId);
  /** The mode each stream really has, reported by its terminal view. */
  private readonly modes = signal<ReadonlyMap<string, Mode>>(new Map());
  /** Why a tab is read-only although input was wanted (another client holds the controller). */
  private readonly blockedBy = signal<ReadonlyMap<string, string>>(new Map());
  /** Modes the user chose per tab; a tab without an entry follows the `autoControl` setting. */
  private readonly choices = signal<ReadonlyMap<string, Mode>>(new Map());

  readonly openIds = computed(
    () => new Set(this.groups().flatMap((g) => g.tabs.map((t) => t.paneId))),
  );

  /** The group a pane is open in. */
  groupOf(paneId: string): EditorGroup | undefined {
    return this.groups().find((g) => g.tabs.some((t) => t.paneId === paneId));
  }

  /**
   * Opens a pane. An open pane is only activated (and pinned when `pin`). A new pane replaces the
   * focused group's preview tab in place, or is appended; `pin` makes it a pinned tab.
   */
  open(paneId: string, pin = false): void {
    if (this.groupOf(paneId)) {
      if (pin) this.pin(paneId);
      this.activate(paneId);
      return;
    }
    const group = this.focused();
    const tab: EditorTab = { paneId, pinned: pin };
    const previewAt = group.tabs.findIndex((t) => !t.pinned);
    let tabs: readonly EditorTab[];
    if (previewAt >= 0) {
      const replaced = group.tabs[previewAt].paneId;
      tabs = group.tabs.map((t, i) => (i === previewAt ? tab : t));
      this.forget(replaced);
    } else {
      tabs = [...group.tabs, tab];
    }
    this.patch(group.id, { tabs, activeId: paneId });
  }

  pin(paneId: string): void {
    const group = this.groupOf(paneId);
    if (!group) return;
    this.patch(group.id, {
      tabs: group.tabs.map((t) => (t.paneId === paneId ? { ...t, pinned: true } : t)),
    });
  }

  /** Activates a tab and focuses its group. */
  activate(paneId: string): void {
    const group = this.groupOf(paneId);
    if (!group) return;
    this.patch(group.id, { activeId: paneId });
    this.focusedId.set(group.id);
  }

  focusGroup(groupId: number): void {
    if (this.groups().some((g) => g.id === groupId)) this.focusedId.set(groupId);
  }

  /**
   * Closes a tab; the neighbor to its right (else left) becomes active when it was active. The last
   * tab of a group removes the group, unless it is the only one.
   */
  close(paneId: string): void {
    if (this.detach(paneId)) this.forget(paneId);
  }

  closeAll(): void {
    this.groups.set([{ id: 1, tabs: [], activeId: null }]);
    this.layout.set({ kind: "group", id: 1 });
    this.focusedId.set(1);
    this.nextId = 2;
    this.modes.set(new Map());
    this.blockedBy.set(new Map());
    this.choices.set(new Map());
  }

  /** Reorders tabs inside a group (default: the focused one). */
  move(from: number, to: number, groupId = this.focusedId()): void {
    const group = this.groups().find((g) => g.id === groupId);
    if (group) this.patch(groupId, { tabs: moveItem(group.tabs, from, to) });
  }

  /**
   * Moves a tab into a new group beside its own (`row`: right, `column`: below) and focuses it.
   * A group with a single tab cannot split (it would leave an empty group).
   */
  split(paneId: string, dir: SplitDirection): void {
    const group = this.groupOf(paneId);
    if (!group || group.tabs.length < 2) return;
    const tab = { ...group.tabs.find((t) => t.paneId === paneId)!, pinned: true };
    this.detach(paneId);
    const id = this.nextId++;
    const splitId = this.nextId++;
    this.groups.update((gs) => [...gs, { id, tabs: [tab], activeId: paneId }]);
    this.layout.update((l) => splitGroup(l, group.id, id, dir, splitId));
    this.focusedId.set(id);
  }

  /** Moves a tab into another group at `index` (default: the end); an emptied group is removed. */
  moveToGroup(paneId: string, targetId: number, index?: number): void {
    const source = this.groupOf(paneId);
    const target = this.groups().find((g) => g.id === targetId);
    if (!source || !target || source.id === targetId) return;
    const tab = { ...source.tabs.find((t) => t.paneId === paneId)!, pinned: true };
    this.detach(paneId);
    const at = Math.min(Math.max(index ?? Infinity, 0), target.tabs.length);
    const tabs = [...target.tabs];
    tabs.splice(at, 0, tab);
    this.patch(targetId, { tabs, activeId: paneId });
    this.focusedId.set(targetId);
  }

  setSizes(splitId: number, sizes: readonly number[]): void {
    this.layout.update((l) => setSplitSizes(l, splitId, sizes));
  }

  mode(paneId: string): Mode {
    return this.modes().get(paneId) ?? "observe";
  }

  /** The mode a tab asks for: the user's choice, else input when `autoControl` is on. */
  wanted(paneId: string, autoControl: boolean): Mode {
    return this.choices().get(paneId) ?? (autoControl ? "control" : "observe");
  }

  /** The user's choice for this tab; kept while the tab is open. */
  choose(paneId: string, mode: Mode): void {
    this.choices.update((m) => new Map(m).set(paneId, mode));
  }

  /** The reason the tab is read-only although input was wanted, else `null`. */
  blocked(paneId: string): string | null {
    return this.blockedBy().get(paneId) ?? null;
  }

  /** Records the real mode of a tab's stream. */
  report(paneId: string, mode: Mode, blocked: string | null): void {
    this.modes.update((m) => new Map(m).set(paneId, mode));
    this.blockedBy.update((m) => {
      const next = new Map(m);
      if (blocked) next.set(paneId, blocked);
      else next.delete(paneId);
      return next;
    });
  }

  private patch(groupId: number, change: Partial<Omit<EditorGroup, "id">>): void {
    this.groups.update((gs) => gs.map((g) => (g.id === groupId ? { ...g, ...change } : g)));
  }

  /** Takes a tab out of its group, fixing the active tab and removing a group left empty. */
  private detach(paneId: string): boolean {
    const group = this.groupOf(paneId);
    if (!group) return false;
    const index = group.tabs.findIndex((t) => t.paneId === paneId);
    const rest = group.tabs.filter((t) => t.paneId !== paneId);
    const activeId =
      group.activeId === paneId
        ? (rest[Math.min(index, rest.length - 1)]?.paneId ?? null)
        : group.activeId;
    if (rest.length === 0 && this.groups().length > 1) this.removeEmpty(group.id);
    else this.patch(group.id, { tabs: rest, activeId });
    return true;
  }

  private removeEmpty(groupId: number): void {
    const order = groupIds(this.layout());
    const at = order.indexOf(groupId);
    this.layout.update((l) => removeGroup(l, groupId) ?? l);
    this.groups.update((gs) => gs.filter((g) => g.id !== groupId));
    if (this.focusedId() === groupId) this.focusedId.set(order[at + 1] ?? order[at - 1]);
  }

  private forget(paneId: string): void {
    this.modes.update((m) => without(m, paneId));
    this.blockedBy.update((m) => without(m, paneId));
    this.choices.update((m) => without(m, paneId));
  }
}
