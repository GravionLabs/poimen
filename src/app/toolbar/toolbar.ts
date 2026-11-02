import { ChangeDetectionStrategy, Component, input, output, signal } from "@angular/core";
import type { PluginAction } from "../core/herdr";
import { nextTheme, type ThemeSetting } from "../core/settings";

/** Tooltip and accessible name of the theme button: the current value and what a click selects. */
export function themeLabel(theme: ThemeSetting): string {
  return `Theme: ${theme} (click for ${nextTheme(theme)})`;
}

/** Tooltip and accessible name of the disabled theme button: which color schema fixes it. */
export function fixedLabel(scheme: string): string {
  return `Theme fixed by color schema ${scheme}`;
}

/** Top bar of the window; holds the theme button. */
@Component({
  selector: "app-toolbar",
  templateUrl: "./toolbar.html",
  styleUrl: "./toolbar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Toolbar {
  readonly theme = input<ThemeSetting>("system");
  /** The color schema that fixes light or dark; the theme button is disabled while set. */
  readonly fixedBy = input<string | null>(null);
  /** The user clicked the theme button: carries the next theme. */
  readonly themeChange = output<ThemeSetting>();

  /** The herdr plugin actions the Plugins menu lists. */
  readonly plugins = input<readonly PluginAction[]>([]);
  /** Why the plugin actions could not be listed, if so. */
  readonly pluginsError = input<string | null>(null);
  /** The Plugins menu is about to open: the host re-reads the actions. */
  readonly pluginsOpened = output<void>();
  /** An action of the Plugins menu was picked. */
  readonly pluginChosen = output<PluginAction>();
  protected readonly pluginsOpen = signal(false);

  protected togglePlugins(): void {
    const open = !this.pluginsOpen();
    this.pluginsOpen.set(open);
    if (open) this.pluginsOpened.emit();
  }

  protected choosePlugin(action: PluginAction): void {
    this.pluginsOpen.set(false);
    this.pluginChosen.emit(action);
  }

  protected readonly label = themeLabel;
  protected readonly fixed = fixedLabel;
  protected readonly next = nextTheme;
}
