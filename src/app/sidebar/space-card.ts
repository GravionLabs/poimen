import { isComposing, refocus } from "./rename-keys";
import { NgTemplateOutlet } from "@angular/common";
import {
  ChangeDetectionStrategy,
  Component,
  Injector,
  EventEmitter,
  Input,
  Output,
  DestroyRef,
  computed,
  inject,
  signal,
} from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import type { HxTreeNode } from "@gravionlabs/helix-ui";
import { silentLabel } from "../core/agent-priority";
import { Attention, workspaceUnread } from "../core/attention";
import { Machines } from "../core/machines";
import { PaneActions } from "../core/pane-actions";
import { SpaceRebuild, type RebuildMark } from "../core/space-rebuild";
import { SpacesStore } from "../core/spaces-store";
import { olderHost, olderHostNote, splitId, type PaneRow } from "../core/snapshot";
import { SpaceActions } from "../core/space-actions";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { isGithubUrl } from "../github/pulls";
import type { SpaceData } from "./sidebar";
import { SpaceEditFocus } from "./space-edit-focus";
import type { SidebarMenuAction } from "./sidebar-menu";
import {
  RECORDS_SHOWN,
  paneMarks,
  recordRows,
  recordsSummary,
  stateLabel,
  type Segment,
} from "./spaces";

/** A card or a tab row in a drag: the dragged one, or where the drop would land. */
export type CardDragState = "source" | "before" | "after" | null;

/** Where a card asks for a context menu: the kind, the id and the point it opens at. */
export interface CardMenuRequest {
  kind: SidebarMenuAction["kind"];
  id: string;
  x: number;
  y: number;
}

/** Number of origin colors (`--origin-<n>` in `styles.css`). */
export const ORIGIN_COLORS = 6;

/** The color slot of a machine: a hash of its name, so it stays the same across sessions. */
export function originColor(name: string): number {
  let hash = 0;
  for (const ch of name) hash = (hash * 31 + ch.charCodeAt(0)) >>> 0;
  return hash % ORIGIN_COLORS;
}

/** The details line of a card: branch and directory on the left, the markers as chips. */
export interface CardDetails {
  branch: Segment | undefined;
  dir: Segment | undefined;
  chips: Segment[];
}

/**
 * Sorts line 2 of a card into its parts. A directory that only repeats the name is left out, and it
 * is not a chip either (#1085).
 */
export function splitDetails(line: Segment[], label: string): CardDetails | null {
  if (!line.length) return null;
  const name = label.trim().toLowerCase();
  const branch = line.find((s) => s.token === "branch");
  const dirSegment = line.find((s) => s.token === "dir");
  const leaf = dirSegment?.tip?.split(/[\\/]/).filter(Boolean).at(-1)?.toLowerCase();
  const dir = leaf === name ? undefined : dirSegment;
  const chips = line.filter((s) => s !== branch && s !== dirSegment && s.kind !== "dot");
  return branch || dir || chips.length ? { branch, dir, chips } : null;
}

/** The last `n` characters stay visible when a long text is cut in the middle. */
export const TAIL_CHARS = 9;

/** Splits a text for a cut in the middle: the head may be ellipsized, the tail always shows. */
export function middleParts(text: string): { head: string; tail: string } {
  return text.length <= TAIL_CHARS * 2
    ? { head: text, tail: "" }
    : { head: text.slice(0, -TAIL_CHARS), tail: text.slice(-TAIL_CHARS) };
}

/** Where a space runs, as the card shows it. */
export interface SpaceOrigin {
  /** A directory on this computer, or a machine; later kinds get their own icon. */
  kind: "directory" | "machine";
  label: string;
  tip: string;
  color: number | null;
}

/**
 * One card per space for the `cards` sidebar layout (#934). It renders the node `spaceNodes` builds
 * (no second model): the header (status dot, first line of `sidebar.rows.space`, unread count and
 * the actions), the further lines of the space row, and a body with the tabs and terminals.
 */
