import { NgTemplateOutlet } from "@angular/common";
import {
  ChangeDetectionStrategy,
  Component,
  TemplateRef,
  contentChild,
  input,
  output,
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
export class TabBar {
  readonly tabs = input.required<readonly TabBarItem[]>();
  readonly activeId = input<string | null>(null);
  readonly selected = output<string>();
  readonly closed = output<string>();
  /** Double click on a tab. */
  readonly pinned = output<string>();
  readonly moved = output<{ from: number; to: number }>();
  /** Identifies this bar when a tab is dragged between bars. */
  readonly groupId = input("");
  /** A tab dragged from another bar was dropped here, at `index`. */
  readonly received = output<{ from: string; id: string; index: number }>();
  /** Entries of the context menu; none disables it. */
  readonly menu = input<readonly TabBarMenuItem[]>([]);
  readonly menuAction = output<{ id: string; action: string }>();

  /** Optional template for the content of a tab (`let-tab`), rendered after the label. */
  readonly tabContent = contentChild<TemplateRef<{ $implicit: TabBarItem }>>("tabContent");

  protected readonly dragging = signal<number | null>(null);
  protected readonly dropAt = signal<number | null>(null);
  /** Where a tab from another bar would land. */
  protected readonly foreignAt = signal<number | null>(null);
  /** The entries offered for the tab `id`. */
  protected menuItems(id: string): readonly TabBarMenuItem[] {
    return itemsForTab(this.menu(), id);
  }

  protected readonly menuAt = signal<{ id: string; x: number; y: number } | null>(null);

  protected dragStart(event: DragEvent, index: number): void {
    this.dragging.set(index);
    activeDrag = { group: this.groupId(), id: this.tabs()[index].id };
    event.dataTransfer?.setData("text/plain", String(index));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  private foreign(): boolean {
    return activeDrag !== null && activeDrag.group !== this.groupId();
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
    this.foreignAt.set(this.tabs().length);
  }

  protected stripDrop(event: DragEvent): void {
    this.drop(event, this.tabs().length);
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
    if (this.menu().length === 0) return;
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
    if (event.key === "Escape") this.closeMenu();
    const tabs = this.tabs();
    const target = arrowTarget(event.key, index, tabs.length);
    if (target !== null) {
      this.selected.emit(tabs[target].id);
      event.preventDefault();
    } else if (event.key === "Delete") {
      this.closed.emit(tabs[index].id);
      event.preventDefault();
    }
  }
}
