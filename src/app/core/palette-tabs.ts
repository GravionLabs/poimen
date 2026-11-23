/** Where a palette entry comes from; the tabs and the source label in "All" read it. */
export type PaletteSource = "poimen" | "snippet" | "github" | "project" | "scripts";

/** The label of a source: the group heading without a query, the tag with one. */
export const SOURCE_LABELS: Readonly<Record<PaletteSource, string>> = {
  poimen: "Poimen",
  snippet: "Snippets",
  github: "GitHub",
  project: "Project",
  scripts: "Scripts",
};

/** A tab of the palette; `sources` is `null` for All (every source). */
export interface PaletteTab {
  id: string;
  label: string;
  sources: readonly PaletteSource[] | null;
}

export const ALL_TAB = "all";

/** The tabs in display order. A new source adds an entry here, the component stays as it is. */
export const PALETTE_TABS: readonly PaletteTab[] = [
  { id: ALL_TAB, label: "All", sources: null },
  { id: "commands", label: "Commands", sources: ["poimen", "snippet", "github"] },
  { id: "project", label: "Project", sources: ["project"] },
  { id: "scripts", label: "Scripts", sources: ["scripts"] },
];

/** Whether an entry of `source` belongs on `tab`. */
export function inTab(tab: PaletteTab, source: PaletteSource): boolean {
  return tab.sources === null || tab.sources.includes(source);
}

/** The tabs to show: All, and every other tab that has an entry among `sources`. */
export function visibleTabs(
  sources: ReadonlySet<PaletteSource>,
  tabs: readonly PaletteTab[] = PALETTE_TABS,
): PaletteTab[] {
  return tabs.filter((t) => t.sources === null || t.sources.some((s) => sources.has(s)));
}

/** The id of the tab `delta` steps from `current` among `visible`, wrapping around. */
export function stepTab(visible: readonly PaletteTab[], current: string, delta: number): string {
  const at = Math.max(
    0,
    visible.findIndex((t) => t.id === current),
  );
  return visible[(at + delta + visible.length) % visible.length]?.id ?? ALL_TAB;
}
