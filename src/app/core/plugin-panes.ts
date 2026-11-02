import { Injectable, inject, signal } from "@angular/core";
import { HerdrStore } from "./herdr-store";
import { TauriBridge } from "./tauri";

/** A pane a plugin declares (`panes[]` of `plugin.list`). */
export interface PluginPaneDef {
  plugin_id: string;
  plugin_name: string;
  entrypoint: string;
  title: string;
}

/** A plugin pane herdr opened, focused or closed (`plugin.pane.*`). */
export interface PluginPane {
  plugin_id: string;
  entrypoint: string;
  pane_id: string;
}

/** The plugin panes poimen knows by pane id, with the plugin name their tab shows. */
export type PluginPaneNames = ReadonlyMap<string, string>;

/**
 * Plugin panes as tabs. herdr owns the panes: this only remembers which pane id belongs to which
 * plugin (herdr 22 does not mark them in the snapshot) so the tab can carry the plugin name, and
 * forwards focus and close to `plugin.pane.focus` / `plugin.pane.close`.
 */
@Injectable({ providedIn: "root" })
export class PluginPanes {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);
  private readonly _defs = signal<PluginPaneDef[]>([]);
  private readonly _names = signal<PluginPaneNames>(new Map());

  /** The panes the enabled plugins declare; empty when herdr could not list them. */
  readonly defs = this._defs.asReadonly();
  /** Plugin name per known plugin pane id. */
  readonly names = this._names.asReadonly();

  /** Re-reads the declared panes. */
  async load(): Promise<void> {
    try {
      this._defs.set(await this.bridge.invoke<PluginPaneDef[]>("plugin_pane_defs"));
    } catch {
      this._defs.set([]);
    }
  }

  /** Remembers that `paneId` belongs to the plugin `name` . */
  track(paneId: string, name: string): void {
    this._names.update((names) => new Map(names).set(paneId, name));
  }

  /** Whether `paneId` is a plugin pane poimen knows. */
  isPluginPane(paneId: string): boolean {
    return this._names().has(paneId);
  }

  /**
   * Opens the pane through herdr as a tab. Resolves to the new pane id and the snapshot is re-read;
   * on refusal to `{ error }`.
   */
  async open(
    def: PluginPaneDef,
    workspaceId?: string,
  ): Promise<{ paneId: string } | { error: string }> {
    try {
      const pane = await this.bridge.invoke<PluginPane>("plugin_pane_open", {
        pluginId: def.plugin_id,
        entrypoint: def.entrypoint,
        workspaceId,
      });
      this.track(pane.pane_id, def.plugin_name);
      await this.store.refresh();
      return { paneId: pane.pane_id };
    } catch (err) {
      return { error: `${def.title}: ${String(err)}` };
    }
  }

  /** Brings a plugin pane to the front in herdr; a pane that is not a plugin pane is left alone. */
  async focus(paneId: string): Promise<void> {
    if (!this.isPluginPane(paneId)) return;
    try {
      await this.bridge.invoke("plugin_pane_focus", { paneId });
    } catch {
      // the pane may be gone already; the snapshot closes its tab
    }
  }

  /** Closes the plugin pane in herdr (explicit user action). Resolves to the error text, if any. */
  async close(paneId: string): Promise<string | null> {
    if (!this.isPluginPane(paneId)) return null;
    try {
      await this.bridge.invoke("plugin_pane_close", { paneId });
      this._names.update((names) => {
        const next = new Map(names);
        next.delete(paneId);
        return next;
      });
      return null;
    } catch (err) {
      return String(err);
    }
  }
}
