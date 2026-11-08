import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { ServerRestart } from "../core/server-restart";

/** Confirms a restart of the poimen-server or "Quit poimen and stop server": both end every terminal. */
@Component({
  selector: "app-server-dialog",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./server-dialog.html",
  styleUrl: "./server-dialog.css",
})
export class ServerDialog {
  protected readonly restart = inject(ServerRestart);
}
