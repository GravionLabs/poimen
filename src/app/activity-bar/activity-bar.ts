import { ChangeDetectionStrategy, Component, input, output } from "@angular/core";

export interface ActivityView {
  id: string;
  label: string;
  /** Single glyph shown on the button. */
  icon: string;
}

/** The views the activity bar offers; plugins and notifications follow later. */
export const ACTIVITY_VIEWS: readonly ActivityView[] = [
  { id: "spaces", label: "Spaces", icon: "▤" },
];

/** Narrow icon rail at the far left; clicking a view selects it. */
@Component({
  selector: "app-activity-bar",
  templateUrl: "./activity-bar.html",
  styleUrl: "./activity-bar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ActivityBar {
  readonly views = input<readonly ActivityView[]>(ACTIVITY_VIEWS);
  readonly active = input("spaces");
  /** True while the sidebar is shown, so the active view looks selected only then. */
  readonly expanded = input(true);
  readonly selected = output<string>();
}
