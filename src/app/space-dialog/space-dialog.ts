import { ChangeDetectionStrategy, Component, inject, signal } from "@angular/core";
import { SpaceActions, dirName } from "../core/space-actions";

/** The question for a new space: a directory (typed with completion or picked) and a label. */
@Component({
  selector: "app-space-dialog",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./space-dialog.html",
  styleUrl: "./space-dialog.css",
})
export class SpaceDialog {
  protected readonly actions = inject(SpaceActions);
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
  }

  protected typeLabel(event: Event): void {
    this.typedLabel.set((event.target as HTMLInputElement).value);
  }

  protected async browse(): Promise<void> {
    const dir = await this.actions.pick();
    if (dir) this.path.set(dir);
  }

  protected async submit(): Promise<void> {
    const outcome = await this.actions.create(this.path(), this.label());
    if (outcome) this.reset();
  }

  protected cancel(): void {
    this.reset();
    this.actions.cancel();
  }

  private reset(): void {
    this.path.set("");
    this.typedLabel.set(null);
  }
}
