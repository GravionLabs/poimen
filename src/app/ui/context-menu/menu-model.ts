import type { IconName } from "../icon/icon";

/** One entry of a context menu. */
export interface MenuEntry {
  /** What `picked` reports. */
  id: string;
  label: string;
  /** A quiet hint after the label (a key chord). */
  shortcut?: string;
  /** `true`, or the reason: it shows as the tooltip and the entry does nothing. */
  disabled?: boolean | string;
  /** Makes the entry a radio item with this state. */
  checked?: boolean;
  /** A mark after the label. */
  mark?: "attention" | "unread";
  /** A decorative icon before the label (the tab kind in the "All tabs" menu). */
  icon?: IconName;
  /** Bold, for the current choice of a list. */
  active?: boolean;
  testId?: string;
}

/** A run of entries; groups are separated by a line, `label` is the heading (plugin groups, #273). */
export interface MenuGroup {
  id: string;
  label?: string | null;
  items: readonly MenuEntry[];
}

/** What a pick reports: the entry and whether Shift was held. */
export interface MenuPick {
  id: string;
  shiftKey: boolean;
}

/** One group without a heading, for a menu that has no groups. */
export function plainGroups(items: readonly MenuEntry[]): MenuGroup[] {
  return [{ id: "items", items }];
}

/**
 * The index of the menu item a key moves to among `labels` (focusable items only, so separators
 * and headings never count), or `null` when the key does nothing. `current` is -1 when no item has
 * focus; arrows wrap; a single character jumps to the next label that starts with it.
 */
export function menuTarget(labels: readonly string[], current: number, key: string): number | null {
  const n = labels.length;
  if (n === 0) return null;
  switch (key) {
    case "ArrowDown":
      return current < 0 ? 0 : (current + 1) % n;
    case "ArrowUp":
      return current < 0 ? n - 1 : (current - 1 + n) % n;
    case "Home":
      return 0;
    case "End":
      return n - 1;
  }
  if (key.length !== 1 || key === " ") return null;
  const c = key.toLowerCase();
  for (let i = 1; i <= n; i++) {
    const idx = (Math.max(current, -1) + i) % n;
    if (labels[idx].toLowerCase().startsWith(c)) return idx;
  }
  return null;
}

/**
 * Where a menu of `size` goes when it is wanted at `at` in a window of `view`: inside the window
 * with a `margin`, flipped to the left of `at` when `alignRight`, never above or left of the margin.
 */
export function clampMenu(
  at: { x: number; y: number },
  size: { width: number; height: number },
  view: { width: number; height: number },
  alignRight = false,
  margin = 4,
): { left: number; top: number } {
  const wanted = alignRight ? at.x - size.width : at.x;
  const left = Math.max(margin, Math.min(wanted, view.width - size.width - margin));
  const top = Math.max(margin, Math.min(at.y, view.height - size.height - margin));
  return { left, top };
}