@Component({
  selector: "app-space-card",
  imports: [HxButton, NgTemplateOutlet, SpaceEditFocus],
  templateUrl: "./space-card.html",
  styleUrl: "./space-card.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SpaceCard {
  // Decorator inputs and outputs: the specs run without Angular's compiler transform.
  private readonly nodeSignal = signal<HxTreeNode<SpaceData> | null>(null);
  private readonly openIdsSignal = signal<ReadonlySet<string>>(new Set());
  private readonly activeIdSignal = signal<string | null>(null);
  private readonly headerOnlySignal = signal(false);
  private readonly filteringSignal = signal(false);
  private readonly spaceDraggableSignal = signal(false);
  private readonly tabDraggableSignal = signal(false);
  private readonly draggingSignal = signal<string | null>(null);
  private readonly dropMarkSignal = signal<{
    id: string;
    after: boolean;
  } | null>(null);
  private readonly tabDragSignal = signal<{ pane: string } | null>(null);
  protected readonly stopSignal = signal<string | null>(null);
  private readonly tabMarkSignal = signal<{
    pane: string;
    after: boolean;
  } | null>(null);
  private readonly blockedSignal = signal<string | null>(null);

  /** The workspace node of `spaceNodes`; its children are the rows of the body. */
  @Input({ required: true }) set node(value: HxTreeNode<SpaceData>) {
    this.nodeSignal.set(value);
  }
  /** Ids of the panes that have an open view. */
  @Input() set openIds(value: ReadonlySet<string>) {
    this.openIdsSignal.set(value);
  }
  /** Id of the pane whose tab is active in the editor. */
  @Input() set activeId(value: string | null) {
    this.activeIdSignal.set(value);
  }
  /** The key of the one tab stop of the list (roving tabindex, #937). */
  @Input() set stop(value: string | null) {
    this.stopSignal.set(value);
  }
  /**
   * Why the actions of the card cannot run (its machine is not connected, `Machines.blockedReason`,
   * #938): the card is greyed, its actions are disabled with this reason as the tooltip.
   */
  @Input() set blocked(value: string | null) {
    this.blockedSignal.set(value);
  }
  /** Only the header: no further lines and no body, so another layout can reuse the card. */
  @Input() set headerOnly(value: boolean) {
    this.headerOnlySignal.set(value);
  }

  /** A filter is active: a card with a match shows its body, whatever its collapsed state. */
  @Input() set filtering(value: boolean) {
    this.filteringSignal.set(value);
  }
  /** Drag and drop of the header is on (manual sort, no filter, no edit). */
  @Input() set spaceDraggable(value: boolean) {
    this.spaceDraggableSignal.set(value);
  }
  @Input() set tabDraggable(value: boolean) {
    this.tabDraggableSignal.set(value);
  }
  /** The space being dragged and where the drop would land (the state of `Sidebar`). */
  @Input() set dragging(value: string | null) {
    this.draggingSignal.set(value);
  }
  @Input() set dropMark(value: { id: string; after: boolean } | null) {
    this.dropMarkSignal.set(value);
  }
  /** The tab row being dragged and where its drop would land. */
  @Input() set tabDrag(value: { pane: string } | null) {
    this.tabDragSignal.set(value);
  }
  @Input() set tabMark(value: { pane: string; after: boolean } | null) {
    this.tabMarkSignal.set(value);
  }

  @Output() readonly spaceDragStart = new EventEmitter<{
    event: DragEvent;
    id: string;
  }>();
  @Output() readonly spaceDragOver = new EventEmitter<{
    event: DragEvent;
    id: string;
  }>();
  @Output() readonly spaceDrop = new EventEmitter<{
    event: DragEvent;
    id: string;
  }>();
  @Output() readonly tabDragStart = new EventEmitter<{
    event: DragEvent;
    row: PaneRow;
  }>();
  @Output() readonly tabDragOver = new EventEmitter<{
    event: DragEvent;
    row: PaneRow;
  }>();
  @Output() readonly tabDrop = new EventEmitter<{
    event: DragEvent;
    row: PaneRow;
  }>();
  /** The drag ended (also a drop outside the list). */
  @Output() readonly dragEnded = new EventEmitter<void>();

  /** The card header was clicked: the app opens the overview of the space. */
  @Output() readonly opened = new EventEmitter<string>();
  /** The new terminal button: also an entry of the context menu. */
  @Output() readonly act = new EventEmitter<SidebarMenuAction>();
  /** The `⋯` button or a right click: the parent opens the same menu as the tree row has. */
  @Output() readonly menu = new EventEmitter<CardMenuRequest>();
  /** A click on a terminal row. */
  @Output() readonly picked = new EventEmitter<string>();

  private readonly ui = inject(UiState);
  private readonly store = inject(SpacesStore);
  private readonly attn = inject(Attention);
  private readonly bridge = inject(TauriBridge);
  private readonly spaceActions = inject(SpaceActions);
  private readonly paneActions = inject(PaneActions);
  private readonly injector = inject(Injector);
  private readonly spaceRebuild = inject(SpaceRebuild);
  private readonly machines = inject(Machines);

  /** The fold button, the further lines and the body: everything but the header. */
  protected readonly full = computed(() => !this.headerOnlySignal());
  protected readonly data = computed(
    () => this.nodeSignal()!.data as Extract<SpaceData, { kind: "workspace" }>,
  );
  protected readonly id = computed(() => this.data().workspace.space_id);
  protected readonly label = computed(() => this.data().workspace.label);
  protected readonly collapsed = computed(
    () =>
      !this.headerOnlySignal() &&
      !this.filteringSignal() &&
      this.ui.collapsedCards().includes(this.id()),
  );
  protected readonly reason = this.blockedSignal.asReadonly();
  /** A space of a machine: its id is qualified (`<machine>::<id>`). */
  protected readonly remote = computed(() => splitId(this.id()).machine !== undefined);
  /**
   * Where the space runs (#1074): a symbol in the header and, for a machine, a stripe on the left
   * edge. `null` when no machine is configured (every card would say "this computer"). The color
   * is one of {@link ORIGIN_COLORS}, stable per machine name; the label is the accessible name.
   */
  protected readonly origin = computed<SpaceOrigin | null>(() => {
    const machine = splitId(this.id()).machine;
    if (machine === undefined) {
      return {
        kind: "directory",
        label: "This computer",
        tip: "This computer",
        color: null,
      };
    }
    const label = `Machine ${machine}`;
    return {
      kind: "machine",
      label,
      tip: this.blockedSignal() ? `${label}: ${this.blockedSignal()}` : label,
      color: originColor(machine),
    };
  });
  /** The accessible name of the card: the label, then where it runs. */
  protected readonly cardName = computed(() => {
    const origin = this.origin();
    // Where it runs is told only when there is more than this computer.
    return origin && (origin.kind === "machine" || this.machines.machines().length > 0)
      ? `${this.label()}, ${origin.label}`
      : this.label();
  });
  /** The card holds the open terminal: its actions stay visible. */
  protected readonly chosen = computed(() => {
    const active = this.activeIdSignal();
    return active !== null && this.rows().some((r) => this.row(r).row.pane.terminal_id === active);
  });
  protected readonly stateTip = computed(() => {
    const status = this.data().status;
    return `Status: ${stateLabel(status) ?? status}`;
  });
  /** The state word is shown only where the dot is not enough. */
  protected wordy(status: string): boolean {
    return status === "blocked" || status === "error";
  }
  protected readonly detailsHidden = computed(() => this.ui.hiddenDetails().includes(this.id()));
  /** Line 2 of the card, sorted into its parts; `null` when it has nothing to show. */
  protected readonly details = computed<CardDetails | null>(() =>
    splitDetails(this.data().lines[1] ?? [], this.label()),
  );
  protected readonly extraLines = computed(() => this.data().lines.slice(2));
  protected parts(text: string) {
    return middleParts(text);
  }

  protected toggleDetails(): void {
    this.ui.setDetailsHidden(this.id(), !this.detailsHidden());
  }

  protected readonly canDragSpace = computed(
    () => this.spaceDraggableSignal() && this.editing()?.kind !== "space",
  );
  protected readonly canDragTabs = computed(() => this.tabDraggableSignal());
  protected readonly dragState = computed<CardDragState>(() => {
    if (this.draggingSignal() === this.id()) return "source";
    const mark = this.dropMarkSignal();
    if (mark?.id !== this.id()) return null;
    return mark.after ? "after" : "before";
  });

  protected tabDragState(row: PaneRow): CardDragState {
    const id = row.pane.terminal_id;
    if (this.tabDragSignal()?.pane === id) return "source";
    const mark = this.tabMarkSignal();
    if (mark?.pane !== id) return null;
    return mark.after ? "after" : "before";
  }
  protected readonly closing = computed(() => this.spaceActions.closingIds().has(this.id()));
  protected readonly firstHasDot = computed(() => this.data().lines[0]?.[0]?.kind === "dot");
  private readonly space = computed(() => {
    const id = this.id();
    const machine = splitId(id).machine;
    const spaces = machine === undefined ? this.store.spaces() : this.store.remoteSpaces(machine);
    return spaces.find((s) => s.workspace.space_id === id);
  });
  protected readonly unread = computed(() => {
    const space = this.space();
    return space ? workspaceUnread(space, this.attn.unread()) : 0;
  });
  /** A pane of the space turned blocked while not viewed: the whole card rings (rule of `Attention`). */
  protected readonly ringing = computed(() => {
    const ringing = this.attn.attention();
    return !!this.space()?.rows.some((r) => ringing.has(r.pane.terminal_id));
  });
  protected readonly pinned = computed(
    () => this.store.pinnedSpaces?.().includes(this.id()) ?? false,
  );

  /** The clock of the silent marks; ticks once a minute. */
  private readonly now = signal(Date.now());
  protected readonly clock = (() => {
    const timer = setInterval(() => this.now.set(Date.now()), 60_000);
    inject(DestroyRef).onDestroy(() => clearInterval(timer));
    return timer;
  })();

  /** The longest silence among the agents of the space, as on the tree row. */
  protected readonly silent = computed(() => {
    const since = (this.space()?.rows ?? [])
      .map((r) => r.pane.agent_silent_since)
      .filter((t): t is number => t !== undefined);
    return since.length ? silentLabel(Math.min(...since), this.now()) : undefined;
  });

  /** The rebuild mark of a space that has no terminals; none for any other. */
  protected readonly rebuildMark = computed<RebuildMark | undefined>(() => {
    const mark = this.spaceRebuild.marks().get(this.id());
    return mark && this.space()?.rows.length === 0 ? mark : undefined;
  });

  /** What is renamed in place: the space (the header) or a terminal; `null` when none. */
  protected readonly editing = signal<{
    kind: "space" | "pane";
    id: string;
  } | null>(null);
  protected readonly renameError = computed(() =>
    this.paneActions.renaming() !== null && this.editing() === null
      ? this.paneActions.error()
      : this.spaceActions.renameError(),
  );
  protected readonly rows = computed(() => this.nodeSignal()!.children ?? []);

  /** Terminals whose folded child records the user opened. */
  private readonly openRecords = signal<ReadonlySet<string>>(new Set());

  /** Space acts on the treeitem itself; on a button inside it the button's own click wins. */
  protected onSpace(event: Event, act: () => void): void {
    if (event.target !== event.currentTarget) return;
    event.preventDefault();
    act();
  }

  /** The header was clicked: a space of a machine has no overview, so nothing opens for it. */
  protected open(): void {
    if (!this.remote()) this.opened.emit(this.id());
  }

  protected toggle(): void {
    this.ui.setCardCollapsed(this.id(), !this.collapsed());
  }

  protected row(node: HxTreeNode): Extract<SpaceData, { kind: "pane" }> {
    return node.data as Extract<SpaceData, { kind: "pane" }>;
  }

  protected marks(row: PaneRow) {
    return paneMarks(row.pane.terminal_id, this.openIdsSignal(), this.activeIdSignal());
  }

  protected rings(row: PaneRow): boolean {
    return this.attn.attention().has(row.pane.terminal_id);
  }

  /** The name of an unread terminal, or of a space with an unread terminal, is semibold (#1031). */
  protected nameUnread(edit: { kind: "space" | "pane"; id: string } | undefined): boolean {
    if (!edit) return false;
    return edit.kind === "space" ? this.unread() > 0 : this.attn.unread().has(edit.id);
  }

  protected isUnread(row: PaneRow): boolean {
    return this.attn.unread().has(row.pane.terminal_id);
  }

  protected silentMark(row: PaneRow): string | undefined {
    return silentLabel(row.pane.agent_silent_since, this.now());
  }

  protected rebuild(event: Event): void {
    event.stopPropagation();
    void this.spaceRebuild.rebuild(this.id());
  }

  protected isEditing(edit: { kind: "space" | "pane"; id: string }): boolean {
    if (edit.kind === "pane") return this.paneActions.renaming() === edit.id;
    const now = this.editing();
    return now?.kind === edit.kind && now.id === edit.id;
  }

  protected startEdit(edit: { kind: "space" | "pane"; id: string }, event: Event): void {
    event.stopPropagation();
    if (this.closing() || this.blockedSignal()) return;
    if (edit.kind === "pane") {
      this.paneActions.askRename(edit.id);
      return;
    }
    this.spaceActions.clearRenameError();
    this.editing.set(edit);
  }

  protected renameBusy(edit: { kind: "space" | "pane" }): boolean {
    return edit.kind === "pane" && this.paneActions.renameBusy();
  }

  /** Enter that ends an IME composition (or is keyCode 229) only picks the candidate. */
  protected enter(
    edit: { kind: "space" | "pane"; id: string },
    input: HTMLInputElement,
    event: Event,
  ): void {
    if (isComposing(event)) return;
    void this.finishEdit(edit, input, true);
  }

  protected clearError(edit: { kind: "space" | "pane" }): void {
    if (edit.kind === "pane") this.paneActions.clearError();
    else this.spaceActions.clearRenameError();
  }

  /** Enter or blur confirms, Escape cancels; a failed rename keeps editing, the old label stays. */
  protected async finishEdit(
    edit: { kind: "space" | "pane"; id: string },
    input: HTMLInputElement,
    commit: boolean,
  ): Promise<void> {
    if (!this.isEditing(edit)) return;
    if (this.renameBusy(edit)) return;
    if (!commit) {
      if (edit.kind === "pane") this.paneActions.cancelRename();
      else this.editing.set(null);
      this.spaceActions.clearRenameError();
      return;
    }
    const ok =
      edit.kind === "space"
        ? await this.spaceActions.rename(edit.id, input.value)
        : await this.paneActions.rename(edit.id, input.value);
    if (ok) {
      if (edit.kind === "pane") this.paneActions.cancelRename();
      else this.editing.set(null);
    } else refocus(input, this.injector);
  }

  protected glyph(row: PaneRow): string {
    return row.agent ? row.agent.slice(0, 2) : ">_";
  }

  protected title(row: PaneRow): string {
    return [row.pane.terminal_id, row.dir, row.branch].filter(Boolean).join(" · ");
  }

  protected hostNote(row: PaneRow): string | null {
    const host = olderHost(row.pane);
    return host ? olderHostNote(host) : null;
  }

  protected records(row: PaneRow) {
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

  protected openPull(url: string, event: Event): void {
    event.stopPropagation();
    if (isGithubUrl(url)) void this.bridge.openUrl(url);
  }

  protected askMenu(event: MouseEvent, kind: SidebarMenuAction["kind"], id: string): void {
    event.preventDefault();
    event.stopPropagation();
    if (kind === "workspace" && this.closing()) return;
    if (this.blockedSignal()) return;
    this.menu.emit({ kind, id, x: event.clientX, y: event.clientY });
  }

  /** The `⋯` button: the menu opens below the button, also for a keyboard click. */
  protected askMore(event: MouseEvent): void {
    event.stopPropagation();
    if (this.closing() || this.blockedSignal()) return;
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.menu.emit({
      kind: "workspace",
      id: this.id(),
      x: box.left,
      y: box.bottom + 2,
    });
  }

  protected newTerminal(event: Event): void {
    event.stopPropagation();
    this.act.emit({ kind: "workspace", id: this.id(), action: "new-terminal" });
  }
}
