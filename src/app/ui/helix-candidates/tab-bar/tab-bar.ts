import { NgTemplateOutlet } from "@angular/common";
import {
  ChangeDetectionStrategy,
  Component,
  ContentChild,
  DestroyRef,
  ElementRef,
  EventEmitter,
  Injector,
  Input,
  OnChanges,
  Output,
  TemplateRef,
  ViewChild,
  afterNextRender,
  inject,
  signal,
} from "@angular/core";

/** One tab of the bar. Generic on purpose: no herdr knowledge (candidate for helix-ui, ADR 0001). */
export interface TabBarItem {
  id: string;
  label: string;
  /** Shown uppercase before the label, separated by a slash (e.g. the workspace). */
  group?: string;
  /** Shown in italics, as VS Code does for a preview tab. */
  preview?: boolean;
  title?: string;
  /** Rings the tab (it needs the user) until the owner clears it. */
  attention?: boolean;
  /** Shows an unread badge. */
  unread?: boolean;
  /** A quiet mark after the label; the text is its tooltip. */
  note?: string;
}

/** An entry of the tab context menu; `action` is reported back with the tab id. */
export interface TabBarMenuItem {
  action: string;
  label: string;
  shortcut?: string;
  disabled?: boolean;
  /** Whether the entry is offered for the tab with this id; absent means for every tab. */
  appliesTo?: (tabId: string) => boolean;
}

/** The menu entries offered for the tab `id`. */
export function itemsForTab(
  items: readonly TabBarMenuItem[],
  id: string,
): readonly TabBarMenuItem[] {
  return items.filter((item) => !item.appliesTo || item.appliesTo(id));
}

/** The tab being dragged, shared by all bars so another bar can accept it. */
let activeDrag: { group: string; id: string } | null = null;

/** The move a drop makes, or `null` when nothing was dragged or it landed on the dragged tab. */
export function dropMove(from: number | null, to: number): { from: number; to: number } | null {
  return from === null || from === to ? null : { from, to };
}

/** The tab an arrow key selects from `index` (wrapping), or `null` for other keys. */
export function arrowTarget(key: string, index: number, count: number): number | null {
  if (count === 0) return null;
  if (key === "ArrowRight") return (index + 1) % count;
  if (key === "ArrowLeft") return (index - 1 + count) % count;
  return null;
}

