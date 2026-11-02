import { Injectable, computed, inject, signal } from "@angular/core";
import { fuzzyFilter } from "./fuzzy";
import { pluginContext, type PluginAction } from "./herdr";
import { HerdrStore } from "./herdr-store";
import { Keymap } from "./keymap";
import type { ShortcutAction } from "./shortcuts";
import { TauriBridge } from "./tauri";

/** A poimen command the palette offers; `id` is what the app runs (`App.run`). */
export interface PoimenCommand {
  id: ShortcutAction | "openSettings";
  title: string;
}

export const POIMEN_COMMANDS: readonly PoimenCommand[] = [
  { id: "newTerminal", title: "New Terminal..." },
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
  { id: "nextTab", title: "Next Tab" },
  { id: "previousTab", title: "Previous Tab" },
  { id: "openSettings", title: "Open Settings" },
];

/** A row of the palette: a poimen command or a herdr plugin action. */
export type PaletteItem =
  | { kind: "command"; key: string; title: string; hint: string; command: PoimenCommand["id"] }
  | { kind: "plugin"; key: string; title: string; hint: string; action: PluginAction };

/**
 * The command palette: poimen commands plus every herdr plugin action (`plugin.action.list`),
 * fuzzy filtered. herdr owns the actions: they are re-read each time the palette opens.
 */
@Injectable({ providedIn: "root" })
export class CommandPalette {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);
  private readonly keymap = inject(Keymap);

  private readonly _open = signal(false);
  private readonly _query = signal("");
  private readonly _selected = signal(0);
  private readonly _actions = signal<PluginAction[]>([]);
  private readonly _loadError = signal<string | null>(null);

  readonly isOpen = this._open.asReadonly();
  readonly query = this._query.asReadonly();
  /** Why the plugin actions could not be listed; the poimen commands still work. */
  readonly loadError = this._loadError.asReadonly();

  private readonly all = computed<PaletteItem[]>(() => [
    ...POIMEN_COMMANDS.map((c): PaletteItem => ({
      kind: "command",
      key: `command:${c.id}`,
      title: c.title,
      hint: c.id === "openSettings" ? "" : this.keymap.label(c.id),
      command: c.id,
    })),
    ...this._actions().map((a): PaletteItem => ({
      kind: "plugin",
      key: `plugin:${a.plugin_id}:${a.action_id}`,
      title: a.title,
      hint: a.plugin_id,
      action: a,
    })),
  ]);

  /** The rows matching the query, best first. */
  readonly items = computed(() =>
    fuzzyFilter(this.all(), this._query(), (i) =>
      i.kind === "plugin" ? `${i.title} ${i.hint}` : i.title,
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
    this._loadError.set(null);
    try {
      this._actions.set(await this.bridge.invoke<PluginAction[]>("plugin_actions"));
    } catch (err) {
      this._actions.set([]);
      this._loadError.set(String(err));
    }
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

  /**
   * Runs a plugin action through herdr with `paneId` (the focused pane) and its workspace as
   * context. Resolves to `null`, or to the error text when herdr refused it.
   */
  async invoke(action: PluginAction, paneId: string | null): Promise<string | null> {
    try {
      await this.bridge.invoke("plugin_invoke", {
        pluginId: action.plugin_id,
        actionId: action.action_id,
        context: pluginContext(this.store.snapshot(), paneId),
      });
      return null;
    } catch (err) {
      return `${action.title}: ${String(err)}`;
    }
  }
}
