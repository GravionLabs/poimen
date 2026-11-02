import { ChangeDetectionStrategy, Component, input, output } from "@angular/core";

/** The icons the activity bar can draw; SVG keeps them the same size in every font. */
export type ActivityIcon = "spaces" | "folder";

export interface ActivityView {
  id: string;
  label: string;
  /** Which inline SVG icon the button shows. */
  icon: ActivityIcon;
}

/** The views the activity bar offers; plugins and notifications follow later. */
export const ACTIVITY_VIEWS: readonly ActivityView[] = [
  { id: "spaces", label: "Spaces", icon: "spaces" },
  { id: "explorer", label: "Explorer", icon: "folder" },
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
  /** The gear at the bottom was clicked: open the settings tab. */
  readonly settingsOpened = output<void>();
}
