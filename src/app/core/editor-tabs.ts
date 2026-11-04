import { Injectable, computed, effect, inject, signal } from "@angular/core";
import { UiState } from "./ui-state";
import type { Mode } from "./herdr";
import {
  groupIds,
  removeGroup,
  setSplitSizes,
  splitGroup,
  type LayoutNode,
  type SplitDirection,
} from "./editor-layout";

/** The id of the settings tab; it is not a herdr pane, so it never reaches the terminal logic. */
export const SETTINGS_TAB = "poimen:settings";

/** The id of the native deliver board tab (ADR 0004); like settings it is no herdr pane. */
export const BOARD_TAB = "poimen:deliver-board";

/** The id of the GitHub pull request tab; like settings it is no herdr pane. */
export const GITHUB_TAB = "poimen:github";

/** Prefix of a flow tab id; the rest is the id of the pane whose agent run the tab shows. */
const FLOW_TAB_PREFIX = "poimen:flow:";

/** The id of the flow tab of `paneId` (at most one per pane). */
export function flowTab(paneId: string): string {
  return FLOW_TAB_PREFIX + paneId;
}

/** The pane a flow tab belongs to; `null` for any other tab. */
export function flowPane(id: string): string | null {
  return id.startsWith(FLOW_TAB_PREFIX) ? id.slice(FLOW_TAB_PREFIX.length) : null;
}

/**
 * What a tab shows: a herdr pane (live terminal), the settings view, the deliver board or the flow
 * of a pane's agent run.
 */
export type TabKind = "pane" | "settings" | "board" | "github" | "flow";

export function tabKind(id: string): TabKind {
  if (id === SETTINGS_TAB) return "settings";
  if (id === BOARD_TAB) return "board";
  if (id === GITHUB_TAB) return "github";
  return flowPane(id) !== null ? "flow" : "pane";
}

/** The herdr pane a tab is about: the pane itself or the pane of a flow tab; else `null`. */
export function tabPaneId(id: string): string | null {
  const kind = tabKind(id);
  if (kind === "pane") return id;
  return kind === "flow" ? flowPane(id) : null;
}

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

/** Moves the item at `from` to index `to`; out-of-range or no-op moves return the same array. */
export function moveItem<T>(list: readonly T[], from: number, to: number): readonly T[] {
  if (from === to || from < 0 || from >= list.length || to < 0 || to >= list.length) return list;
  const next = [...list];
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item);
  return next;
}

/** The persisted part of the editor area; modes are not kept (they follow the settings). */
export interface EditorSnapshot {
  groups: readonly EditorGroup[];
  layout: LayoutNode;
  focusedId: number;
}

function validLayout(node: unknown, seen: number[]): node is LayoutNode {
  if (typeof node !== "object" || node === null) return false;
  const n = node as Record<string, unknown>;
  if (!Number.isInteger(n["id"])) return false;
  if (n["kind"] === "group") {
    seen.push(n["id"] as number);
    return true;
  }
  const { children, sizes, dir } = n;
  return (
    n["kind"] === "split" &&
    (dir === "row" || dir === "column") &&
    Array.isArray(children) &&
    children.length >= 2 &&
    Array.isArray(sizes) &&
    sizes.length === children.length &&
    sizes.every((s) => typeof s === "number" && Number.isFinite(s) && s > 0) &&
    children.every((c) => validLayout(c, seen))
  );
}

