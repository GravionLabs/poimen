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

  /** Optional template for the content of a tab (`let-tab`), rendered after the label. */
  readonly tabContent = contentChild<TemplateRef<{ $implicit: TabBarItem }>>("tabContent");

  protected readonly dragging = signal<number | null>(null);
  protected readonly dropAt = signal<number | null>(null);

  protected dragStart(event: DragEvent, index: number): void {
    this.dragging.set(index);
    event.dataTransfer?.setData("text/plain", String(index));
    if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
  }

  protected dragOver(event: DragEvent, index: number): void {
    if (this.dragging() === null) return;
    event.preventDefault();
    this.dropAt.set(index);
  }

  protected drop(event: DragEvent, index: number): void {
    event.preventDefault();
    const from = this.dragging();
    this.dragEnd();
    const move = dropMove(from, index);
    if (move) this.moved.emit(move);
  }

  protected dragEnd(): void {
    this.dragging.set(null);
    this.dropAt.set(null);
  }

  protected middleClick(event: MouseEvent, id: string): void {
    if (event.button !== 1) return;
    event.preventDefault();
    this.closed.emit(id);
  }

  /** Arrow keys move between tabs, Delete closes the focused one. */
  protected key(event: KeyboardEvent, index: number): void {
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
