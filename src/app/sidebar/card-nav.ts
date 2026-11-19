import type { HxTreeNode } from "@gravionlabs/helix-ui";

/** Keyboard navigation of the cards layout (#937): pure, items in display order in, target out. */

/** One stop of the list: a card header, or a visible row of an expanded card. */
export type CardItem =
  | { kind: "header"; key: string; expanded: boolean; hasBody: boolean }
  | { kind: "row"; key: string; header: string; level: number };

/** What a key does: move the cursor, toggle a card, or open the item under it. */
export type CardNavResult =
  | { action: "focus"; key: string }
  | { action: "toggle"; key: string; expanded: boolean }
  | { action: "open"; key: string }
  | null;

/** A card as the list sees it: the header key, its rows (depth first) and whether it is expanded. */
export interface CardNode {
  key: string;
  expanded: boolean;
  hasBody: boolean;
  rows: { key: string; depth: number }[];
}

/** The stops in display order: each header, followed by its rows when the card is expanded. */
export function cardItems(cards: readonly CardNode[]): CardItem[] {
  return cards.flatMap((c): CardItem[] => [
    { kind: "header", key: c.key, expanded: c.expanded, hasBody: c.hasBody },
    ...(c.expanded && c.hasBody
      ? c.rows.map((r): CardItem => ({
          kind: "row",
          key: r.key,
          header: c.key,
          level: 2 + r.depth,
        }))
      : []),
  ]);
}

/** The key the roving tabindex gives to the tab stop: the cursor if it is visible, else the first. */
export function tabStop(items: readonly CardItem[], cursor: string | null): string | null {
  return items.find((i) => i.key === cursor)?.key ?? items[0]?.key ?? null;
}

/** What `key` does with the cursor on `current`; `null` when the key is not handled here. */
export function cardNav(
  items: readonly CardItem[],
  current: string | null,
  key: string,
): CardNavResult {
  if (items.length === 0) return null;
  const at = items.findIndex((i) => i.key === current);
  const item = items[at];
  const go = (i: number): CardNavResult => ({ action: "focus", key: items[i]!.key });
  switch (key) {
    case "ArrowDown":
      return go(at < 0 ? 0 : Math.min(items.length - 1, at + 1));
    case "ArrowUp":
      return go(at < 0 ? items.length - 1 : Math.max(0, at - 1));
    case "Home":
      return go(0);
    case "End":
      return go(items.length - 1);
    case "ArrowRight":
      if (item?.kind !== "header" || !item.hasBody) return null;
      if (!item.expanded) return { action: "toggle", key: item.key, expanded: true };
      return items[at + 1]?.kind === "row" ? go(at + 1) : null;
    case "ArrowLeft":
      if (item?.kind === "row") {
        const header = items.findIndex((i) => i.key === item.header);
        return header >= 0 ? go(header) : null;
      }
      if (item?.kind === "header" && item.hasBody && item.expanded)
        return { action: "toggle", key: item.key, expanded: false };
      return null;
    case "Enter":
      if (!item) return null;
      if (item.kind === "row") return { action: "open", key: item.key };
      return item.hasBody
        ? { action: "toggle", key: item.key, expanded: !item.expanded }
        : { action: "open", key: item.key };
    default:
      return null;
  }
}

const WORKSPACE_PREFIX = "workspace:";

/** The stops of the `spaceNodes` of the cards: a filter shows every card expanded. */
export function cardStopsOf(
  nodes: readonly HxTreeNode[],
  collapsed: readonly string[],
  filtering: boolean,
): CardItem[] {
  const rowsOf = (list: readonly HxTreeNode[], depth: number): CardNode["rows"] =>
    list.flatMap((n) => [
      { key: (n.data as { row: { pane: { terminal_id: string } } }).row.pane.terminal_id, depth },
      ...rowsOf(n.children ?? [], depth + 1),
    ]);
  return cardItems(
    nodes.map((n) => {
      const id = n.key.slice(WORKSPACE_PREFIX.length);
      return {
        key: id,
        expanded: filtering || !collapsed.includes(id),
        hasBody: true,
        rows: rowsOf(n.children ?? [], 0),
      };
    }),
  );
}
