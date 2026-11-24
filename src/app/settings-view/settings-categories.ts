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
          "Terminal font family",
          "Terminal font size",
          "UI font family",
          "UI font size",
          "Sounds",
          "Sound when blocked",
          "Sound when done",
        ],
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
        id: "spaces",
        title: "Templates",
        labels: [
          "Space templates",
          "Template",
          "Add template",
          "Name",
          "Directory",
          "Remote",
          "Agent",
          "Model",
          "Layout",
        ],
      },
      {
        id: "layouts",
        title: "Layouts",
        labels: [
          "Named layouts",
          "New layout",
          "Duplicate",
          "Tabs",
          "Add tab",
          "Indent",
          "Outdent",
          "Terminal",
          "Split tree",
          "Place",
          "Preview",
        ],
      },
      {
        id: "snippets",
        title: "Snippets",
        labels: ["Add snippet", "Label", "Text", "Send to", "Press Enter"],
      },
      {
        id: "palette",
        title: "Palette",
        labels: ["Detected sources", "Hidden entries", "Scripts", "Skills"],
      },
      {
        id: "keyboard",
        title: "Keyboard",
        labels: ["Record", "Reset", "Clear", "Unbound"],
      },
      { id: "trusted-projects", title: "Trusted projects", labels: ["Revoke"] },
    ],
  },
  {
    id: "backend",
    title: "Backend",
    pages: [
      {
        id: "server",
        title: "Server",
        labels: [
          "Terminal history",
          "Keep terminal history",
          "Limit per terminal",
          "Limit in total",
          "Agent hooks and MCP",
          "Install hooks",
        ],
      },
      {
        id: "notifications",
        title: "Notifications",
        labels: ["Blocked", "Done", "Test", "Command"],
      },
      {
        id: "machines",
        title: "Machines",
        labels: ["Remote machines", "Check machine"],
      },
      {
        id: "agents",
        title: "Agents",
        labels: [
          "Harnesses",
          "Models",
          "Agent actions",
          "Claude Code: classic renderer",
          "Keep the machine awake",
        ],
      },
    ],
  },
  {
    id: "general",
    title: "General",
    pages: [
      {
        id: "transfer",
        title: "Export & import",
        labels: ["Export settings", "Import settings"],
      },
    ],
  },
];

export const DEFAULT_SETTINGS_PAGE = SETTINGS_GROUPS[0].pages[0].id;

/** Retired page ids, mapped to the page that now holds their settings (persisted selections, deep links). */
const RETIRED_PAGES: ReadonlyMap<string, string> = new Map([
  ["color-scheme", "appearance"],
  // The page was named after herdr, the former backend (#174).
  ["herdr", "server"],
  // "New space layout" is the layout `default` of the Layouts page (#855).
  ["space-layout", "layouts"],
]);

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
