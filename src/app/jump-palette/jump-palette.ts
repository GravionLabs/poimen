import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  effect,
  inject,
  output,
  untracked,
  viewChild,
} from "@angular/core";
import { JumpPalette, type JumpEntry } from "../core/jump-palette";
import { Icon } from "../ui/icon/icon";
import { titleParts } from "../command-palette/title-parts";

/** The jump palette dialog: type to filter, arrows to move, Enter to jump, Escape to close. */
@Component({
  selector: "app-jump-palette",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [Icon],
  templateUrl: "./jump-palette.html",
  styleUrl: "./jump-palette.css",
})
export class JumpPaletteView {
  protected readonly palette = inject(JumpPalette);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** An entry was chosen; the host focuses its pane or opens its space. */
  readonly chosen = output<JumpEntry>();
  private readonly field = viewChild<ElementRef<HTMLInputElement>>("field");
  private returnTo: HTMLElement | null = null;

  constructor() {
    effect(() => {
      if (this.palette.isOpen()) {
        untracked(() => {
          const active = document.activeElement;
          this.returnTo = active instanceof HTMLElement ? active : null;
        });
        this.field()?.nativeElement.focus();
      }
    });
    effect(() => {
      this.palette.selected();
      if (!this.palette.isOpen()) return;
      queueMicrotask(() =>
        this.host.nativeElement.querySelector(".selected")?.scrollIntoView({ block: "nearest" }),
      );
    });
  }

  protected parts(title: string, positions: readonly number[]): { text: string; hit: boolean }[] {
    return titleParts(title, positions);
  }

  protected type(event: Event): void {
    this.palette.setQuery((event.target as HTMLInputElement).value);
  }

  protected key(event: KeyboardEvent): void {
    if (event.key === "ArrowDown") this.palette.move(1);
    else if (event.key === "ArrowUp") this.palette.move(-1);
    else if (event.key === "Enter") {
      const entry = this.palette.current();
      if (entry) this.choose(entry);
    } else if (event.key === "Escape") this.dismiss();
    else return;
    event.preventDefault();
  }

  /** Closes and gives the focus back to where it was. */
  protected dismiss(): void {
    this.palette.close();
    this.returnTo?.focus();
    this.returnTo = null;
  }

  protected choose(entry: JumpEntry): void {
    if (entry.disabled) return;
    this.palette.close();
    this.returnTo = null;
    this.chosen.emit(entry);
  }
}