/** Checks stored editor state; anything inconsistent yields `null` (start with a blank editor). */
export function parseEditor(raw: unknown): EditorSnapshot | null {
  if (typeof raw !== "object" || raw === null) return null;
  const { groups, layout, focusedId } = raw as Record<string, unknown>;
  if (!Array.isArray(groups) || groups.length === 0) return null;
  const panes = new Set<string>();
  const ids: number[] = [];
  const clean: EditorGroup[] = [];
  for (const g of groups as Record<string, unknown>[]) {
    if (typeof g !== "object" || g === null || !Number.isInteger(g["id"])) return null;
    if (!Array.isArray(g["tabs"])) return null;
    const tabs: EditorTab[] = [];
    for (const t of g["tabs"] as Record<string, unknown>[]) {
      if (typeof t?.["paneId"] !== "string" || typeof t["pinned"] !== "boolean") return null;
      if (panes.has(t["paneId"])) return null;
      panes.add(t["paneId"]);
      tabs.push({ paneId: t["paneId"], pinned: t["pinned"] });
    }
    const active = g["activeId"];
    if (active !== null && !tabs.some((t) => t.paneId === active)) return null;
    ids.push(g["id"] as number);
    clean.push({ id: g["id"] as number, tabs, activeId: active as string | null });
  }
  const leaves: number[] = [];
  if (!validLayout(layout, leaves)) return null;
  const sorted = (a: number[]) => [...a].sort((x, y) => x - y).join();
  if (new Set(ids).size !== ids.length || sorted(ids) !== sorted(leaves)) return null;
  if (typeof focusedId !== "number" || !ids.includes(focusedId)) return null;
  return { groups: clean, layout, focusedId };
}

function maxId(node: LayoutNode): number {
  return node.kind === "group" ? node.id : Math.max(node.id, ...node.children.map(maxId));
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
  private readonly ui = inject(UiState);
  private readonly restored = parseEditor(this.ui.storedEditor);
  readonly groups = signal<readonly EditorGroup[]>(
    this.restored?.groups ?? [{ id: 1, tabs: [], activeId: null }],
  );
  readonly layout = signal<LayoutNode>(this.restored?.layout ?? { kind: "group", id: 1 });
  readonly focusedId = signal(this.restored?.focusedId ?? 1);
  private nextId = this.restored
    ? Math.max(...this.groups().map((g) => g.id), maxId(this.layout())) + 1
    : 2;

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

  constructor() {
    effect(() =>
      this.ui.saveEditor({
        groups: this.groups(),
        layout: this.layout(),
        focusedId: this.focusedId(),
      } satisfies EditorSnapshot),
    );
  }

  /**
   * Closes the tabs whose pane is not in `existing` (after a restart herdr may have lost them),
   * flow tabs included, and returns their ids.
   */
  prune(existing: ReadonlySet<string>): string[] {
    const gone = [...this.openIds()].filter((id) => {
      const paneId = tabPaneId(id);
      return paneId !== null && !existing.has(paneId);
    });
    gone.forEach((id) => this.close(id));
    return gone;
  }

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

  /** Opens the settings tab (at most one) as a pinned tab, or activates it when open. */
  openSettings(): void {
    this.openPinned(SETTINGS_TAB);
  }

  /** Opens the deliver board tab (at most one) as a pinned tab, or activates it when open. */
  openBoard(): void {
    this.openPinned(BOARD_TAB);
  }

  /** Opens the GitHub pull request tab (at most one) as a pinned tab, or activates it. */
  openGithub(): void {
    this.openPinned(GITHUB_TAB);
  }

  /** Opens the flow tab of `paneId` (at most one per pane) as a pinned tab, or activates it. */
  openFlow(paneId: string): void {
    this.openPinned(flowTab(paneId));
  }

  /** Appends a pinned tab to the focused group, or activates it when it is open anywhere. */
  private openPinned(id: string): void {
    if (this.groupOf(id)) {
      this.activate(id);
      return;
    }
    const group = this.focused();
    this.patch(group.id, { tabs: [...group.tabs, { paneId: id, pinned: true }], activeId: id });
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

  /** Activates the tab `delta` places after the active one in the focused group, wrapping around. */
  cycle(delta: number): void {
    const group = this.focused();
    if (group.tabs.length < 2) return;
    const at = group.tabs.findIndex((t) => t.paneId === group.activeId);
    const next = (at + delta + group.tabs.length) % group.tabs.length;
    this.patch(group.id, { activeId: group.tabs[next].paneId });
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
