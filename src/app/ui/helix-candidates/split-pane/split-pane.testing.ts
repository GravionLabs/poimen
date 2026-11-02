import { NgTemplateOutlet } from "@angular/common";
import { Component, ContentChild, EventEmitter, Input, Output, TemplateRef } from "@angular/core";
import { resizeSizes } from "../../../core/editor-layout";
import { paneSizes } from "./split-pane";

/**
 * Test double of `SplitPane` for JIT specs: Vitest compiles components without the Angular
 * compiler, so signal inputs (`input.required`) are never bound. Same selector, inputs and markup;
 * arrow keys on a divider emit `sizesChange` like the real one.
 */
@Component({
  selector: "app-split-pane",
  imports: [NgTemplateOutlet],
  template: `
    @for (i of indexes(); track i) {
      <div class="pane" [style.flex-grow]="shares()[i]">
        @if (pane) {
          <ng-container [ngTemplateOutlet]="pane" [ngTemplateOutletContext]="{ $implicit: i }" />
        }
      </div>
      @if (i < count - 1) {
        <div class="divider" role="separator" tabindex="0" (keydown)="key($event, i)"></div>
      }
    }
  `,
})
export class SplitPaneStub {
  @Input() direction: "row" | "column" = "row";
  @Input() count = 0;
  @Input() sizes: readonly number[] | undefined;
  @Output() readonly sizesChange = new EventEmitter<number[]>();
  @ContentChild("pane") pane: TemplateRef<{ $implicit: number }> | null = null;

  shares(): number[] {
    return paneSizes(this.sizes, this.count);
  }

  indexes(): number[] {
    return Array.from({ length: this.count }, (_, i) => i);
  }

  key(event: KeyboardEvent, index: number): void {
    const forward = event.key === (this.direction === "column" ? "ArrowDown" : "ArrowRight");
    const back = event.key === (this.direction === "column" ? "ArrowUp" : "ArrowLeft");
    if (!forward && !back) return;
    const step = event.shiftKey ? 0.1 : 0.02;
    this.sizesChange.emit(resizeSizes(this.shares(), index, forward ? step : -step));
  }
}
