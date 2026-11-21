import { ChangeDetectionStrategy, Component, inject, output, signal } from "@angular/core";
import { SpaceActions, dirName } from "../core/space-actions";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";
import { delayedBusy } from "../ui/loading/delayed";
import { Icon } from "../ui/icon/icon";
import { HxButton } from "@gravionlabs/helix-ui";

/** The question for a new space: a directory (typed with completion or picked) and a label. */
@Component({
  selector: "app-space-dialog",
  imports: [DialogShell, HxButton, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./space-dialog.html",
  styleUrl: "./space-dialog.css",
})
export class SpaceDialog {
  protected readonly actions = inject(SpaceActions);
  /** "Creating…" shows only after 1 s; the button is disabled from the first moment (#1041). */
  protected readonly slow = delayedBusy(() => this.actions.busy());
  /** A space was closed in herdr; the host drops the tabs of these panes. */
  readonly closed = output<string[]>();
  protected readonly path = signal("");
  private readonly typedLabel = signal<string | null>(null);

  /** The typed label, else the name of the directory. */
  protected label(): string {
    return this.typedLabel() ?? dirName(this.path());
  }

  protected typePath(event: Event): void {
    const value = (event.target as HTMLInputElement).value;
    this.path.set(value);
    void this.actions.complete(value);
    void this.actions.lookupTemplate(value);
  }

  protected typeLabel(event: Event): void {
    this.typedLabel.set((event.target as HTMLInputElement).value);
  }

  protected async browse(): Promise<void> {
    const dir = await this.actions.pick();
    if (dir) {
      this.path.set(dir);
      void this.actions.lookupTemplate(dir);
    }
  }

  /** The harness id with a capital: `claude` is "Claude". */
  protected agent(id: string): string {
    return id.charAt(0).toUpperCase() + id.slice(1);
  }

  protected async submit(): Promise<void> {
    const outcome = await this.actions.create(this.path(), this.label());
    if (outcome || this.actions.trust()) this.reset();
  }

  protected async confirmClose(): Promise<void> {
    const paneIds = await this.actions.confirmClose();
    if (paneIds) this.closed.emit(paneIds);
  }

  protected async rename(form: HTMLFormElement): Promise<void> {
    await this.actions.confirmRename(String(new FormData(form).get("name") ?? ""));
  }

  protected cancel(): void {
    this.reset();
    this.actions.cancel();
  }

  private reset(): void {
    this.path.set("");
    this.typedLabel.set(null);
    void this.actions.lookupTemplate("");
  }
}
