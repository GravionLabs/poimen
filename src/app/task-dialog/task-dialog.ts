import { ChangeDetectionStrategy, Component, ElementRef, effect, inject } from "@angular/core";
import { LEVEL_FILTERS, TaskPicker, errorText, markers } from "./task-picker";

/**
 * Picks an open issue and a task mode. Only returns the choice (see `TaskPicker`); keys: Up/Down
 * move, Enter picks, Tab switches the mode, Esc cancels.
 */
@Component({
  selector: "app-task-dialog",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./task-dialog.html",
  styleUrl: "./task-dialog.css",
})
export class TaskDialog {
  protected readonly picker = inject(TaskPicker);
  protected readonly levels = LEVEL_FILTERS;
  protected readonly markers = markers;
  protected readonly errorText = errorText;
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  constructor() {
    effect(() => {
      if (!this.picker.isOpen()) return;
      queueMicrotask(() =>
        this.host.nativeElement.querySelector<HTMLInputElement>("input")?.focus(),
      );
    });
    effect(() => {
      this.picker.index();
      queueMicrotask(() =>
        this.host.nativeElement
          .querySelector(".issue.active")
          ?.scrollIntoView?.({ block: "nearest" }),
      );
    });
  }

  protected onKey(event: KeyboardEvent): void {
    const handlers: Record<string, () => void> = {
      ArrowDown: () => this.picker.move(1),
      ArrowUp: () => this.picker.move(-1),
      Enter: () => this.picker.confirm(),
      Tab: () => this.picker.toggleMode(),
      Escape: () => this.picker.cancel(),
    };
    const handler = handlers[event.key];
    if (!handler) return;
    event.preventDefault();
    handler();
  }

  protected input(event: Event): void {
    this.picker.setQuery((event.target as HTMLInputElement).value);
  }
}
