import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  computed,
  effect,
  inject,
} from "@angular/core";
import { LEVEL_FILTERS, TaskPicker, errorText, markers } from "./task-picker";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";
import { HxButton, HxField, HxSelect, HxInput, HxMessage } from "@gravionlabs/helix-ui";
import { LoadingRows } from "../ui/loading/loading-rows";
import { selectOptions } from "../ui/select/select-options";

/**
 * Picks an open issue and a task mode. Only returns the choice (see `TaskPicker`); keys: Up/Down
 * move, Enter picks, Tab switches the mode, Esc cancels.
 */
@Component({
  selector: "app-task-dialog",
  imports: [LoadingRows, DialogShell, HxButton, HxField, HxSelect, HxInput, HxMessage],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./task-dialog.html",
  styleUrl: "./task-dialog.css",
})
export class TaskDialog {
  protected readonly picker = inject(TaskPicker);
  protected readonly levels = LEVEL_FILTERS;
  protected readonly markers = markers;
  protected readonly errorText = errorText;
  protected readonly machineOptions = computed(() => [
    { label: "This Mac", value: "" },
    ...selectOptions(this.picker.machines()),
  ]);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  constructor() {
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
    // The machine select owns its keys: open, move in the list, pick and close.
    if ((event.target as Element | null)?.closest?.("hx-select")) return;
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

  protected machine(value: string | null): void {
    this.picker.setMachine(value || null);
  }

  protected input(event: Event): void {
    this.picker.setQuery((event.target as HTMLInputElement).value);
  }
}
