import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { ServerRestart } from "../core/server-restart";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";
import { HxButton } from "@gravionlabs/helix-ui";

/**
 * Confirms a restart of the poimen-server or "Quit poimen and stop server": the quit ends every
 * terminal, the restart only those the next server does not adopt.
 */
@Component({
  selector: "app-server-dialog",
  imports: [DialogShell, HxButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./server-dialog.html",
  styleUrl: "./server-dialog.css",
})
export class ServerDialog {
  protected readonly restart = inject(ServerRestart);
}
