import { ChangeDetectionStrategy, Component, input, output } from "@angular/core";
import { nextTheme, type ThemeSetting } from "../core/settings";

/** Tooltip and accessible name of the theme button: the current value and what a click selects. */
export function themeLabel(theme: ThemeSetting): string {
  return `Theme: ${theme} (click for ${nextTheme(theme)})`;
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
  /** The user clicked the theme button: carries the next theme. */
  readonly themeChange = output<ThemeSetting>();

  protected readonly label = themeLabel;
  protected readonly next = nextTheme;
}
