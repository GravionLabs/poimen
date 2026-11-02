import { NgTemplateOutlet } from "@angular/common";
import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  TemplateRef,
  computed,
  contentChild,
  inject,
  input,
  output,
} from "@angular/core";
import { resizeSizes } from "../../../core/editor-layout";

/** Fractions that fill the pane: `sizes` when they match `count`, else equal shares. */
export function paneSizes(sizes: readonly number[] | undefined, count: number): number[] {
  return sizes && sizes.length === count
    ? [...sizes]
    : Array.from({ length: count }, () => 1 / count);
}

/**
 * Panes side by side or stacked with draggable, keyboard-operable dividers. Generic on purpose: no
 * herdr knowledge (candidate for helix-ui, ADR 0001). The `#pane` template (`let-index`) renders
 * each pane; sizes are fractions that sum to 1.
 */
@Component({
  selector: "app-split-pane",
  imports: [NgTemplateOutlet],
  templateUrl: "./split-pane.html",
  styleUrl: "./split-pane.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { "[class.column]": "direction() === 'column'" },
})
export class SplitPane {
  readonly direction = input<"row" | "column">("row");
  readonly count = input.required<number>();
  readonly sizes = input<readonly number[]>();
  readonly sizesChange = output<number[]>();
  readonly pane = contentChild<TemplateRef<{ $implicit: number }>>("pane");

  protected readonly shares = computed(() => paneSizes(this.sizes(), this.count()));
  protected readonly indexes = computed(() => Array.from({ length: this.count() }, (_, i) => i));
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  private drag: { start: number; sizes: number[]; extent: number } | null = null;

  protected dragStart(event: PointerEvent): void {
    const handle = event.target as HTMLElement;
    handle.setPointerCapture(event.pointerId);
    handle.classList.add("dragging");
    const box = this.host.nativeElement.getBoundingClientRect();
    const column = this.direction() === "column";
    this.drag = {
      start: column ? event.clientY : event.clientX,
      sizes: this.shares(),
      extent: (column ? box.height : box.width) || 1,
    };
  }

  protected dragMove(event: PointerEvent, index: number): void {
    if (!this.drag) return;
    const at = this.direction() === "column" ? event.clientY : event.clientX;
    const delta = (at - this.drag.start) / this.drag.extent;
    this.sizesChange.emit(resizeSizes(this.drag.sizes, index, delta));
  }

  protected dragEnd(event: PointerEvent): void {
    this.drag = null;
    (event.target as HTMLElement).classList.remove("dragging");
  }

  protected key(event: KeyboardEvent, index: number): void {
    const column = this.direction() === "column";
    const back = event.key === (column ? "ArrowUp" : "ArrowLeft");
    const forward = event.key === (column ? "ArrowDown" : "ArrowRight");
    if (!back && !forward) return;
    event.preventDefault();
    const step = event.shiftKey ? 0.1 : 0.02;
    this.sizesChange.emit(resizeSizes(this.shares(), index, forward ? step : -step));
  }
}
