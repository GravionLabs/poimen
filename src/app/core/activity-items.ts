import type { TemplateRef } from "@angular/core";
import type { HxActivityBarItem } from "@gravionlabs/helix-desktop";

/** What a click on an item of the activity bar does. */
export type ActivityTarget =
  /** Switches the sidebar view. */
  | { kind: "view" }
  /** Opens an editor tab. */
  | { kind: "tab"; tab: "github" | "deliver" | "settings" };

interface ActivityEntry {
  id: string;
  label: string;
  position: "top" | "bottom";
  target: ActivityTarget;
}

/** The entries of the activity bar, in order: the views, the tabs, then Settings at the bottom. */
export const ACTIVITY_ENTRIES: readonly ActivityEntry[] = [
  { id: "spaces", label: "Spaces", position: "top", target: { kind: "view" } },
  { id: "explorer", label: "Explorer", position: "top", target: { kind: "view" } },
  { id: "notifications", label: "Notifications", position: "top", target: { kind: "view" } },
  { id: "github", label: "GitHub", position: "top", target: { kind: "tab", tab: "github" } },
  { id: "deliver", label: "Runs", position: "top", target: { kind: "tab", tab: "deliver" } },
  {
    id: "settings",
    label: "Settings",
    position: "bottom",
    target: { kind: "tab", tab: "settings" },
  },
];

/** The ids of the views that switch the sidebar. */
export const SIDEBAR_VIEW_IDS: readonly string[] = ACTIVITY_ENTRIES.filter(
  (e) => e.target.kind === "view",
).map((e) => e.id);

/** The tab an id opens, or undefined for a sidebar view and an unknown id. */
export function tabOf(id: string): "github" | "deliver" | "settings" | undefined {
  const target = ACTIVITY_ENTRIES.find((e) => e.id === id)?.target;
  return target?.kind === "tab" ? target.tab : undefined;
}

/** The items of `hx-activity-bar`; `icons` has the SVG template of each id. */
export function activityItems(
  icons: Readonly<Record<string, TemplateRef<unknown>>>,
): HxActivityBarItem[] {
  return ACTIVITY_ENTRIES.flatMap((entry) => {
    const icon = icons[entry.id];
    if (!icon) return [];
    return [
      {
        id: entry.id,
        label: entry.label,
        icon,
        position: entry.position,
        kind: entry.target.kind === "view" ? ("view" as const) : ("action" as const),
      },
    ];
  });
}
