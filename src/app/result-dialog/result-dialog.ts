import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { ResultDialog as ResultState } from "../core/result-dialog";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";
import { HxButton } from "@gravionlabs/helix-ui";

/** Shows the result of a command (teardown output) or asks for a confirmation. */
@Component({
  selector: "app-result-dialog",
  imports: [DialogShell, HxButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./result-dialog.html",
  styleUrl: "./result-dialog.css",
})
export class ResultDialog {
  protected readonly state = inject(ResultState);
}
