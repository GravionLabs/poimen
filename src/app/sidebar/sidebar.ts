import { NgTemplateOutlet } from "@angular/common";
import {
  ChangeDetectionStrategy,
  Component,
  Directive,
  ElementRef,
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
  type Terminal,
  type PaneRow,
  type SpaceNode,
  type Space,
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
import { TauriBridge } from "../core/tauri";
import { isGithubUrl } from "../github/pulls";
import { UiState } from "../core/ui-state";
import { SpaceActions } from "../core/space-actions";
import { SplitPane } from "../ui/helix-candidates/split-pane/split-pane";

/** What a tree node carries back to the template. */
export type SpaceData =
  | {
      kind: "workspace";
      workspace: Space;
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
      key: WORKSPACE_PREFIX + space.workspace.space_id,
      label: space.workspace.label,
      expanded: true,
      data: {
        kind: "workspace",
        workspace: space.workspace,
        status,
        lines: renderRow("workspace", rows.workspace, space, ctx),
      },
      children: space.rows.map((row) => ({
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
    spaces.find((s) => s.workspace.space_id === selectedId) ?? spaces[0] ?? undefined;
  const spaceList = spaces.map<HxTreeNode<SpaceData>>((space) => {
    const ctx = spaceContext(space);
    const isSelected = space === selected;
    return {
      key: WORKSPACE_PREFIX + space.workspace.space_id,
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
              key: row.pane.terminal_id,
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
        key: row.pane.terminal_id,
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
  return { spaces: spaceList, agents, selectedId: selected?.workspace.space_id ?? null };
}

/** The directory of the first pane of space `id` when it is a repository, else `undefined`. */
export function spaceRepoDir(spaces: readonly SpaceNode[], id: string): string | undefined {
  const row = spaces
    .find((n) => n.workspace.space_id === id)
    ?.rows.find((r) => r.dir !== undefined && r.branch !== undefined);
  return row?.dir;
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
    | "close-space"
    | "rename-space"
    | "start-task"
    | "cleanup-tasks"
    | "show-changes";
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
export function sidebarMenuItems(
  kind: SidebarMenuAction["kind"],
  /** The space is in a repository: it offers to start a task. */
  repo = false,
): SidebarMenuGroup[] {
  const builtin: SidebarMenuGroup = {
    id: "builtin",
    label: null,
    items:
      kind === "workspace"
        ? [
            { action: "new-terminal", label: "New terminal..." },
            ...(repo
              ? [
                  { action: "show-changes" as const, label: "Show changes" },
                  { action: "start-task" as const, label: "Start task from issue…" },
                  { action: "cleanup-tasks" as const, label: "Clean up merged tasks" },
                ]
              : []),
            { action: "rename-space", label: "Rename space..." },
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
/** Focuses and selects the inline rename input once it is in the DOM. */
@Directive({ selector: "input[appSpaceEditFocus]" })
export class SpaceEditFocus {
  constructor() {
    const el = inject<ElementRef<HTMLInputElement>>(ElementRef).nativeElement;
    afterNextRender(() => {
      el.focus();
      el.select();
    });
  }
}

@Component({
  selector: "app-sidebar",
  imports: [
    SpaceEditFocus,
    HxMessage,
    HxTree,
    HxTreeNodeTemplate,
    HxInput,
    NgTemplateOutlet,
    SplitPane,
  ],
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
  readonly selected = output<Terminal>();
  /** A second click on the same row within {@link DOUBLE_CLICK_MS}: pin the tab. */
  readonly pinned = output<Terminal>();
  /** A space row was picked (click, Enter): the app opens its overview tab. */
  readonly spaceOpened = output<string>();
  /** The gear icon of a space row: its overview at the configuration section. */
  readonly configOpened = output<string>();
  /** An entry of a row's context menu was picked. */
  readonly menuAction = output<SidebarMenuAction>();
  private last: { key: string; at: number } | null = null;
  protected readonly menuAt = signal<{
    kind: SidebarMenuAction["kind"];
    id: string;
    x: number;
    y: number;
  } | null>(null);
  protected menuItems(kind: SidebarMenuAction["kind"], id: string) {
    const repo = kind === "workspace" && spaceRepoDir(this.store.spaces(), id) !== undefined;
    return sidebarMenuItems(kind, repo);
  }

  protected openMenu(event: MouseEvent, kind: SidebarMenuAction["kind"], id: string): void {
    event.preventDefault();
    event.stopPropagation();
    if (kind === "workspace" && this.isClosing(id)) return;
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
  private readonly bridge = inject(TauriBridge);

  protected rings(paneId: string): boolean {
    return this.attn.attention().has(paneId);
  }

  protected isUnread(paneId: string): boolean {
    return this.attn.unread().has(paneId);
  }

  /** Unread panes of a workspace (its row shows the sum). */
  protected unreadOf(workspaceId: string): number {
    const space = this.store.spaces().find((s) => s.workspace.space_id === workspaceId);
    return space ? workspaceUnread(space, this.attn.unread()) : 0;
  }
  protected readonly query = signal("");

  /** The space being dragged; `null` when no drag runs. */
  protected readonly dragging = signal<string | null>(null);
  /** Where the dragged space would land: before or after `id`. */
  protected readonly dropMark = signal<{ id: string; after: boolean } | null>(null);

  /** Dragging reorders the complete list, so it is off while a filter hides spaces. */
  protected canDrag(workspaceId: string): boolean {
    return this.query().trim() === "" && this.editing() !== workspaceId;
  }

  protected dragStart(event: DragEvent, workspaceId: string): void {
    if (!this.canDrag(workspaceId)) {
      event.preventDefault();
      return;
    }
    this.dragging.set(workspaceId);
    event.dataTransfer?.setData("text/plain", workspaceId);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  protected dragOver(event: DragEvent, workspaceId: string): void {
    const source = this.dragging();
    if (source === null || source === workspaceId) return;
    event.preventDefault();
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.dropMark.set({ id: workspaceId, after: event.clientY > box.top + box.height / 2 });
  }

  protected drop(event: DragEvent, workspaceId: string): void {
    event.preventDefault();
    const source = this.dragging();
    const mark = this.dropMark();
    this.dragEnd();
    if (source === null || mark?.id !== workspaceId) return;
    this.spaceActions.reorder(source, workspaceId, mark.after);
  }

  /** Ends a drag; a drop outside the list lands here without a `drop`, so nothing moves. */
  protected dragEnd(): void {
    this.dragging.set(null);
    this.dropMark.set(null);
  }

  /** The tab row being dragged (its pane, tab and space); `null` when no drag runs. */
  protected readonly tabDrag = signal<{ pane: string; tab: string; space: string } | null>(null);
  /** Where the dragged tab would land: before or after the row of `pane`. */
  protected readonly tabMark = signal<{ pane: string; after: boolean } | null>(null);

  /** Like spaces, tabs are only reordered in the complete list, so not while a filter is active. */
  protected canDragTab(): boolean {
    return this.query().trim() === "";
  }

  protected tabDragStart(event: DragEvent, pane: Terminal): void {
    if (!this.canDragTab()) {
      event.preventDefault();
      return;
    }
    this.tabDrag.set({ pane: pane.terminal_id, tab: pane.tab_id, space: pane.space_id });
    event.dataTransfer?.setData("text/plain", pane.terminal_id);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  /** A row of another space, or of the dragged tab itself, is no target: no marker, no drop. */
  protected tabDragOver(event: DragEvent, pane: Terminal): void {
    const source = this.tabDrag();
    if (source === null || source.space !== pane.space_id || source.tab === pane.tab_id) return;
    event.preventDefault();
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.tabMark.set({ pane: pane.terminal_id, after: event.clientY > box.top + box.height / 2 });
  }

  protected tabDrop(event: DragEvent, pane: Terminal): void {
    event.preventDefault();
    const source = this.tabDrag();
    const mark = this.tabMark();
    this.tabDragEnd();
    if (source === null || mark?.pane !== pane.terminal_id || source.space !== pane.space_id)
      return;
    this.spaceActions.reorderTabs(source.space, source.tab, pane.tab_id, mark.after);
  }

  protected tabDragEnd(): void {
    this.tabDrag.set(null);
    this.tabMark.set(null);
  }
  protected readonly selection = signal<string | null>(null);

  /** The chevron at the right edge of a workspace row: expands or collapses it (not a selection). */
  protected toggle(tree: HxTree, key: string, event: Event): void {
    event.stopPropagation();
    tree.toggle(key);
  }

  /** The close icon of a workspace row: the close flow of the context menu, no selection or toggle. */
  protected isClosing(workspaceId: string): boolean {
    return this.spaceActions.closingIds().has(workspaceId);
  }

  /** The space whose label is being edited in place; `null` when none. */
  protected readonly editing = signal<string | null>(null);

  protected startEdit(workspaceId: string, event: Event): void {
    event.stopPropagation();
    if (this.isClosing(workspaceId)) return;
    this.spaceActions.clearRenameError();
    this.editing.set(workspaceId);
  }

  /** Enter or blur confirms, Escape cancels; a failed rename keeps editing, the old label stays. */
  protected async finishEdit(
    workspaceId: string,
    input: HTMLInputElement,
    commit: boolean,
  ): Promise<void> {
    if (this.editing() !== workspaceId) return;
    if (!commit) {
      this.editing.set(null);
      this.spaceActions.clearRenameError();
      return;
    }
    if (await this.spaceActions.rename(workspaceId, input.value)) this.editing.set(null);
    else input.focus();
  }

  protected configureSpace(workspaceId: string, event: Event): void {
    event.stopPropagation();
    this.configOpened.emit(workspaceId);
  }

  protected closeSpace(workspaceId: string, event: Event): void {
    event.stopPropagation();
    this.menuAction.emit({ kind: "workspace", id: workspaceId, action: "close-space" });
  }

  /** Opens a pull request in the system browser; only github.com links, and the row stays as is. */
  protected openPull(url: string, event: Event): void {
    event.stopPropagation();
    if (isGithubUrl(url)) void this.bridge.openUrl(url);
  }

  protected marks(row: PaneRow) {
    return paneMarks(row.pane.terminal_id, this.openIds(), this.activeId());
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
      this.spaceOpened.emit(key.slice(WORKSPACE_PREFIX.length));
      return;
    }
    const row = this.store
      .spaces()
      .flatMap((s) => s.rows)
      .find((r) => r.pane.terminal_id === key);
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
    return [row.pane.terminal_id, row.dir, row.branch].filter(Boolean).join(" · ");
  }

  protected glyph(row: PaneRow): string {
    return row.agent ? row.agent.slice(0, 2) : ">_";
  }
}
