import type { Terminal } from "../core/snapshot";
import { offeredFor, type Snippet } from "../core/snippets-model";
import type { MenuGroup } from "../ui/context-menu/menu-model";

const PREFIX = "snippet:";

/** The "Snippets" group of a pane's menu: the snippets that suit the pane; `null` when none do. */
export function snippetMenuGroup(
  snippets: readonly Snippet[],
  pane: Terminal | undefined,
): MenuGroup | null {
  const items = snippets
    .filter((s) => offeredFor(s, pane))
    .map((s) => ({ id: `${PREFIX}${s.label}`, label: s.label }));
  return items.length === 0 ? null : { id: "snippets", label: "Snippets", items };
}

/** The snippet a picked menu entry id names, or `null` for any other entry. */
export function snippetOf(picked: string, snippets: readonly Snippet[]): Snippet | null {
  if (!picked.startsWith(PREFIX)) return null;
  return snippets.find((s) => s.label === picked.slice(PREFIX.length)) ?? null;
}
