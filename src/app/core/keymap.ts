import { Injectable, computed, inject } from "@angular/core";
import { SettingsStore } from "./settings";
import { formatShortcut, isMac, matchShortcut, type ShortcutAction } from "./shortcuts";

/** The effective shortcuts (settings file over the defaults) and the event matcher. */
@Injectable({ providedIn: "root" })
export class Keymap {
  private readonly prefs = inject(SettingsStore);
  readonly shortcuts = computed(() => this.prefs.settings().shortcuts);
  readonly mac = isMac();

  match(event: KeyboardEvent, inTerminal: boolean): ShortcutAction | null {
    return matchShortcut(event, this.shortcuts(), { mac: this.mac, inTerminal });
  }

  /** Display text of an action's shortcut for menus and tooltips; empty when unbound. */
  label(action: ShortcutAction): string {
    return formatShortcut(this.shortcuts()[action], this.mac);
  }
}
