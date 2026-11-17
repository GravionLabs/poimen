import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { HxButton } from "@gravionlabs/helix-ui";
import { HooksOffer, agentName } from "../core/hooks-offer";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";

/** Offers to install the Claude Code and Copilot CLI hooks of the poimen backend. */
@Component({
  selector: "app-hooks-dialog",
  imports: [HxButton, DialogShell],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./hooks-dialog.html",
  styleUrl: "./hooks-dialog.css",
})
export class HooksDialog {
  protected readonly offer = inject(HooksOffer);
  protected readonly name = agentName;
}
