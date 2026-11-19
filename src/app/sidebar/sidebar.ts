import { NgTemplateOutlet } from "@angular/common";
import {
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  Injector,
  afterNextRender,
  computed,
  effect,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { HxButton, HxInput, HxMessage, HxTree, HxTreeNodeTemplate } from "@gravionlabs/helix-ui";
import type { HxTreeNode } from "@gravionlabs/helix-ui";
import {
  filterSpaces,
  olderHost,
  olderHostNote,
  type AgentStatus,
  type Terminal,
  type PaneRow,
  type SpaceNode,
  type Space,
  dropAllowed,
} from "../core/snapshot";
import { ContextMenu } from "../ui/context-menu/context-menu";
import type { MenuGroup } from "../ui/context-menu/menu-model";
import { Attention, canMarkUnread, workspaceUnread } from "../core/attention";
import { SpacesStore } from "../core/spaces-store";
import {
  TokenTotalsService,
  compactTotal,
  sumTotals,
  totalsTip,
  type TokenTotals,
} from "../core/token-totals";
import {
  DEFAULT_SIDEBAR_ROWS,
  RECORDS_SHOWN,
  aggregateStatus,
  paneMarks,
  recordRows,
  recordsSummary,
  renderRow,
  spaceContext,
  type Line,
  type RecordRow,
  type RowType,
  type SidebarRows,
} from "./spaces";
import { SettingsStore, type SidebarAgents, type SidebarLayout } from "../core/settings";
import {
  AGENT_SECTIONS,
  formatElapsed,
  silentLabel,
  orderAgents,
  type AgentSection,
} from "../core/agent-priority";
import { TauriBridge } from "../core/tauri";
import { isGithubUrl } from "../github/pulls";
import { UiState } from "../core/ui-state";
import { SpaceActions } from "../core/space-actions";
import { SpaceRebuild, type RebuildMark } from "../core/space-rebuild";
import { Machines } from "../core/machines";
import { sidebarMenuItems, type SidebarMenuAction } from "./sidebar-menu";
import { SpaceCard } from "./space-card";
import { SpaceEditFocus } from "./space-edit-focus";
import { MachinesSection } from "./machines-section/machines-section";
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

/** The layout the toggle in the header switches to next. */
const LAYOUT_CYCLE: Record<SidebarLayout, SidebarLayout> = {
  tree: "split",
  split: "cards",
  cards: "tree",
};

const rowType = (row: PaneRow): RowType => (row.agent ? "agent" : "terminal");

/** Texts of the shown tokens of a space and one of its panes: what the filter matches. */
export function filterValues(
  space: SpaceNode,
  row: PaneRow,
  rows: SidebarRows,
  showBase = false,
): string[] {
  const ctx = spaceContext(space, showBase);
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
  return top;
}

/** The two lists of the `split` layout. */
export interface SplitSections {
  /** Workspaces; the selected one lists its terminal panes below it. */
  spaces: HxTreeNode<SpaceData>[];
  /** Every agent pane across the workspaces in herdr order, with its workspace name. */
  agents: HxTreeNode<SpaceData>[];
  /** The workspace whose panes are listed (the wanted one, else the first). */
  selectedId: string | null;
  /** `priority` mode only: the agents in their sections, in order (`agents` is the flat list). */
  agentSections: AgentSectionView[];
}

/** One section of the Agents list in `priority` mode. */
export interface AgentSectionView {
  id: AgentSection;
  title: string;
  count: number;
  folded: boolean;
  /** The agents of the section; empty while it is folded. */
  nodes: HxTreeNode<SpaceData>[];
}

/** What the Agents section needs besides the spaces: order, fold state and the status clock. */
export interface AgentView {
  /** `priority` sorts and sections the list; `list` keeps the space order. */
  mode: SidebarAgents;
  /** Section of each agent (the settled one); an agent missing here is idle. */
  sections: ReadonlyMap<string, AgentSection>;
  folded: ReadonlySet<AgentSection>;
  /** When each pane's status last changed (epoch ms). */
  changedAt: ReadonlyMap<string, number>;
  /** The change times the order inside a section uses (settled like `sections`). */
  orderAt: ReadonlyMap<string, number>;
  now: number;
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
  showBase = false,
  view?: AgentView,
): SplitSections {
  const selected =
    spaces.find((s) => s.workspace.space_id === selectedId) ?? spaces[0] ?? undefined;
  const spaceList = spaces.map<HxTreeNode<SpaceData>>((space) => {
    const ctx = spaceContext(space, showBase);
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
        member: !!space.parent,
      },
      children: isSelected
        ? nestTabs(
            space.rows.filter((row) => !row.agent),
            (row) => ({
              key: row.pane.terminal_id,
              label: row.name,
              leaf: true,
              data: {
                kind: "pane" as const,
                row,
                lines: renderRow("terminal", rows.terminal, row, ctx),
              },
            }),
          )
        : undefined,
    };
  });
  const entries = spaces.flatMap((space) => {
    const ctx = spaceContext(space, showBase);
    return space.rows
      .filter((row) => row.agent)
      .map((row) => ({
        row,
        node: {
          key: row.pane.terminal_id,
          label: row.name,
          leaf: true,
          data: {
            kind: "pane",
            row,
            since: view?.changedAt.has(row.pane.terminal_id)
              ? formatElapsed(view.now - view.changedAt.get(row.pane.terminal_id)!)
              : undefined,
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
        } as HxTreeNode<SpaceData>,
      }));
  });
  const priority = view?.mode === "priority";
  const ordered = priority
    ? orderAgents(
        entries,
        (e) => e.row.pane.terminal_id,
        (e) => e.row.name,
        view.sections,
        view.orderAt,
        view.now,
      )
    : entries;
  const agentSections = priority
    ? AGENT_SECTIONS.map<AgentSectionView>(({ id, title }) => {
        const members = ordered.filter(
          (e) => (view.sections.get(e.row.pane.terminal_id) ?? "idle") === id,
        );
        const folded = view.folded.has(id);
        return {
          id,
          title,
          count: members.length,
          folded,
          nodes: folded ? [] : members.map((e) => e.node),
        };
      })
    : [];
  return {
    spaces: spaceList,
    agents: ordered.map((e) => e.node),
    selectedId: selected?.workspace.space_id ?? null,
    agentSections,
  };
}

