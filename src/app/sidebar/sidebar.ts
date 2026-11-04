import { NgTemplateOutlet } from "@angular/common";
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
import { Attention, workspaceUnread } from "../core/attention";
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
import { UiState } from "../core/ui-state";
import { SpaceActions } from "../core/space-actions";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";

/** What a tree node carries back to the template. */
export type SpaceData =
  | {
      kind: "workspace";
      workspace: Workspace;
      status: AgentStatus;
      lines: Line[];
      /** Split layout only: the workspace whose terminals are listed. */
      selected?: boolean;
    }
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

/** The two lists of the `split` layout. */
export interface SplitSections {
  /** Workspaces; the selected one lists its terminal panes below it. */
  spaces: HxTreeNode<SpaceData>[];
  /** Every agent pane across the workspaces in herdr order, with its workspace name. */
  agents: HxTreeNode<SpaceData>[];
  /** The workspace whose panes are listed (the wanted one, else the first). */
  selectedId: string | null;
}

/**
 * Structure of the `split` layout (as in the herdr sidebar): a flat list of workspaces where the
 * `selectedId` one (else the first) has its terminal panes as children, and a flat list of all
 * agent panes, each rendered like in the tree plus a last line with the workspace name.
 */
export function splitSections(
  spaces: SpaceNode[],
  selectedId: string | null,
  rows: SidebarRows = DEFAULT_SIDEBAR_ROWS,
): SplitSections {
  const selected =
    spaces.find((s) => s.workspace.workspace_id === selectedId) ?? spaces[0] ?? undefined;
  const spaceList = spaces.map<HxTreeNode<SpaceData>>((space) => {
    const ctx = spaceContext(space);
    const isSelected = space === selected;
    return {
      key: WORKSPACE_PREFIX + space.workspace.workspace_id,
      label: space.workspace.label,
      // helix applies `expanded` once per key, so every workspace starts open; only the selected
      // one has children, the others are leaves until they are selected.
      expanded: true,
      leaf: !isSelected,
      data: {
        kind: "workspace",
        workspace: space.workspace,
        status: aggregateStatus(space),
        lines: renderRow("workspace", rows.workspace, space, ctx),
        selected: isSelected,
      },
      children: isSelected
        ? space.rows
            .filter((row) => !row.agent)
            .map((row) => ({
              key: row.pane.pane_id,
              label: row.name,
              leaf: true,
              data: {
                kind: "pane" as const,
                row,
                lines: renderRow("terminal", rows.terminal, row, ctx),
              },
            }))
        : undefined,
    };
  });
  const agents = spaces.flatMap((space) => {
    const ctx = spaceContext(space);
    return space.rows
      .filter((row) => row.agent)
      .map<HxTreeNode<SpaceData>>((row) => ({
        key: row.pane.pane_id,
        label: row.name,
        leaf: true,
        data: {
          kind: "pane",
          row,
          lines: [
            ...renderRow("agent", rows.agent, row, ctx),
            [
              {
                kind: "text",
                text: space.workspace.label,
                status: row.pane.agent_status,
                bold: false,
                dim: false,
                sep: false,
              },
            ],
          ],
        },
      }));
  });
  return { spaces: spaceList, agents, selectedId: selected?.workspace.workspace_id ?? null };
}

/** A row's context menu: `kind` says what was clicked, `action` what was picked. */
export interface SidebarMenuAction {
  kind: "workspace" | "pane";
  id: string;
  action:
    | "new-terminal"
    | "split-right"
    | "split-down"
    | "start-agent"
    | "rename"
    | "close"
    | "close-space";
}

/** One entry of a context menu. */
export interface SidebarMenuItem {
  action: SidebarMenuAction["action"];
  label: string;
}

/** A group of context menu entries; `label` is the heading, `null` for the built-in group. */
export interface SidebarMenuGroup {
  id: string;
  label: string | null;
  items: SidebarMenuItem[];
}

/**
 * Groups of the context menu of a workspace row or a pane row: the built-in actions.
 */
export function sidebarMenuItems(kind: SidebarMenuAction["kind"]): SidebarMenuGroup[] {
  const builtin: SidebarMenuGroup = {
    id: "builtin",
    label: null,
    items:
      kind === "workspace"
        ? [
            { action: "new-terminal", label: "New terminal..." },
            { action: "close-space", label: "Close space" },
          ]
        : [
            { action: "split-right", label: "Split right" },
            { action: "split-down", label: "Split down" },
            { action: "start-agent", label: "Start agent..." },
            { action: "rename", label: "Rename..." },
            { action: "close", label: "Close pane..." },
          ],
  };
  return [builtin];
}

/**
 * The index of the menu item a key moves to among `labels` (focusable items only, so separators
 * and headings never count), or `null` when the key does nothing. `current` is -1 when no item has
 * focus; arrows wrap; a single character jumps to the next label that starts with it.
 */
