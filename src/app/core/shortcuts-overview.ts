import { Injectable, computed, inject, signal } from "@angular/core";
import { POIMEN_COMMANDS } from "./command-palette";
import { Keymap } from "./keymap";
import type { ShortcutAction } from "./shortcuts";

export interface ShortcutGroup {
  title: string;
  actions: readonly ShortcutAction[];
}

/** The groups of the overview; every `ShortcutAction` belongs to one (tested). */
export const SHORTCUT_GROUPS: readonly ShortcutGroup[] = [
  { title: "General", actions: ["commandPalette", "showShortcuts", "refresh"] },
  {
    title: "Tabs and panes",
    actions: [
      "newTerminal",
      "closeTab",
      "closeAll",
      "nextTab",
      "previousTab",
      "splitRight",
      "splitDown",
    ],
  },
  { title: "View", actions: ["toggleSidebar", "toggleOutput", "zoomIn", "zoomOut", "zoomReset"] },
  { title: "Agents", actions: ["nextWaiting"] },
];

/** The title of an action, from the same list as the command palette (which does not list itself). */
export function shortcutTitle(action: ShortcutAction): string {
  if (action === "commandPalette") return "Command Palette";
  return POIMEN_COMMANDS.find((c) => c.id === action)?.title ?? action;
}

export interface ShortcutRow {
  action: ShortcutAction;
  title: string;
  /** Display keys, `null` when the action is unbound. */
  keys: string | null;
}

export interface ShortcutSection {
  title: string;
  rows: ShortcutRow[];
}

/** Whether the keyboard shortcuts overview is open, and its content from the effective keymap. */
@Injectable({ providedIn: "root" })
export class ShortcutsOverview {
  private readonly keymap = inject(Keymap);
  private readonly _open = signal(false);
  readonly isOpen = this._open.asReadonly();

  readonly sections = computed<ShortcutSection[]>(() =>
    SHORTCUT_GROUPS.map((g) => ({
      title: g.title,
      rows: g.actions.map((action) => ({
        action,
        title: shortcutTitle(action),
        keys: this.keymap.label(action) || null,
      })),
    })),
  );

  /** Display keys of the command palette for the hint. */
  readonly paletteKeys = computed(() => this.keymap.label("commandPalette") || "from the menu");

  open(): void {
    this._open.set(true);
  }

  close(): void {
    this._open.set(false);
  }
}
