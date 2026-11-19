import { NgTemplateOutlet } from "@angular/common";
import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
  computed,
  inject,
  signal,
} from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import type { HxTreeNode } from "@gravionlabs/helix-ui";
import { Attention, workspaceUnread } from "../core/attention";
import { SpacesStore } from "../core/spaces-store";
import { olderHost, olderHostNote, type PaneRow } from "../core/snapshot";
import { SpaceActions } from "../core/space-actions";
import { TauriBridge } from "../core/tauri";
import { UiState } from "../core/ui-state";
import { isGithubUrl } from "../github/pulls";
import type { SpaceData } from "./sidebar";
import type { SidebarMenuAction } from "./sidebar-menu";
import { RECORDS_SHOWN, paneMarks, recordRows, recordsSummary } from "./spaces";

/** Where a card asks for a context menu: the kind, the id and the point it opens at. */
export interface CardMenuRequest {
  kind: SidebarMenuAction["kind"];
  id: string;
  x: number;
  y: number;
}

/**
 * One card per space for the `cards` sidebar layout (#934). It renders the node `spaceNodes` builds
 * (no second model): the header (status dot, first line of `sidebar.rows.space`, unread count and
 * the actions), the further lines of the space row, and a body with the tabs and terminals.
 */
@Component({
  selector: "app-space-card",
  imports: [HxButton, NgTemplateOutlet],
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
  /** Only the header: no further lines and no body, so another layout can reuse the card. */
  @Input() set headerOnly(value: boolean) {
    this.headerOnlySignal.set(value);
  }

  /** The card header was clicked: the app opens the overview of the space. */
  @Output() readonly opened = new EventEmitter<string>();
  @Output() readonly configure = new EventEmitter<string>();
  /** New terminal and close space: the entries of the context menu with an icon. */
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

  /** The fold button, the further lines and the body: everything but the header. */
  protected readonly full = computed(() => !this.headerOnlySignal());
  protected readonly data = computed(
    () => this.nodeSignal()!.data as Extract<SpaceData, { kind: "workspace" }>,
  );
  protected readonly id = computed(() => this.data().workspace.space_id);
  protected readonly label = computed(() => this.data().workspace.label);
  protected readonly collapsed = computed(
    () => !this.headerOnlySignal() && this.ui.collapsedCards().includes(this.id()),
  );
  protected readonly closing = computed(() => this.spaceActions.closingIds().has(this.id()));
  protected readonly firstHasDot = computed(() => this.data().lines[0]?.[0]?.kind === "dot");
  protected readonly unread = computed(() => {
    const space = this.store.spaces().find((s) => s.workspace.space_id === this.id());
    return space ? workspaceUnread(space, this.attn.unread()) : 0;
  });
  protected readonly rows = computed(() => this.nodeSignal()!.children ?? []);

  /** Terminals whose folded child records the user opened. */
  private readonly openRecords = signal<ReadonlySet<string>>(new Set());

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

  protected isUnread(row: PaneRow): boolean {
    return this.attn.unread().has(row.pane.terminal_id);
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
    this.menu.emit({ kind, id, x: event.clientX, y: event.clientY });
  }

  /** The `⋯` button: the menu opens below the button, also for a keyboard click. */
  protected askMore(event: MouseEvent): void {
    event.stopPropagation();
    if (this.closing()) return;
    const box = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.menu.emit({ kind: "workspace", id: this.id(), x: box.left, y: box.bottom + 2 });
  }

  protected newTerminal(event: Event): void {
    event.stopPropagation();
    this.act.emit({ kind: "workspace", id: this.id(), action: "new-terminal" });
  }

  protected closeSpace(event: Event): void {
    event.stopPropagation();
    this.act.emit({ kind: "workspace", id: this.id(), action: "close-space" });
  }

  protected configureSpace(event: Event): void {
    event.stopPropagation();
    this.configure.emit(this.id());
  }
}
