import { ChangeDetectionStrategy, Component, inject } from "@angular/core";
import { SpaceActions } from "../core/space-actions";

/**
 * Asks whether to trust the project file of a repository before its commands run. Lists every
 * command of the file; nothing was created or run yet.
 */
@Component({
  selector: "app-trust-dialog",
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: "./trust-dialog.html",
  styleUrl: "./trust-dialog.css",
})
export class TrustDialog {
  protected readonly actions = inject(SpaceActions);

  protected answer(answer: "trust" | "without" | "cancel"): void {
    void this.actions.answerTrust(answer);
  }
}
