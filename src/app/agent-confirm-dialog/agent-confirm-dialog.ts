import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { AgentConfirm } from "../core/agent-confirm";

/**
 * An agent asks to write into another space (ADR 0012, #615): who asks, what and where. Allowing
 * is per call; closing the dialog or Escape declines.
 */
@Component({
  selector: "app-agent-confirm-dialog",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./agent-confirm-dialog.html",
  styleUrl: "./agent-confirm-dialog.css",
})
export class AgentConfirmDialog {
  protected readonly confirm = inject(AgentConfirm);
}
