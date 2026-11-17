import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { AgentConfirm } from "../core/agent-confirm";
import { DialogShell } from "../ui/dialog-shell/dialog-shell";
import { HxButton } from "@gravionlabs/helix-ui";

/**
 * An agent asks to write into another space (ADR 0012, #615): who asks, what and where. Allowing
 * is per call; closing the dialog or Escape declines.
 */
@Component({
  selector: "app-agent-confirm-dialog",
  imports: [DialogShell, HxButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./agent-confirm-dialog.html",
  styleUrl: "./agent-confirm-dialog.css",
})
export class AgentConfirmDialog {
  protected readonly confirm = inject(AgentConfirm);
}
