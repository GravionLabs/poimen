import { ChangeDetectionStrategy, Component, Input, computed, signal } from "@angular/core";
import { HxSkeleton } from "@gravionlabs/helix-ui";
import { SKELETON_DELAY_MS, delayed } from "./delayed";

/**
 * Placeholder rows for a view that loads (#1041): nothing for the first 200 ms, then `rows`
 * skeleton lines. The container announces the state (`role="status"`, hidden "Loading" text);
 * the skeletons themselves are `aria-hidden`. Decorator inputs: specs run without the compiler.
 */
@Component({
  selector: "app-loading-rows",
  imports: [HxSkeleton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./loading-rows.html",
  styles: `
    .rows {
      display: flex;
      flex-direction: column;
      gap: 10px;
      padding: 12px;
    }
  `,
})
export class LoadingRows {
  private readonly _active = signal(false);
  private readonly _rows = signal(5);
  @Input() set active(value: boolean) {
    this._active.set(value);
  }
  @Input() set rows(value: number) {
    this._rows.set(value);
  }
  @Input() label = "Loading";
  @Input() testId = "loading-rows";
  protected readonly visible = delayed(() => this._active(), SKELETON_DELAY_MS);
  protected readonly rowList = computed(() => Array.from({ length: this._rows() }, (_, i) => i));
  protected widthOf(row: number): string {
    return ["100%", "82%", "92%", "66%", "76%"][row % 5] ?? "100%";
  }
}