/**
 * The directory of the first pane of space `id` when it is a repository, else the space's own
 * directory when that is one (a space without terminals); `undefined` otherwise.
 */
export function spaceRepoDir(spaces: readonly SpaceNode[], id: string): string | undefined {
  const node = spaces.find((n) => n.workspace.space_id === id);
  const row = node?.rows.find((r) => r.dir !== undefined && r.branch !== undefined);
  if (row) return row.dir;
  return node?.repo?.branch != null ? node.workspace.cwd : undefined;
}

export {
  sidebarMenuItems,
  type SidebarMenuAction,
  type SidebarMenuGroup,
  type SidebarMenuItem,
} from "./sidebar-menu";

export { menuTarget } from "../ui/context-menu/menu-model";

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
  imports: [
    ContextMenu,
    SpaceEditFocus,
    HxButton,
    HxMessage,
    HxTree,
    HxTreeNodeTemplate,
    HxInput,
    NgTemplateOutlet,
    SplitPane,
    MachinesSection,
    SpaceCard,
  ],
  templateUrl: "./sidebar.html",
  styleUrl: "./sidebar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    "[class.split]": "layout() === 'split'",
    // The order stays while the pointer is pressed, so a row does not move under it (#712).
    "(pointerdown)": "holdSort(true)",
    "(window:pointerup)": "holdSort(false)",
    "(window:pointercancel)": "holdSort(false)",
  },
})
export class Sidebar {
  protected readonly machines = inject(Machines);
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
    const row =
      kind === "pane"
        ? this.store
            .spaces()
            .flatMap((n) => n.rows)
            .find((r) => r.pane.terminal_id === id)?.pane
        : undefined;
    const older = row !== undefined && olderHost(row) !== null;
    const markable = canMarkUnread(row, this.attn.unread());
    const local =
      kind === "workspace" && this.store.snapshot().spaces.some((s) => s.space_id === id);
    const pinned = local ? this.isPinned(id) : null;
    return sidebarMenuItems(kind, repo, older, this.machines.blockedReason(id), markable, pinned);
  }

  /** Whether the space row shows the pin mark (#710). */
  protected isPinned(spaceId: string): boolean {
    return this.store.pinnedSpaces?.().includes(spaceId) ?? false;
  }

  protected openMenu(event: MouseEvent, kind: SidebarMenuAction["kind"], id: string): void {
    event.preventDefault();
    event.stopPropagation();
    if (kind === "workspace" && this.isClosing(id)) return;
    this.menuAt.set({ kind, id, x: event.clientX, y: event.clientY });
  }

  /** The entries of the open menu as the shared menu shows them. */
  protected menuGroups(kind: SidebarMenuAction["kind"], id: string): MenuGroup[] {
    return this.menuItems(kind, id).map((g) => ({
      id: g.id,
      label: g.label,
      items: g.items.map((i) => ({ id: i.action, label: i.label, disabled: i.disabled })),
    }));
  }

  protected closeMenu(): void {
    this.menuAt.set(null);
  }

  protected choose(picked: string): void {
    const action = picked as SidebarMenuAction["action"];
    const at = this.menuAt();
    // A disabled entry (offline machine) does nothing, and never reaches another backend.
    if (at && this.machines.blockedReason(at.id)) return;
    this.closeMenu();
    if (at) this.menuAction.emit({ kind: at.kind, id: at.id, action });
  }

  private readonly injector = inject(Injector);
  protected readonly store = inject(SpacesStore);
  protected readonly spaceActions = inject(SpaceActions);
  private readonly attn = inject(Attention);
  private readonly bridge = inject(TauriBridge);
  private readonly spaceRebuild = inject(SpaceRebuild);
  private readonly tokenTotals = inject(TokenTotalsService);

  /** Total of one pane; none for a machine that is not connected or a pane without transcript. */
  private totalOf(paneId: string): TokenTotals | undefined {
    return this.machines.blockedReason(paneId) ? undefined : this.tokenTotals.totals().get(paneId);
  }

  /** Compact sum of the panes of a space (`1.2M`); empty when none has a total. */
  protected spaceTokens(workspaceId: string): string {
    const space = this.store.spaces().find((s) => s.workspace.space_id === workspaceId);
    const sum = space && sumTotals(space.rows.map((r) => this.totalOf(r.pane.terminal_id)));
    return sum ? compactTotal(sum) : "";
  }

  protected spaceTokensTip(workspaceId: string): string {
    const space = this.store.spaces().find((s) => s.workspace.space_id === workspaceId);
    const sum = space && sumTotals(space.rows.map((r) => this.totalOf(r.pane.terminal_id)));
    return sum ? totalsTip(sum) : "";
  }

  /** The rebuild mark of a space that has no terminals; `undefined` for any other. */
  protected rebuildMark(workspaceId: string): RebuildMark | undefined {
    const mark = this.spaceRebuild.marks().get(workspaceId);
    if (!mark) return undefined;
    const space = this.store.spaces().find((s) => s.workspace.space_id === workspaceId);
    return space?.rows.length === 0 ? mark : undefined;
  }

  /** The "Rebuild" of an empty space (agents and setup start, the project file is gated). */
  protected rebuildSpace(workspaceId: string, event: Event): void {
    event.stopPropagation();
    void this.spaceRebuild.rebuild(workspaceId);
  }

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

  /** Dragging reorders the complete list, so it is off while a filter hides spaces or outside `manual`. */
  protected canDrag(workspaceId: string): boolean {
    return this.sort() === "manual" && this.query().trim() === "" && this.editing() !== workspaceId;
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
    if (!dropAllowed(this.store.spaces(), source, workspaceId)) return;
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
  protected readonly tabDrag = signal<{
    pane: string;
    tab: string;
    space: string;
    parent?: string;
  } | null>(null);
  /** Where the dragged tab would land: before or after the row of `pane`. */
  protected readonly tabMark = signal<{ pane: string; after: boolean } | null>(null);

  /** Like spaces, tabs are only reordered in the complete list, so not while a filter is active. */
  protected canDragTab(): boolean {
    return this.query().trim() === "";
  }

  protected tabDragStart(event: DragEvent, row: PaneRow): void {
    const pane = row.pane;
    if (!this.canDragTab()) {
      event.preventDefault();
      return;
    }
    this.tabDrag.set({
      pane: pane.terminal_id,
      tab: pane.tab_id,
      space: pane.space_id,
      parent: row.tabParent,
    });
    event.dataTransfer?.setData("text/plain", pane.terminal_id);
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  /**
   * A row of another space, of another parent tab (tabs reorder among siblings only, #656) or of
   * the dragged tab itself is no target: no marker, no drop.
   */
  protected tabDragOver(event: DragEvent, row: PaneRow): void {
    const pane = row.pane;
    const source = this.tabDrag();
    if (
      source === null ||
      source.space !== pane.space_id ||
      source.tab === pane.tab_id ||
      source.parent !== row.tabParent
    ) {
      this.tabMark.set(null);
      return;
    }
    event.preventDefault();
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.tabMark.set({ pane: pane.terminal_id, after: event.clientY > box.top + box.height / 2 });
  }

  protected tabDrop(event: DragEvent, row: PaneRow): void {
    const pane = row.pane;
    event.preventDefault();
    const source = this.tabDrag();
    const mark = this.tabMark();
    this.tabDragEnd();
    if (
      source === null ||
      mark?.pane !== pane.terminal_id ||
      source.space !== pane.space_id ||
      source.parent !== row.tabParent
    )
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

  /** Terminals whose folded child records the user opened. */
  protected readonly openRecords = signal<ReadonlySet<string>>(new Set());

  /**
   * The child records of a pane (#684): all rows, or one summary line when there are more than a
   * few and the user has not opened them. `undefined` without records.
   */
  protected recordView(
    row: PaneRow,
  ): { rows: RecordRow[]; summary: string | undefined; many: boolean; open: boolean } | undefined {
    const records = row.pane.records;
    if (!records?.length) return undefined;
    const many = records.length > RECORDS_SHOWN;
    const open = !many || this.openRecords().has(row.pane.terminal_id);
    return {
      rows: open ? recordRows(row.pane) : [],
      summary: many && !open ? recordsSummary(records) : undefined,
      many,
      open,
    };
  }

  protected toggleRecords(terminalId: string, event: Event): void {
    event.stopPropagation();
    this.openRecords.update((set) => {
      const next = new Set(set);
      if (!next.delete(terminalId)) next.add(terminalId);
      return next;
    });
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
  private readonly showBase = computed(() => this.settings.settings().sidebarShowBaseBranch);

  protected readonly ui = inject(UiState);
  protected readonly layout = computed(() => this.settings.settings().sidebarLayout);
  protected readonly writeError = this.settings.writeError;
  /** The layout a click on the toggle switches to. */
  protected readonly otherLayout = computed(() => LAYOUT_CYCLE[this.layout()]);

  protected readonly sort = computed(() => this.settings.settings().sidebarSort);

  /** Keeps the order of the spaces while the pointer is pressed. */
  holdSort(held: boolean): void {
    this.store.holdSort(held);
  }

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** A re-sort keeps the selected row in view (#712). */
  protected readonly keepSelectedInView = effect(() => {
    this.store.spaces();
    afterNextRender(
      () =>
        this.host.nativeElement
          .querySelector<HTMLElement>(".row.active, [aria-selected='true']")
          ?.scrollIntoView?.({ block: "nearest" }),
      { injector: this.injector },
    );
  });
  /** Where the sort menu of the header is open (below its button); `null` when closed. */
  protected readonly sortMenuAt = signal<{ x: number; y: number } | null>(null);

  protected openSortMenu(event: MouseEvent): void {
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.sortMenuAt.set({ x: box.left, y: box.bottom + 2 });
  }

  protected readonly sortGroups = computed<MenuGroup[]>(() => {
    const mode = this.sort();
    const radio = (id: "manual" | "name" | "attention" | "recent", label: string) => ({
      id,
      label,
      checked: mode === id,
    });
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
  });

  protected chooseSort(picked: string): void {
    const choice = picked as "manual" | "name" | "attention" | "recent" | "now";
    this.sortMenuAt.set(null);
    if (choice === "now") this.spaceActions.sortNow();
    else void this.settings.setSidebarSort(choice);
  }

  protected toggleLayout(): void {
    void this.settings.setSidebarLayout(this.otherLayout());
  }

  private readonly filtered = computed(() => {
    const rows = this.rows();
    const showBase = this.showBase();
    const spaces = this.store.spaces();
    const bySpace = new Map(spaces.flatMap((s) => s.rows.map((r) => [r, s] as const)));
    return filterSpaces(spaces, this.query(), (row) =>
      filterValues(bySpace.get(row)!, row, rows, showBase),
    );
  });

  protected readonly nodes = computed(() =>
    spaceNodes(this.filtered(), this.rows(), this.showBase()),
  );

  /** The clock of the elapsed times in the Agents section; ticks once a minute. */
  private readonly now = signal(Date.now());
  protected readonly clock = (() => {
    const timer = setInterval(() => this.now.set(Date.now()), 60_000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
    return timer;
  })();

  protected silentMark(row: PaneRow): string | undefined {
    return silentLabel(row.pane.agent_silent_since, this.now());
  }

  protected readonly agentsMode = computed(() => this.settings.settings().sidebarAgents);
  protected readonly otherAgentsMode = computed(() =>
    this.agentsMode() === "list" ? "priority" : "list",
  );

  protected readonly split = computed(() =>
    splitSections(this.filtered(), this.ui.splitWorkspace(), this.rows(), this.showBase(), {
      mode: this.agentsMode(),
      sections: this.store.agentSections(),
      folded: this.ui.agentSectionsFolded(),
      changedAt: this.attn.changedAt(),
      orderAt: this.store.agentChangedAt(),
      now: this.now(),
    }),
  );

  protected toggleAgentsMode(): void {
    void this.settings.setSidebarAgents(this.otherAgentsMode());
  }

  protected toggleAgentSection(id: AgentSection): void {
    this.ui.toggleAgentSection(id);
  }

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
    const totals = this.totalOf(row.pane.terminal_id);
    return [row.pane.terminal_id, row.dir, row.branch, totals && totalsTip(totals)]
      .filter(Boolean)
      .join(" · ");
  }

  /** The tooltip of the quiet mark of a terminal on an older PTY host; `null` when it is current. */
  protected hostNote(row: PaneRow): string | null {
    const host = olderHost(row.pane);
    return host ? olderHostNote(host) : null;
  }

  protected glyph(row: PaneRow): string {
    return row.agent ? row.agent.slice(0, 2) : ">_";
  }
}
