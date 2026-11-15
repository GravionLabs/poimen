import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Input,
  Output,
  signal,
} from "@angular/core";

/**
 * Chips for a set of names (reviewers, labels, assignees) with a field that adds one from the
 * options. It only reports `add` and `remove`; the owner decides what to do (decorator inputs so
 * the specs run without Angular's compiler transform). Reused by the issue detail (#340).
 */
@Component({
  selector: "app-chip-picker",
  templateUrl: "./chip-picker.html",
  styleUrl: "./chip-picker.css",
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ChipPicker {
  @Input({ required: true }) label = "";
  @Input() selected: readonly string[] = [];
  @Input() options: readonly string[] = [];
  @Input() disabled = false;
  @Output() readonly add = new EventEmitter<string>();
  @Output() readonly remove = new EventEmitter<string>();

  protected readonly query = signal("");

  /** The options not chosen yet. */
  protected free(): string[] {
    return this.options.filter((o) => !this.selected.includes(o));
  }

  /** Options matching the query, at most 8. */
  protected matches(): string[] {
    const q = this.query().trim().toLowerCase();
    return this.free()
      .filter((o) => o.toLowerCase().includes(q))
      .slice(0, 8);
  }

  protected pick(name: string): void {
    if (this.disabled || this.selected.includes(name) || !this.options.includes(name)) return;
    this.add.emit(name);
    this.query.set("");
  }

  protected takeFirst(): void {
    const first = this.matches()[0];
    if (first) this.pick(first);
  }
}