/** Tabs side by side in one row with close buttons and drag-to-reorder; projects trailing actions. */
@Component({
  selector: "app-tab-bar",
  imports: [NgTemplateOutlet],
  templateUrl: "./tab-bar.html",
  styleUrl: "./tab-bar.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class TabBar implements OnChanges {
  @Input({ required: true }) tabs: readonly TabBarItem[] = [];
  @Input() activeId: string | null = null;
  @Output() readonly selected = new EventEmitter<string>();
  @Output() readonly closed = new EventEmitter<string>();
  /** Double click on a tab. */
  @Output() readonly pinned = new EventEmitter<string>();
  @Output() readonly moved = new EventEmitter<{ from: number; to: number }>();
  /** Identifies this bar when a tab is dragged between bars. */
  @Input() groupId = "";
  /** A tab dragged from another bar was dropped here, at `index`. */
  @Output() readonly received = new EventEmitter<{ from: string; id: string; index: number }>();
  /** Entries of the context menu; none disables it. */
  @Input() menu: readonly TabBarMenuItem[] = [];
  @Output() readonly menuAction = new EventEmitter<{ id: string; action: string }>();

  /** Optional template for the content of a tab (`let-tab`), rendered after the label. */
  @ContentChild("tabContent") tabContent?: TemplateRef<{ $implicit: TabBarItem }>;
  @ViewChild("strip") private readonly strip?: ElementRef<HTMLElement>;

  private readonly injector = inject(Injector);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  /** Whether the tabs are wider than the strip: scroll buttons and the all-tabs button show. */
  protected readonly overflowing = signal(false);
  /** Whether the all-tabs menu is open, with its anchor. */
  protected readonly allAt = signal<{ right: number; y: number } | null>(null);

  constructor() {
    const destroyRef = inject(DestroyRef);
    afterNextRender(() => {
      this.measure();
      if (typeof ResizeObserver === "undefined") return;
      const observer = new ResizeObserver(() => this.measure());
      observer.observe(this.host.nativeElement);
      destroyRef.onDestroy(() => observer.disconnect());
    });
  }

  /** The active id last brought into view; new arrays with the same active tab keep the user's scroll. */
  private revealed: string | null = null;

  private pending = false;

  ngOnChanges(): void {
    this.schedule();
  }

  /** After the new tabs are rendered: re-measure and bring a newly (or again) requested active tab into view. */
  private schedule(): void {
    if (this.pending) return;
    this.pending = true;
    afterNextRender(
      () => {
        this.pending = false;
        this.measure();
        if (this.activeId !== this.revealed) {
          // An active tab that is not rendered (removed, or null) forgets the reveal so its return scrolls.
          this.revealed = this.revealActive() ? this.activeId : null;
        } else if (this.activeId !== null && !this.isRendered(this.activeId)) {
          this.revealed = null;
        }
      },
      { injector: this.injector },
    );
  }

  private isRendered(id: string): boolean {
    return this.tabs.some((tab) => tab.id === id);
  }

  /** The user activated a tab (also the active one): emit and bring it into view. */
  protected activate(id: string): void {
    this.selected.emit(id);
    this.revealed = null;
    this.schedule();
  }

  /** Re-reads whether the strip overflows (resize, tab add/remove, scroll). */
  protected measure(): void {
    const el = this.strip?.nativeElement;
    if (el) this.overflowing.set(el.scrollWidth > el.clientWidth + 1);
  }

  /** Scrolls the strip by 60% of its width in `direction` (-1 left, 1 right). */
  protected scrollBy(direction: -1 | 1): void {
    const el = this.strip?.nativeElement;
    el?.scrollBy?.({ left: direction * el.clientWidth * 0.6, behavior: "smooth" });
  }

  /** The vertical wheel scrolls the strip sideways while it overflows. */
  protected wheel(event: WheelEvent): void {
    const el = this.strip?.nativeElement;
    if (!el || !this.overflowing() || Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
    event.preventDefault();
    el.scrollLeft += event.deltaMode === 1 ? event.deltaY * 16 : event.deltaY;
  }

  /** Scrolls the active tab into view. */
  private revealActive(): boolean {
    const el = this.strip?.nativeElement;
    if (!el || this.activeId === null) return false;
    const index = this.tabs.findIndex((tab) => tab.id === this.activeId);
    const tab = el.querySelectorAll<HTMLElement>('[role="tab"]')[index];
    if (!tab) return false;
    tab.scrollIntoView?.({ block: "nearest", inline: "nearest" });
    return true;
  }

  protected openAll(event: MouseEvent): void {
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    this.allAt.set({ right: window.innerWidth - rect.right, y: rect.bottom });
  }

  protected closeAll(): void {
    this.allAt.set(null);
  }

  protected chooseTab(id: string): void {
    this.closeAll();
    this.activate(id);
  }

  protected readonly dragging = signal<number | null>(null);
  protected readonly dropAt = signal<number | null>(null);
  /** Where a tab from another bar would land. */
  protected readonly foreignAt = signal<number | null>(null);
  /** The entries offered for the tab `id`. */
  protected menuItems(id: string): readonly TabBarMenuItem[] {
    return itemsForTab(this.menu, id);
  }

  protected readonly menuAt = signal<{ id: string; x: number; y: number } | null>(null);

  protected dragStart(event: DragEvent, index: number): void {
    this.dragging.set(index);
    activeDrag = { group: this.groupId, id: this.tabs[index].id };
    event.dataTransfer?.setData("text/plain", String(index));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  private foreign(): boolean {
    return activeDrag !== null && activeDrag.group !== this.groupId;
  }

  protected dragOver(event: DragEvent, index: number): void {
    if (this.foreign()) {
      event.preventDefault();
      event.stopPropagation();
      this.foreignAt.set(index);
      return;
    }
    if (this.dragging() === null) return;
    event.preventDefault();
    this.dropAt.set(index);
  }

  /** Over the free part of the strip: a tab from another bar lands at the end. */
  protected stripOver(event: DragEvent): void {
    if (!this.foreign()) return;
    event.preventDefault();
    this.foreignAt.set(this.tabs.length);
  }

  protected stripDrop(event: DragEvent): void {
    this.drop(event, this.tabs.length);
  }

  protected drop(event: DragEvent, index: number): void {
    event.preventDefault();
    event.stopPropagation();
    if (this.foreign()) {
      const { group, id } = activeDrag!;
      this.dragEnd();
      this.received.emit({ from: group, id, index });
      return;
    }
    const from = this.dragging();
    this.dragEnd();
    const move = dropMove(from, index);
    if (move) this.moved.emit(move);
  }

  protected dragEnd(): void {
    this.dragging.set(null);
    this.dropAt.set(null);
    this.foreignAt.set(null);
    activeDrag = null;
  }

  protected openMenu(event: MouseEvent, id: string): void {
    if (this.menu.length === 0) return;
    event.preventDefault();
    this.menuAt.set({ id, x: event.clientX, y: event.clientY });
  }

  protected closeMenu(): void {
    this.menuAt.set(null);
  }

  protected choose(item: TabBarMenuItem): void {
    const at = this.menuAt();
    this.closeMenu();
    if (at && !item.disabled) this.menuAction.emit({ id: at.id, action: item.action });
  }

  protected middleClick(event: MouseEvent, id: string): void {
    if (event.button !== 1) return;
    event.preventDefault();
    this.closed.emit(id);
  }

  /** Arrow keys move between tabs, Delete closes the focused one. */
  protected key(event: KeyboardEvent, index: number): void {
    if (event.key === "Escape") {
      this.closeMenu();
      this.closeAll();
    }
    const tabs = this.tabs;
    const target = arrowTarget(event.key, index, tabs.length);
    if (target !== null) {
      this.activate(tabs[target].id);
      event.preventDefault();
    } else if (event.key === "Delete") {
      this.closed.emit(tabs[index].id);
      event.preventDefault();
    }
  }
}
