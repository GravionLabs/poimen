import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  output,
  signal,
} from "@angular/core";
import { type Terminal } from "../core/snapshot";
import { agentKinds } from "../core/backend";
import { SpacesStore } from "../core/spaces-store";
import { PaneActions, type PaneClosed } from "../core/pane-actions";

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
  readonly created = output<Terminal>();
  /** A pane was closed; the host drops its tab or hands it to the successor. */
  readonly closed = output<PaneClosed>();
  private readonly store = inject(SpacesStore);
  /** Only the kinds that can report on the active backend (#410). */
  protected readonly kinds = computed(() => agentKinds(this.store.backendKind()));
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
    const closed = await this.actions.confirmClose();
    if (closed) this.closed.emit(closed);
  }

  protected async restart(): Promise<void> {
    const closed = await this.actions.confirmRestart();
    if (closed) this.closed.emit(closed);
  }

  protected async startAgent(form: HTMLFormElement): Promise<void> {
    const data = new FormData(form);
    const field = (key: string) => String(data.get(key) ?? "");
    await this.actions.confirmStartAgent(field("kind"), field("name"), field("args"));
  }

  protected async rename(form: HTMLFormElement): Promise<void> {
    await this.actions.confirmRename(String(new FormData(form).get("name") ?? ""));
  }

  protected cancel(): void {
    this.typed.set(null);
    this.actions.cancel();
  }
}
