import { paneRename } from "./rename-keys";
import { PaneActions } from "../core/pane-actions";
import {
  ChangeDetectionStrategy,
  Component,
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
import { HxButton, HxInput, HxMessage, HxTooltip } from "@gravionlabs/helix-ui";
import { cardKeyboard } from "./card-keys";
import { cardMenuGroups, runCardEntry, sortGroups, spaceDirOf } from "./card-menu";
import {
  filterSpaces,
  olderHost,
  type Terminal,
  type PaneRow,
  type SpaceNode,
  dropAllowed,
} from "../core/snapshot";
import { ContextMenu } from "../ui/context-menu/context-menu";
import type { MenuGroup } from "../ui/context-menu/menu-model";
import { Attention, canMarkUnread } from "../core/attention";
import { SpacesStore } from "../core/spaces-store";
import { filterValues } from "./spaces";
import { SettingsStore } from "../core/settings";
import { UiState } from "../core/ui-state";
import { SpaceActions } from "../core/space-actions";
import { Machines } from "../core/machines";
import { resumeUnavailable } from "../core/session-resume";
import { sidebarMenuItems, type SidebarMenuAction } from "./sidebar-menu";
import { SpaceCard } from "./space-card";
import { WORKSPACE_PREFIX, nestTabs, spaceNodes, type SpaceData } from "./space-nodes";

export { nestTabs, spaceNodes, type SpaceData };
import { MachinesSection } from "./machines-section/machines-section";

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
  imports: [ContextMenu, HxButton, HxTooltip, HxMessage, HxInput, MachinesSection, SpaceCard],
  templateUrl: "./sidebar.html",
  styleUrl: "./sidebar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
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
    return sidebarMenuItems(
      kind,
      repo,
      older,
      this.machines.blockedReason(id),
      markable,
      pinned,
      resumeUnavailable(id),
    );
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
    const groups: MenuGroup[] = this.menuItems(kind, id).map((g) => ({
      id: g.id,
      label: g.label,
      items: g.items.map((i) => ({ id: i.action, label: i.label, disabled: i.disabled })),
    }));
    if (kind !== "workspace") return groups;
    return [...groups, ...cardMenuGroups(id, this.cardMenu)];
  }

  private get cardMenu() {
    return {
      ui: this.ui,
      sort: this.sort,
      dir: (id: string) => spaceDirOf(this.store.allSpaces(), id),
      configure: (id: string) => this.configOpened.emit(id),
      chooseSort: (mode: string) => this.chooseSort(mode),
    };
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
    if (at && runCardEntry(picked, at.id, this.cardMenu, this.spaceActions)) return;
    if (at) this.menuAction.emit({ kind: at.kind, id: at.id, action });
  }

  private readonly injector = inject(Injector);
  protected readonly paneActions = inject(PaneActions);
  protected readonly store = inject(SpacesStore);
  protected readonly spaceActions = inject(SpaceActions);
  private readonly attn = inject(Attention);

  protected readonly query = signal("");

  /** The space being dragged; `null` when no drag runs. */
  protected readonly dragging = signal<string | null>(null);
  /** Where the dragged space would land: before or after `id`. */
  protected readonly dropMark = signal<{ id: string; after: boolean } | null>(null);

  /** Dragging reorders the complete list, so it is off while a filter hides spaces or outside `manual`. */
  protected canDrag(): boolean {
    return this.sort() === "manual" && this.query().trim() === "";
  }

  protected dragStart(event: DragEvent, workspaceId: string): void {
    if (!this.canDrag()) {
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
  /** Whether the space is being closed: its menu stays shut meanwhile. */
  protected isClosing(workspaceId: string): boolean {
    return this.spaceActions.closingIds().has(workspaceId);
  }

  protected readonly paneEdit = paneRename({
    actions: this.paneActions,
    injector: this.injector,
    pane: (id) => this.store.pane(id),
  });

  private readonly settings = inject(SettingsStore);
  private readonly rows = computed(() => this.settings.settings().sidebarRows);
  private readonly showBase = computed(() => this.settings.settings().sidebarShowBaseBranch);

  protected readonly ui = inject(UiState);
  protected readonly writeError = this.settings.writeError;
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

  protected readonly sortGroups = computed<MenuGroup[]>(() => sortGroups(this.sort()));

  protected chooseSort(picked: string): void {
    const choice = picked as "manual" | "name" | "attention" | "recent" | "now";
    this.sortMenuAt.set(null);
    if (choice === "now") this.spaceActions.sortNow();
    else void this.settings.setSidebarSort(choice);
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

  /** The cards of the spaces of each machine (`cards` layout, #938); not filtered, not reordered. */
  protected readonly remoteNodes = computed(
    () =>
      new Map(
        this.machines
          .machines()
          .map((m) => [
            m.name,
            spaceNodes(this.store.remoteSpaces(m.name), this.rows(), this.showBase()),
          ]),
      ),
  );
  protected readonly nodes = computed(() =>
    spaceNodes(this.filtered(), this.rows(), this.showBase()),
  );

  /** Keyboard navigation of the cards layout (#937). */
  private readonly cardKeys = cardKeyboard({
    nodes: this.nodes,
    collapsed: this.ui.collapsedCards,
    filtering: computed(() => this.query().trim() !== ""),
    toggle: (id, collapsed) => this.ui.setCardCollapsed(id, collapsed),
    open: (key) => this.onSelect(key),
    root: () => this.host.nativeElement,
    injector: this.injector,
  });
  protected readonly cardStop = this.cardKeys.stop;
  protected readonly onCardKey = this.cardKeys.onKey;
  protected readonly onCardFocus = this.cardKeys.onFocus;

  protected onSelect(key: string | readonly string[] | null): void {
    if (typeof key !== "string") return;
    if (key.startsWith(WORKSPACE_PREFIX)) {
      this.spaceOpened.emit(key.slice(WORKSPACE_PREFIX.length));
      return;
    }
    const row = this.store
      .spaces()
      .flatMap((s) => s.rows)
      .find((r) => r.pane.terminal_id === key);
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
}
