import { ChangeDetectionStrategy, Component, ElementRef, effect, inject } from "@angular/core";
import { ShortcutsOverview } from "../core/shortcuts-overview";

/** Native modal dialog listing the keyboard shortcuts (effective bindings, grouped). */
@Component({
  selector: "app-shortcuts-dialog",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./shortcuts-dialog.html",
  styleUrl: "./shortcuts-dialog.css",
})
export class ShortcutsDialog {
  protected readonly overview = inject(ShortcutsOverview);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  constructor() {
    effect(() => {
      const el = this.host.nativeElement.querySelector("dialog");
      if (!el) return;
      if (this.overview.isOpen()) {
        if (!el.open) el.showModal();
      } else if (el.open) el.close();
    });
    // A click on the dialog's own box (the backdrop area) closes it.
    this.host.nativeElement.addEventListener("click", (event) => {
      if ((event.target as HTMLElement).localName === "dialog") this.overview.close();
    });
  }
}
