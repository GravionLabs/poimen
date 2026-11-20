import { splitId, type SpaceNode } from "../core/snapshot";
import type { UiState } from "../core/ui-state";
import { spaceContext } from "./spaces";
import type { MenuGroup } from "../ui/context-menu/menu-model";

export type SortMode = "manual" | "name" | "attention" | "recent";

/** The sort entries of the sidebar: the modes as radios, and "sort now". */
export function sortGroups(mode: SortMode): MenuGroup[] {
  const radio = (id: SortMode, label: string) => ({ id, label, checked: mode === id });
  return [
    {
      id: "modes",
      items: [
        radio("manual", "Manual"),
        radio("name", "By name"),
        radio("attention", "By attention"),
        radio("recent", "Most recent"),
      ],
    },
    { id: "now", items: [{ id: "now", label: "Sort alphabetically now" }] },
  ];
}

/**
 * The entries the card header gave up as buttons (#1075): configure (not for a machine), copy path,
 * the details line, and the sort entries in their groups.
 */
export function cardMenuGroups(id: string, ctx: CardMenuContext): MenuGroup[] {
  const { sort, ui } = ctx;
  return [
    {
      id: "card",
      items: [
        ...(splitId(id).machine === undefined
          ? [{ id: "configure-space", label: "Configure space..." }]
          : []),
        ...(ctx.dir(id) ? [{ id: "copy-path", label: "Copy path" }] : []),
        {
          id: "toggle-details",
          label: ui.hiddenDetails().includes(id) ? "Show details" : "Hide details",
        },
      ],
    },
    ...sortGroups(sort()).map((g, i) => ({
      ...g,
      id: "sort-" + g.id,
      label: i === 0 ? "Sort spaces" : null,
      items: g.items.map((item) => ({ ...item, id: "sort:" + item.id })),
    })),
  ];
}

/** What the card menu needs from the sidebar. */
export interface CardMenuContext {
  ui: Pick<UiState, "hiddenDetails" | "setDetailsHidden">;
  sort: () => SortMode;
  dir: (id: string) => string | undefined;
  configure: (id: string) => void;
  chooseSort: (mode: string) => void;
}

/** Runs a picked id of {@link cardMenuGroups}; `false` for an id of the built-in menu. */
export function runCardEntry(picked: string, id: string, ctx: CardMenuContext): boolean {
  const entry = cardEntry(picked);
  if (entry === null) return false;
  if (entry.kind === "sort") ctx.chooseSort(entry.mode);
  else if (entry.kind === "configure") ctx.configure(id);
  else if (entry.kind === "details")
    ctx.ui.setDetailsHidden(id, !ctx.ui.hiddenDetails().includes(id));
  else copyText(ctx.dir(id));
  return true;
}

export type CardEntry = { kind: "sort"; mode: string } | { kind: "configure" | "details" | "path" };

/** What a picked id of {@link cardMenuGroups} stands for; `null` for an id of the built-in menu. */
export function cardEntry(id: string): CardEntry | null {
  if (id.startsWith("sort:")) return { kind: "sort", mode: id.slice(5) };
  const kinds: Record<string, "configure" | "details" | "path"> = {
    "configure-space": "configure",
    "toggle-details": "details",
    "copy-path": "path",
  };
  const kind = kinds[id];
  return kind ? { kind } : null;
}

/** Puts `text` on the clipboard; nothing for an empty text. */
export function copyText(text: string | undefined): void {
  if (text) void Promise.resolve(navigator.clipboard?.writeText(text)).catch(() => undefined);
}

/** The directory of the space `id` among `spaces`; `undefined` when it has none. */
export function spaceDirOf(spaces: SpaceNode[], id: string): string | undefined {
  const space = spaces.find((s) => s.workspace.space_id === id);
  return space && spaceContext(space, false).dir;
}
