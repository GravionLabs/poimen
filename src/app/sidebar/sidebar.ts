import {
  ChangeDetectionStrategy,
  Component,
  Injector,
  afterNextRender,
  computed,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { HxInput, HxMessage, HxTree, HxTreeNodeTemplate } from "@gravionlabs/helix-ui";
import type { HxTreeNode } from "@gravionlabs/helix-ui";
import {
  filterSpaces,
  type AgentStatus,
  type Pane,
  type PaneRow,
  type SpaceNode,
  type Workspace,
} from "../core/herdr";
import { HerdrStore } from "../core/herdr-store";
import {
  DEFAULT_SIDEBAR_ROWS,
  aggregateStatus,
  paneMarks,
  renderRow,
  spaceContext,
  type Line,
  type RowType,
  type SidebarRows,
} from "./spaces";
import { SettingsStore } from "../core/settings";

/** What a tree node carries back to the template. */
export type SpaceData =
  | { kind: "workspace"; workspace: Workspace; status: AgentStatus; lines: Line[] }
  | { kind: "pane"; row: PaneRow; lines: Line[] };

const rowType = (row: PaneRow): RowType => (row.agent ? "agent" : "terminal");

/** Texts of the shown tokens of a space and one of its panes: what the filter matches. */
export function filterValues(space: SpaceNode, row: PaneRow, rows: SidebarRows): string[] {
  const ctx = spaceContext(space);
  const texts = (lines: Line[]) => lines.flatMap((l) => l.map((s) => s.text));
  return [
    ...texts(renderRow(rowType(row), rows[rowType(row)], row, ctx)),
    ...texts(renderRow("workspace", rows.workspace, space, ctx)),
  ];
}

/** Tree keys: the pane id for a pane, a prefixed workspace id for a workspace. */
const WORKSPACE_PREFIX = "workspace:";

/** Tree nodes of the (filtered) spaces; a workspace starts expanded (helix only applies it once). */
export function spaceNodes(
  spaces: SpaceNode[],
  rows: SidebarRows = DEFAULT_SIDEBAR_ROWS,
): HxTreeNode<SpaceData>[] {
  return spaces.map((space) => {
    const ctx = spaceContext(space);
    const status = aggregateStatus(space);
    return {
      key: WORKSPACE_PREFIX + space.workspace.workspace_id,
      label: space.workspace.label,
      expanded: true,
      data: {
        kind: "workspace",
        workspace: space.workspace,
        status,
        lines: renderRow("workspace", rows.workspace, space, ctx),
      },
      children: space.rows.map((row) => ({
        key: row.pane.pane_id,
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

/** A row's context menu: `kind` says what was clicked, `action` what was picked. */
export interface SidebarMenuAction {
  kind: "workspace" | "pane";
  id: string;
  action: "new-terminal" | "split-right" | "split-down" | "start-agent" | "rename" | "close";
}

/** Entries of the context menu of a workspace row or a pane row. */
export function sidebarMenuItems(
  kind: SidebarMenuAction["kind"],
): { action: SidebarMenuAction["action"]; label: string }[] {
  return kind === "workspace"
    ? [{ action: "new-terminal", label: "New terminal..." }]
    : [
        { action: "split-right", label: "Split right" },
        { action: "split-down", label: "Split down" },
        { action: "start-agent", label: "Start agent..." },
        { action: "rename", label: "Rename..." },
        { action: "close", label: "Close pane..." },
      ];
}

export const DOUBLE_CLICK_MS = 400;

/** Whether a click on `key` at `now` completes a double click that started with `last`. */
export function isDoubleClick(
  last: { key: string; at: number } | null,
  key: string,
  now: number,
): boolean {
  return last !== null && last.key === key && now - last.at <= DOUBLE_CLICK_MS;
}

/** Workspaces and their panes with live agent status; a click or Enter opens the pane. */
@Component({
  selector: "app-sidebar",
  imports: [HxMessage, HxTree, HxTreeNodeTemplate, HxInput],
  templateUrl: "./sidebar.html",
  styleUrl: "./sidebar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class Sidebar {
  /** Ids of the panes that have an open view. */
  readonly openIds = input<ReadonlySet<string>>(new Set());
  /** Id of the pane whose tab is active in the editor. */
  readonly activeId = input<string | null>(null);
  readonly selected = output<Pane>();
  /** A second click on the same row within {@link DOUBLE_CLICK_MS}: pin the tab. */
  readonly pinned = output<Pane>();
  /** An entry of a row's context menu was picked. */
  readonly menuAction = output<SidebarMenuAction>();
  private last: { key: string; at: number } | null = null;
  protected readonly menuAt = signal<{
    kind: SidebarMenuAction["kind"];
    id: string;
    x: number;
    y: number;
  } | null>(null);
  protected readonly menuItems = sidebarMenuItems;

  protected openMenu(event: MouseEvent, kind: SidebarMenuAction["kind"], id: string): void {
    event.preventDefault();
    event.stopPropagation();
    this.menuAt.set({ kind, id, x: event.clientX, y: event.clientY });
  }

  protected closeMenu(): void {
    this.menuAt.set(null);
  }

  protected choose(action: SidebarMenuAction["action"]): void {
    const at = this.menuAt();
    this.closeMenu();
    if (at) this.menuAction.emit({ kind: at.kind, id: at.id, action });
  }

  private readonly injector = inject(Injector);
  protected readonly store = inject(HerdrStore);
  protected readonly query = signal("");
  protected readonly selection = signal<string | null>(null);

  /** The chevron at the right edge of a workspace row: expands or collapses it (not a selection). */
  protected toggle(tree: HxTree, key: string, event: Event): void {
    event.stopPropagation();
    tree.toggle(key);
  }

  protected marks(row: PaneRow) {
    return paneMarks(row.pane.pane_id, this.openIds(), this.activeId());
  }

  private readonly settings = inject(SettingsStore);
  private readonly rows = computed(() => this.settings.settings().sidebarRows);

  protected readonly nodes = computed(() => {
    const rows = this.rows();
    const spaces = this.store.spaces();
    const bySpace = new Map(spaces.flatMap((s) => s.rows.map((r) => [r, s] as const)));
    return spaceNodes(
      filterSpaces(spaces, this.query(), (row) => filterValues(bySpace.get(row)!, row, rows)),
      rows,
    );
  });

  protected data(node: HxTreeNode): SpaceData {
    return node.data as SpaceData;
  }

  protected onSelect(key: string | readonly string[] | null): void {
    if (typeof key !== "string") return;
    const row = this.store
      .spaces()
      .flatMap((s) => s.rows)
      .find((r) => r.pane.pane_id === key);
    // hx-tree skips equal values, so it reports a row once while it stays selected. Hold the key in
    // the binding for one render (the tree sees it), then clear it: the tree is reset to null and
    // reports the same row again. Setting null right here would be a no-op for the binding.
    this.selection.set(key);
    afterNextRender(() => this.selection.set(null), { injector: this.injector });
    if (!row) return;
    const now = Date.now();
    const double = isDoubleClick(this.last, key, now);
    this.last = double ? null : { key, at: now };
    if (double) this.pinned.emit(row.pane);
    else this.selected.emit(row.pane);
  }

  protected setQuery(event: Event): void {
    this.query.set((event.target as HTMLInputElement).value);
  }

  protected rowTitle(row: PaneRow): string {
    return [row.pane.pane_id, row.dir, row.branch].filter(Boolean).join(" · ");
  }

  protected glyph(row: PaneRow): string {
    return row.agent ? row.agent.slice(0, 2) : ">_";
  }
}
