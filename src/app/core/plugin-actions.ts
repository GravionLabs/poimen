import { Injectable, inject, signal } from "@angular/core";
import { pluginContext, type PluginAction } from "./herdr";
import { HerdrStore } from "./herdr-store";
import { TauriBridge } from "./tauri";

const PREFIX = "plugin:";

/** The menu action id of a plugin action; menus carry it as an opaque string. */
export function pluginMenuId(action: Pick<PluginAction, "plugin_id" | "action_id">): string {
  return PREFIX + JSON.stringify([action.plugin_id, action.action_id]);
}

/** The plugin and action ids behind a menu action id, or `null` when it is not a plugin entry. */
export function parsePluginMenuId(id: string): { pluginId: string; actionId: string } | null {
  if (!id.startsWith(PREFIX)) return null;
  try {
    const [pluginId, actionId] = JSON.parse(id.slice(PREFIX.length)) as unknown[];
    return typeof pluginId === "string" && typeof actionId === "string"
      ? { pluginId, actionId }
      : null;
  } catch {
    return null;
  }
}

/**
 * The herdr plugin actions (`plugin.action.list`) for the Plugins menu, the pane context menus and
 * the command palette. herdr owns them: `load` re-reads the list.
 */
@Injectable({ providedIn: "root" })
export class PluginActions {
  private readonly bridge = inject(TauriBridge);
  private readonly store = inject(HerdrStore);
  private readonly _actions = signal<PluginAction[]>([]);
  private readonly _error = signal<string | null>(null);

  readonly actions = this._actions.asReadonly();
  /** Why the actions could not be listed; `null` when the last read worked. */
  readonly error = this._error.asReadonly();

  /** Re-reads the actions; on failure the list is empty and `error` says why. */
  async load(): Promise<void> {
    this._error.set(null);
    try {
      this._actions.set(await this.bridge.invoke<PluginAction[]>("plugin_actions"));
    } catch (err) {
      this._actions.set([]);
      this._error.set(String(err));
    }
  }

  /** The action behind a menu action id, if herdr still lists it. */
  find(menuId: string): PluginAction | undefined {
    const ids = parsePluginMenuId(menuId);
    return ids
      ? this._actions().find((a) => a.plugin_id === ids.pluginId && a.action_id === ids.actionId)
      : undefined;
  }

  /**
   * Runs an action through herdr with `paneId` and its tab and workspace as context. Resolves to
   * `null`, or to the error text when herdr refused it.
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
