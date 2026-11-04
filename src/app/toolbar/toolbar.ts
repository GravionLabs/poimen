import { ChangeDetectionStrategy, Component, input, output } from "@angular/core";
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

  /** Whether the output panel is shown. */
  readonly outputOpen = input(false);
  /** The user clicked the Output button. */
  readonly outputToggle = output<void>();

  protected readonly label = themeLabel;
  protected readonly fixed = fixedLabel;
  protected readonly next = nextTheme;
}
