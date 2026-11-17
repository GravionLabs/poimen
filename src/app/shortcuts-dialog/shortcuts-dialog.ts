import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { ShortcutsOverview } from "../core/shortcuts-overview";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";

/** Modal dialog listing the keyboard shortcuts (effective bindings, grouped). */
@Component({
  selector: "app-shortcuts-dialog",
  imports: [DialogShell],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./shortcuts-dialog.html",
  styleUrl: "./shortcuts-dialog.css",
})
export class ShortcutsDialog {
  protected readonly overview = inject(ShortcutsOverview);
}
