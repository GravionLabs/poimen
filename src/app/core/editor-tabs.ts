import { Injectable, computed, signal } from "@angular/core";
import type { Mode } from "./herdr";

/** An open editor tab. A preview tab (not pinned) is replaced by the next preview, as in VS Code. */
export interface EditorTab {
  paneId: string;
  pinned: boolean;
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
 * Open tabs of the single editor group, the active one and each tab's read-only/input mode.
 * UI state only: closing a tab never touches the herdr pane.
 */
@Injectable({ providedIn: "root" })
export class EditorTabs {
  readonly tabs = signal<readonly EditorTab[]>([]);
  readonly activeId = signal<string | null>(null);
  /** The mode each stream really has, reported by its terminal view. */
  private readonly modes = signal<ReadonlyMap<string, Mode>>(new Map());
  /** Why a tab is read-only although input was wanted (another client holds the controller). */
  private readonly blockedBy = signal<ReadonlyMap<string, string>>(new Map());
  /** Modes the user chose per tab; a tab without an entry follows the `autoControl` setting. */
  private readonly choices = signal<ReadonlyMap<string, Mode>>(new Map());

  readonly openIds = computed(() => new Set(this.tabs().map((t) => t.paneId)));

  /**
   * Opens a pane. An open pane is only activated (and pinned when `pin`). A new pane replaces the
   * preview tab in place, or is appended; `pin` makes it a pinned tab.
   */
  open(paneId: string, pin = false): void {
    const tabs = this.tabs();
    const existing = tabs.find((t) => t.paneId === paneId);
    if (existing) {
      if (pin && !existing.pinned) this.pin(paneId);
      this.activeId.set(paneId);
      return;
    }
    const tab: EditorTab = { paneId, pinned: pin };
    const previewAt = tabs.findIndex((t) => !t.pinned);
    if (previewAt >= 0) {
      const replaced = tabs[previewAt].paneId;
      this.tabs.set(tabs.map((t, i) => (i === previewAt ? tab : t)));
      this.forget(replaced);
    } else {
      this.tabs.set([...tabs, tab]);
    }
    this.activeId.set(paneId);
  }

  pin(paneId: string): void {
    this.tabs.update((tabs) => tabs.map((t) => (t.paneId === paneId ? { ...t, pinned: true } : t)));
  }

  activate(paneId: string): void {
    if (this.openIds().has(paneId)) this.activeId.set(paneId);
  }

  /** Closes a tab; the neighbor to its right (else left) becomes active when it was active. */
  close(paneId: string): void {
    const tabs = this.tabs();
    const index = tabs.findIndex((t) => t.paneId === paneId);
    if (index < 0) return;
    const rest = tabs.filter((t) => t.paneId !== paneId);
    this.tabs.set(rest);
    this.forget(paneId);
    if (this.activeId() === paneId) {
      this.activeId.set(rest[Math.min(index, rest.length - 1)]?.paneId ?? null);
    }
  }

  closeAll(): void {
    this.tabs.set([]);
    this.activeId.set(null);
    this.modes.set(new Map());
    this.blockedBy.set(new Map());
    this.choices.set(new Map());
  }

  move(from: number, to: number): void {
    this.tabs.update((tabs) => moveItem(tabs, from, to));
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

  private forget(paneId: string): void {
    this.modes.update((m) => without(m, paneId));
    this.blockedBy.update((m) => without(m, paneId));
    this.choices.update((m) => without(m, paneId));
  }
}
