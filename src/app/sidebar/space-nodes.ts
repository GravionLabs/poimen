import type { HxTreeNode } from "@gravionlabs/helix-ui";
import type { AgentStatus, PaneRow, Space, SpaceNode } from "../core/snapshot";
import {
  DEFAULT_SIDEBAR_ROWS,
  aggregateStatus,
  renderRow,
  rowType,
  spaceContext,
  type Line,
  type SidebarRows,
} from "./spaces";

/** What a tree node carries back to the template. */
export type SpaceData =
  | {
      kind: "workspace";
      workspace: Space;
      status: AgentStatus;
      lines: Line[];
      /** Split layout only: the workspace whose terminals are listed. */
      selected?: boolean;
      /** A linked worktree listed below the space of its repository: indented. */
      member?: boolean;
    }
  | {
      kind: "pane";
      row: PaneRow;
      lines: Line[];
      /** Agents section: time since the last status change (`5m`, `2h`, `3d`). */
      since?: string;
    };

/** Tree keys: the pane id for a pane, a prefixed workspace id for a workspace. */
export const WORKSPACE_PREFIX = "workspace:";

/** Tree nodes of the (filtered) spaces; a workspace starts expanded (helix only applies it once). */
export function spaceNodes(
  spaces: SpaceNode[],
  rows: SidebarRows = DEFAULT_SIDEBAR_ROWS,
  showBase = false,
): HxTreeNode<SpaceData>[] {
  return spaces.map((space) => {
    const ctx = spaceContext(space, showBase);
    const status = aggregateStatus(space);
    return {
      key: WORKSPACE_PREFIX + space.workspace.space_id,
      label: space.workspace.label,
      expanded: true,
      data: {
        kind: "workspace",
        workspace: space.workspace,
        status,
        lines: renderRow("workspace", rows.workspace, space, ctx),
        member: !!space.parent,
      },
      children: nestTabs(space.rows, (row) => ({
        key: row.pane.terminal_id,
        label: row.name,
        leaf: true,
        data: {
          kind: "pane",
          row,
          lines: renderRow(rowType(row), rows[rowType(row)], row, ctx),
        },
      })),
    };
  });
}

/**
 * Nodes of pane rows nested by tab (#656): the rows of a tab with `parent` become children of the
 * first row of that parent tab, in tab order; the other rows of a tab stay siblings of its first
 * row. A tab whose parent has no row here (or a parent cycle) is top level. A node with children
 * is expandable and starts expanded.
 */
export function nestTabs(
  rows: readonly PaneRow[],
  make: (row: PaneRow) => HxTreeNode<SpaceData>,
): HxTreeNode<SpaceData>[] {
  const entries = rows.map((row) => ({ row, node: make(row) }));
  const firstOf = new Map<string, HxTreeNode<SpaceData>>();
  const parentOf = new Map<string, string | undefined>();
  for (const { row, node } of entries) {
    if (!row.tabId) continue;
    if (!firstOf.has(row.tabId)) firstOf.set(row.tabId, node);
    parentOf.set(row.tabId, row.tabParent);
  }
  const containerOf = (tab: string | undefined): HxTreeNode<SpaceData> | undefined => {
    const parent = tab ? parentOf.get(tab) : undefined;
    if (!tab || !parent || !firstOf.has(parent)) return undefined;
    // Only a chain that comes back to this tab is a cycle; one that joins another is fine.
    const seen = new Set<string>();
    for (let up: string | undefined = parent; up && !seen.has(up); up = parentOf.get(up)) {
      if (up === tab) return undefined;
      seen.add(up);
    }
    return firstOf.get(parent);
  };
  const top: HxTreeNode<SpaceData>[] = [];
  for (const { row, node } of entries) {
    const container = containerOf(row.tabId);
    if (!container) {
      top.push(node);
      continue;
    }
    container.children = [...(container.children ?? []), node];
    container.leaf = false;
    container.expanded = true;
  }
  return nestStarted(entries, top);
}

/**
 * Nodes of agents started by another agent of the same list (`started_by`, #982) become children
 * of the starting agent's node, which starts expanded. A start chain that comes back to the row
 * (a cycle) and a row nested under a tab parent are left where they are.
 */
function nestStarted(
  entries: readonly { row: PaneRow; node: HxTreeNode<SpaceData> }[],
  top: HxTreeNode<SpaceData>[],
): HxTreeNode<SpaceData>[] {
  const byTerminal = new Map(entries.map((e) => [e.row.pane.terminal_id, e]));
  const startedBy = (id: string) => byTerminal.get(id)?.row.pane.started_by;
  const keep: HxTreeNode<SpaceData>[] = [];
  for (const node of top) {
    const entry = entries.find((e) => e.node === node)!;
    const starter = entry.row.pane.started_by
      ? byTerminal.get(entry.row.pane.started_by)
      : undefined;
    let cycle = false;
    const seen = new Set<string>();
    for (let up = entry.row.pane.started_by; up && !seen.has(up); up = startedBy(up)) {
      if (up === entry.row.pane.terminal_id) cycle = true;
      seen.add(up);
    }
    if (!starter || starter.node === node || cycle) {
      keep.push(node);
      continue;
    }
    starter.node.children = [...(starter.node.children ?? []), node];
    starter.node.leaf = false;
    starter.node.expanded = true;
  }
  return keep;
}
