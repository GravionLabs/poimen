/** A page of the settings view: one section of settings, shown on the right. */
export interface SettingsPage {
  /** Stable id: persisted as the selected page and used by deep links. */
  readonly id: string;
  readonly title: string;
  /** The labels of the settings on the page; the search matches them and highlights the hits. */
  readonly labels: readonly string[];
}

export interface SettingsGroup {
  readonly id: string;
  readonly title: string;
  readonly pages: readonly SettingsPage[];
}

/**
 * The category tree of the settings view, the one place that defines it: a new page is an entry
 * here plus a `@case` in the view template.
 */
export const SETTINGS_GROUPS: readonly SettingsGroup[] = [
  {
    id: "appearance-behavior",
    title: "Appearance & Behavior",
    pages: [
      {
        id: "appearance",
        title: "Appearance",
        labels: [
          "Light",
          "Dark",
          "System",
          "Single scheme",
          "Light/dark pair",
          "Dark schemes",
          "Light schemes",
        ],
      },
      {
        id: "notifications",
        title: "Notifications",
        labels: ["When an agent waits for you", "When an agent is done"],
      },
    ],
  },
  {
    id: "workspace",
    title: "Workspace",
    pages: [
      {
        id: "explorer",
        title: "Explorer",
        labels: ["Explorer folders", "Add folder", "Show hidden folders"],
      },
      {
        id: "space-layout",
        title: "New space layout",
        labels: ["Panes of a new space", "Name", "Command", "Split from", "Direction", "Keeps"],
      },
      { id: "trusted-projects", title: "Trusted projects", labels: ["Revoke"] },
    ],
  },
  {
    id: "backend",
    title: "Backend",
    pages: [
      {
        id: "herdr",
        title: "Server",
        labels: ["Start the backend server when it is not running", "Agent hooks", "Install hooks"],
      },
      { id: "agents", title: "Agents", labels: ["Harnesses", "Models"] },
    ],
  },
];

export const DEFAULT_SETTINGS_PAGE = SETTINGS_GROUPS[0].pages[0].id;

/** Retired page ids, mapped to the page that now holds their settings (persisted selections, deep links). */
const RETIRED_PAGES: ReadonlyMap<string, string> = new Map([["color-scheme", "appearance"]]);

export function allPages(groups: readonly SettingsGroup[] = SETTINGS_GROUPS): SettingsPage[] {
  return groups.flatMap((g) => g.pages);
}

/** The page with `id` (a retired id resolves to its replacement) and its group, or `null`. */
export function findPage(
  id: string | null,
  groups: readonly SettingsGroup[] = SETTINGS_GROUPS,
): { group: SettingsGroup; page: SettingsPage } | null {
  const target = id === null ? null : (RETIRED_PAGES.get(id) ?? id);
  for (const group of groups) {
    const page = group.pages.find((p) => p.id === target);
    if (page) return { group, page };
  }
  return null;
}

/** Whether `text` contains the trimmed `query`, ignoring case; an empty query matches nothing. */
export function matches(text: string, query: string): boolean {
  const q = query.trim().toLowerCase();
  return q !== "" && text.toLowerCase().includes(q);
}

/** The pages whose title or a setting label match `query`, with their group; all for an empty query. */
export function filterGroups(
  query: string,
  groups: readonly SettingsGroup[] = SETTINGS_GROUPS,
): SettingsGroup[] {
  if (query.trim() === "") return [...groups];
  return groups
    .map((g) => ({
      ...g,
      pages: g.pages.filter(
        (p) => matches(p.title, query) || p.labels.some((l) => matches(l, query)),
      ),
    }))
    .filter((g) => g.pages.length > 0);
}
