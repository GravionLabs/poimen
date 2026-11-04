import { Injectable, computed, inject, signal } from "@angular/core";
import { fuzzyFilter } from "./fuzzy";
import type { PluginAction } from "./herdr";
import { Keymap } from "./keymap";
import type { ShortcutAction } from "./shortcuts";
import { PluginActions } from "./plugin-actions";
import { PluginPanes, type PluginPaneDef } from "./plugin-panes";

/** A poimen command the palette offers; `id` is what the app runs (`App.run`). */
export interface PoimenCommand {
  id: ShortcutAction | "openSettings" | "closeSpace";
  title: string;
}

export const POIMEN_COMMANDS: readonly PoimenCommand[] = [
  { id: "newTerminal", title: "New Terminal..." },
  { id: "closeSpace", title: "Close Space" },
  { id: "closeTab", title: "Close Tab" },
  { id: "closeAll", title: "Close All Tabs" },
  { id: "refresh", title: "Refresh" },
  { id: "toggleSidebar", title: "Toggle Sidebar" },
  { id: "splitRight", title: "Split Right" },
  { id: "splitDown", title: "Split Down" },
  { id: "zoomIn", title: "Zoom In" },
  { id: "zoomOut", title: "Zoom Out" },
  { id: "zoomReset", title: "Reset Zoom" },
  { id: "nextWaiting", title: "Next Waiting Agent" },
  { id: "toggleOutput", title: "Toggle Output Panel" },
  { id: "nextTab", title: "Next Tab" },
  { id: "previousTab", title: "Previous Tab" },
  { id: "showShortcuts", title: "Keyboard Shortcuts" },
  { id: "openSettings", title: "Open Settings" },
];

/** A row of the palette: a poimen command or a herdr plugin action. */
export type PaletteItem =
  | { kind: "command"; key: string; title: string; hint: string; command: PoimenCommand["id"] }
  | { kind: "plugin"; key: string; title: string; hint: string; action: PluginAction }
  | { kind: "pane"; key: string; title: string; hint: string; def: PluginPaneDef };

/**
 * The command palette: poimen commands plus every herdr plugin action (`plugin.action.list`),
 * fuzzy filtered. herdr owns the actions: they are re-read each time the palette opens.
 */
@Injectable({ providedIn: "root" })
export class CommandPalette {
  private readonly plugins = inject(PluginActions);
  private readonly panes = inject(PluginPanes);
  private readonly keymap = inject(Keymap);

  private readonly _open = signal(false);
  private readonly _query = signal("");
  private readonly _selected = signal(0);

  readonly isOpen = this._open.asReadonly();
  readonly query = this._query.asReadonly();
  /** Why the plugin actions could not be listed; the poimen commands still work. */
  readonly loadError = this.plugins.error;

  private readonly all = computed<PaletteItem[]>(() => [
    ...POIMEN_COMMANDS.map((c): PaletteItem => ({
      kind: "command",
      key: `command:${c.id}`,
      title: c.title,
      hint: c.id === "openSettings" || c.id === "closeSpace" ? "" : this.keymap.label(c.id),
      command: c.id,
    })),
    ...this.plugins.actions().map((a): PaletteItem => ({
      kind: "plugin",
      key: `plugin:${a.plugin_id}:${a.action_id}`,
      title: a.title,
      hint: a.plugin_id,
      action: a,
    })),
    ...this.panes.defs().map((d): PaletteItem => ({
      kind: "pane",
      key: `pane:${d.plugin_id}:${d.entrypoint}`,
      title: `Open ${d.title} as tab`,
      hint: d.plugin_name,
      def: d,
    })),
  ]);

  /** The rows matching the query, best first. */
  readonly items = computed(() =>
    fuzzyFilter(this.all(), this._query(), (i) =>
      i.kind === "command" ? i.title : `${i.title} ${i.hint}`,
    ),
  );

  /** Index of the highlighted row, kept inside the list. */
  readonly selected = computed(() =>
    Math.max(0, Math.min(this._selected(), this.items().length - 1)),
  );

  /** Opens the palette and re-reads the plugin actions. */
  async open(): Promise<void> {
    this._query.set("");
    this._selected.set(0);
    this._open.set(true);
    await Promise.all([this.plugins.load(), this.panes.load()]);
  }

  close(): void {
    this._open.set(false);
  }

  toggle(): void {
    if (this._open()) this.close();
    else void this.open();
  }

  setQuery(query: string): void {
    this._query.set(query);
    this._selected.set(0);
  }

  /** Highlights row `index` (mouse hover). */
  hover(index: number): void {
    this._selected.set(index);
  }

  /** Moves the highlight by `delta`, wrapping around. */
  move(delta: number): void {
    const n = this.items().length;
    if (n > 0) this._selected.set((this.selected() + delta + n) % n);
  }

  /** The highlighted row, if any. */
  current(): PaletteItem | undefined {
    return this.items()[this.selected()];
  }

  /** Runs a plugin action with `paneId` as context; see {@link PluginActions.invoke}. */
  invoke(action: PluginAction, paneId: string | null): Promise<string | null> {
    return this.plugins.invoke(action, paneId);
  }
}