export function menuTarget(labels: readonly string[], current: number, key: string): number | null {
  const n = labels.length;
  if (n === 0) return null;
  switch (key) {
    case "ArrowDown":
      return current < 0 ? 0 : (current + 1) % n;
    case "ArrowUp":
      return current < 0 ? n - 1 : (current - 1 + n) % n;
    case "Home":
      return 0;
    case "End":
      return n - 1;
  }
  if (key.length !== 1 || key === " ") return null;
  const c = key.toLowerCase();
  for (let i = 1; i <= n; i++) {
    const idx = (Math.max(current, -1) + i) % n;
    if (labels[idx].toLowerCase().startsWith(c)) return idx;
  }
  return null;
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
  imports: [HxMessage, HxTree, HxTreeNodeTemplate, HxInput, NgTemplateOutlet, SplitPane],
  templateUrl: "./sidebar.html",
  styleUrl: "./sidebar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { "[class.split]": "layout() === 'split'" },
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
  protected menuItems(kind: SidebarMenuAction["kind"]) {
    return sidebarMenuItems(kind);
  }

  protected openMenu(event: MouseEvent, kind: SidebarMenuAction["kind"], id: string): void {
    event.preventDefault();
    event.stopPropagation();
    this.menuAt.set({ kind, id, x: event.clientX, y: event.clientY });
  }

  /** Arrow keys, Home/End and type-ahead move focus over the entries; separators and headings are no buttons. */
  protected onMenuKey(event: KeyboardEvent, menu: HTMLElement): void {
    if (event.key === "Escape") {
      this.closeMenu();
      return;
    }
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const items = [...menu.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    const current = items.findIndex((e) => e === document.activeElement);
    const to = menuTarget(
      items.map((e) => e.textContent?.trim() ?? ""),
      current,
      event.key,
    );
    if (to === null) return;
    event.preventDefault();
    items[to].focus();
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
  protected readonly spaceActions = inject(SpaceActions);
  private readonly attn = inject(Attention);

  protected rings(paneId: string): boolean {
    return this.attn.attention().has(paneId);
  }

  protected isUnread(paneId: string): boolean {
    return this.attn.unread().has(paneId);
  }

  /** Unread panes of a workspace (its row shows the sum). */
  protected unreadOf(workspaceId: string): number {
    const space = this.store.spaces().find((s) => s.workspace.workspace_id === workspaceId);
    return space ? workspaceUnread(space, this.attn.unread()) : 0;
  }
  protected readonly query = signal("");
  protected readonly selection = signal<string | null>(null);

  /** The chevron at the right edge of a workspace row: expands or collapses it (not a selection). */
  protected toggle(tree: HxTree, key: string, event: Event): void {
    event.stopPropagation();
    tree.toggle(key);
  }

  /** The close icon of a workspace row: the close flow of the context menu, no selection or toggle. */
  protected closeSpace(workspaceId: string, event: Event): void {
    event.stopPropagation();
    this.menuAction.emit({ kind: "workspace", id: workspaceId, action: "close-space" });
  }

  protected marks(row: PaneRow) {
    return paneMarks(row.pane.pane_id, this.openIds(), this.activeId());
  }

  private readonly settings = inject(SettingsStore);
  private readonly rows = computed(() => this.settings.settings().sidebarRows);

  protected readonly ui = inject(UiState);
  protected readonly layout = computed(() => this.settings.settings().sidebarLayout);
  protected readonly writeError = this.settings.writeError;
  /** The layout a click on the toggle switches to. */
  protected readonly otherLayout = computed(() => (this.layout() === "tree" ? "split" : "tree"));

  protected toggleLayout(): void {
    void this.settings.setSidebarLayout(this.otherLayout());
  }

  private readonly filtered = computed(() => {
    const rows = this.rows();
    const spaces = this.store.spaces();
    const bySpace = new Map(spaces.flatMap((s) => s.rows.map((r) => [r, s] as const)));
    return filterSpaces(spaces, this.query(), (row) => filterValues(bySpace.get(row)!, row, rows));
  });

  protected readonly nodes = computed(() => spaceNodes(this.filtered(), this.rows()));

  protected readonly split = computed(() =>
    splitSections(this.filtered(), this.ui.splitWorkspace(), this.rows()),
  );

  protected data(node: HxTreeNode): SpaceData {
    return node.data as SpaceData;
  }

  protected onSelect(key: string | readonly string[] | null): void {
    if (typeof key !== "string") return;
    if (key.startsWith(WORKSPACE_PREFIX)) {
      if (this.layout() === "split") this.ui.setSplitWorkspace(key.slice(WORKSPACE_PREFIX.length));
      this.hold(key);
      return;
    }
    const row = this.store
      .spaces()
      .flatMap((s) => s.rows)
      .find((r) => r.pane.pane_id === key);
    this.hold(key);
    if (!row) return;
    const now = Date.now();
    const double = isDoubleClick(this.last, key, now);
    this.last = double ? null : { key, at: now };
    if (double) this.pinned.emit(row.pane);
    else this.selected.emit(row.pane);
  }

  /**
   * hx-tree skips equal values, so it reports a row once while it stays selected. Hold the key in
   * the binding for one render (the tree sees it), then clear it: the tree is reset to null and
   * reports the same row again. Setting null right here would be a no-op for the binding.
   */
  private hold(key: string): void {
    this.selection.set(key);
    afterNextRender(() => this.selection.set(null), { injector: this.injector });
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
