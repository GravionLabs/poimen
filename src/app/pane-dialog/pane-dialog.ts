import { ChangeDetectionStrategy, Component, inject, output, signal } from "@angular/core";
import type { Pane } from "../core/herdr";
import { PaneActions } from "../core/pane-actions";

/** The question for a new terminal (directory) or a pane close (confirmation), and action errors. */
@Component({
  selector: "app-pane-dialog",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./pane-dialog.html",
  styleUrl: "./pane-dialog.css",
})
export class PaneDialog {
  protected readonly actions = inject(PaneActions);
  /** A terminal was created; the host opens it. */
  readonly created = output<Pane>();
  /** A pane was closed; the host drops its tab. */
  readonly closed = output<string>();
  private readonly typed = signal<string | null>(null);

  protected cwd(fallback: string): string {
    return this.typed() ?? fallback;
  }

  protected type(event: Event): void {
    this.typed.set((event.target as HTMLInputElement).value);
  }

  protected async create(fallback: string): Promise<void> {
    const cwd = this.cwd(fallback);
    this.typed.set(null);
    const pane = await this.actions.confirmNew(cwd);
    if (pane) this.created.emit(pane);
  }

  protected async close(): Promise<void> {
    const id = await this.actions.confirmClose();
    if (id) this.closed.emit(id);
  }

  protected cancel(): void {
    this.typed.set(null);
    this.actions.cancel();
  }
}
